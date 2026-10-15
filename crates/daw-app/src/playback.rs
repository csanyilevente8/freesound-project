//! Shared playback helper used by both the CLI (`daw play`) and the GUI.
//!
//! Renders a whole project offline at the **device** sample rate (so playback
//! is correct speed regardless of the project rate), starts a CPAL output
//! stream, and returns a live [`LivePlayback`] holding the control handle and
//! the stream. Dropping `LivePlayback` stops audio.
//!
//! This keeps the "project -> audible" wiring in one place so the GUI and CLI
//! behave identically.

use daw_audio_io::{CpalOutput, PlaybackHandle, PreparedBuffer};
use daw_core::project::Project;
use daw_core::time::SampleRate;
use daw_render::ExportOptions;

use crate::cli; // reuse source_for via a thin re-export below

/// A running live-playback session.
pub struct LivePlayback {
    /// Control-side handle (send Play/Stop/Seek/gain, poll meter/end events).
    pub handle: PlaybackHandle,
    /// Device sample rate the material was rendered/played at.
    pub device_rate: u32,
    /// Total length in frames.
    pub total_frames: u64,
    // Keeps the CPAL stream alive; dropping stops playback.
    _stream: CpalOutput,
}

impl LivePlayback {
    /// Length in seconds.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        self.total_frames as f64 / f64::from(self.device_rate.max(1))
    }
}

/// Renders `project` at the default output device's rate and starts a stream.
///
/// `base_dir` is used to resolve relative media paths (typically the directory
/// of the project file) so playback works regardless of the current directory.
///
/// # Errors
/// Returns a human-readable error if there is no output device, re-import of a
/// referenced asset fails, rendering fails, or the stream cannot start.
pub fn start(
    project: &Project,
    base_dir: Option<&std::path::Path>,
) -> Result<LivePlayback, String> {
    let (device_rate, _ch) =
        CpalOutput::default_output_params().map_err(|e| format!("no audio output: {e}"))?;

    let mut at_rate = project.clone();
    at_rate.sample_rate = SampleRate::new(device_rate).ok_or("device reported 0 Hz")?;

    let source = cli::source_for_at(&at_rate, base_dir)?;
    let rendered = daw_render::render_project(&at_rate, &source, &ExportOptions::default())
        .map_err(|e| format!("render failed: {e}"))?;

    let total_frames = rendered.frames() as u64;
    let prepared = PreparedBuffer::new(rendered.channel(0).to_vec(), rendered.channel(1).to_vec());

    let meter_interval = (u64::from(device_rate) / 30).max(1); // ~30 updates/sec
    let (player, handle) = daw_audio_io::playback_channel(prepared, 4096, meter_interval);
    let stream = CpalOutput::start(player).map_err(|e| format!("device start failed: {e}"))?;

    Ok(LivePlayback {
        handle,
        device_rate,
        total_frames,
        _stream: stream,
    })
}
