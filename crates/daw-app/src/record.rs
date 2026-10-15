//! Recording orchestration: input device → file → asset + clip (Slice 2b).
//!
//! Ties the real-time capture core (`daw-audio-io`) to the project model. A
//! [`RecordingSession`] holds the live input stream and writer while recording;
//! [`finish`](RecordingSession::finish) finalizes the WAV, imports it as an
//! asset, and adds the asset + a clip to the project **as one undoable
//! transaction** (so undo removes the references but never the media file —
//! recording-safe undo, spec §13).

use std::path::PathBuf;

use daw_audio_io::{CpalInput, RecordHandle};
use daw_commands::commands::{AddAsset, AddClip};
use daw_commands::History;
use daw_core::clip::AudioClip;
use daw_core::id::TrackId;
use daw_core::project::Project;
use daw_core::time::SampleTime;

/// A live recording in progress. Dropping it stops capture without finalizing.
pub struct RecordingSession {
    _input: CpalInput,
    handle: RecordHandle,
    /// Where the WAV is being written.
    path: PathBuf,
    /// Track to place the resulting clip on.
    track: TrackId,
    /// Timeline position (frames at the project rate) where recording started.
    start_frame: SampleTime,
    /// Device sample rate of the capture (for the elapsed-time readout).
    device_rate: u32,
}

/// Result of finishing a recording.
pub struct RecordingResult {
    /// Status/summary message for the UI.
    pub message: String,
    /// Whether a clip was actually created.
    pub created_clip: bool,
}

impl RecordingSession {
    /// Starts recording the default input device into a WAV file under the
    /// project directory (or a temp dir), to be placed on `track` at
    /// `start_frame`.
    ///
    /// # Errors
    /// Returns a message if no input device is available or the file/stream
    /// cannot be created.
    pub fn start(
        project_dir: Option<&std::path::Path>,
        track: TrackId,
        start_frame: SampleTime,
    ) -> Result<Self, String> {
        let (rate, channels) =
            CpalInput::default_input_params().map_err(|e| format!("no audio input: {e}"))?;

        // Choose a destination path: <project_dir>/recordings/rec-<ts>.wav, or a
        // temp file if there is no saved project yet.
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let file_name = format!("rec-{ts}.wav");
        let path = match project_dir {
            Some(dir) => dir.join("recordings").join(&file_name),
            None => std::env::temp_dir().join(&file_name),
        };

        let sink = daw_audio_io::create_wav_file(&path, channels, rate)
            .map_err(|e| format!("cannot create recording file: {e}"))?;
        // Ring sized for ~1 second of audio to tolerate writer-thread hiccups.
        let ring = (rate as usize) * channels as usize;
        let (producer, handle) = daw_audio_io::start_recording(sink, channels, ring.max(1 << 16));
        let input = CpalInput::start(producer).map_err(|e| format!("cannot open input: {e}"))?;

        Ok(Self {
            _input: input,
            handle,
            path,
            track,
            start_frame,
            device_rate: rate,
        })
    }

    /// Elapsed recorded time in seconds (live readout).
    #[must_use]
    pub fn elapsed_secs(&self) -> f64 {
        self.handle.frames_written() as f64 / f64::from(self.device_rate.max(1))
    }

    /// Current overrun sample count (0 is healthy).
    #[must_use]
    pub fn overruns(&self) -> usize {
        self.handle.overrun_samples()
    }

    /// Stops capture, finalizes the WAV, imports it as an asset, and adds the
    /// asset + a clip to `project` via `history` (one undoable transaction).
    ///
    /// Returns a [`RecordingResult`]; on any failure the project is left
    /// unchanged (the recorded file is preserved on disk regardless).
    pub fn finish(self, project: &mut Project, history: &mut History) -> RecordingResult {
        // Stop the input stream first (drops `_input`), then finalize the writer.
        let RecordingSession {
            _input,
            handle,
            path,
            track,
            start_frame,
            device_rate: _,
        } = self;
        drop(_input); // stop capture before finalizing

        let outcome = handle.finish();
        if let Some(err) = &outcome.error {
            return RecordingResult {
                message: format!("recording error: {err} (file kept at {})", path.display()),
                created_clip: false,
            };
        }
        if outcome.frames_written == 0 {
            return RecordingResult {
                message: "recording stopped (nothing captured)".into(),
                created_clip: false,
            };
        }

        // Import the finalized file at the PROJECT rate (resampling if needed).
        let imported = match daw_media::import_wav_at(&path, project.sample_rate) {
            Ok(i) => i,
            Err(e) => {
                return RecordingResult {
                    message: format!(
                        "recorded, but import failed: {e} (file: {})",
                        path.display()
                    ),
                    created_clip: false,
                }
            }
        };
        let asset = imported.asset;
        let asset_id = asset.id;
        let duration = asset.frames;

        let clip = AudioClip::new(asset_id, start_frame, duration);
        let result = history.transaction(project, "record clip", |proj, tx| {
            tx.execute(
                proj,
                Box::new(AddAsset {
                    asset: asset.clone(),
                }),
            )?;
            tx.execute(
                proj,
                Box::new(AddClip {
                    track,
                    clip: clip.clone(),
                }),
            )
        });

        match result {
            Ok(()) => {
                let warn = if outcome.overrun_samples > 0 {
                    format!(" ({} samples dropped — overrun)", outcome.overrun_samples)
                } else {
                    String::new()
                };
                RecordingResult {
                    message: format!(
                        "recorded {:.2}s clip{warn}",
                        duration.frames() as f64 / f64::from(project.sample_rate.hz())
                    ),
                    created_clip: true,
                }
            }
            Err(e) => RecordingResult {
                message: format!(
                    "recorded, but could not add clip: {e} (file: {})",
                    path.display()
                ),
                created_clip: false,
            },
        }
    }
}
