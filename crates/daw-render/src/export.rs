//! Offline project export to WAV (see spec §8.8).
//!
//! Renders a whole project block-by-block, entirely offline, and writes the
//! stereo master to a WAV file. The render path is the same hardware-
//! independent code used elsewhere ([`daw_engine::render_block`]); only the
//! per-block clip scheduling and file writing live here.
//!
//! No audio device is required. The renderer runs as fast as the host allows
//! (spec §8.8).

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter};
use std::path::Path;

use daw_core::id::TrackId;
use daw_core::project::Project;
use daw_core::time::SampleTime;
use daw_core::track::TrackKind;
use daw_dsp::buffer::AudioBuffer;
use daw_engine::render::render_block;
use daw_engine::schedule::{project_length_frames, schedule_track_block, AssetSource};

/// Options controlling an export.
#[derive(Debug, Clone, Copy)]
pub struct ExportOptions {
    /// Output sample format.
    pub format: crate::wav::WavFormat,
    /// Block size used while rendering. Larger blocks are faster; the result is
    /// identical regardless of block size (verified by test).
    pub block_frames: usize,
    /// Explicit length in frames to render. If `None`, uses the project's clip
    /// content length.
    pub length_frames: Option<SampleTime>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: crate::wav::WavFormat::Pcm16,
            block_frames: 1024,
            length_frames: None,
        }
    }
}

/// Errors that can occur during export.
#[derive(Debug)]
pub enum ExportError {
    /// A rendering error from the engine.
    Render(daw_engine::render::RenderError),
    /// An I/O error writing the output file.
    Io(io::Error),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Render(e) => write!(f, "render error: {e}"),
            ExportError::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<io::Error> for ExportError {
    fn from(e: io::Error) -> Self {
        ExportError::Io(e)
    }
}

impl From<daw_engine::render::RenderError> for ExportError {
    fn from(e: daw_engine::render::RenderError) -> Self {
        ExportError::Render(e)
    }
}

/// Renders the whole project to an in-memory interleaved-free (planar) stereo
/// buffer. Exposed separately so tests can inspect levels without a file.
///
/// # Errors
/// Returns [`ExportError::Render`] if a block fails to render.
pub fn render_project<S: AssetSource>(
    project: &Project,
    source: &S,
    options: &ExportOptions,
) -> Result<AudioBuffer, ExportError> {
    let channels = daw_core::audio::ChannelLayout::default().channel_count();
    let total = options
        .length_frames
        .unwrap_or_else(|| project_length_frames(&project.tracks))
        .frames() as usize;

    let mut out = AudioBuffer::new(channels, total);
    if total == 0 {
        return Ok(out);
    }

    let block = options.block_frames.max(1);
    let mut pos = 0usize;

    // Persistent per-track synth instances for MIDI tracks (envelope/phase
    // continuity across blocks). Created lazily for tracks that have MIDI.
    let mut synths: HashMap<TrackId, daw_dsp::Synth> = HashMap::new();
    let sr = project.sample_rate.hz() as f32;

    while pos < total {
        let this_block = block.min(total - pos);
        let block_start = SampleTime::new(pos as u64);

        // Snapshot the project with automation evaluated at this block's start,
        // so track volumes and effect params reflect automation.
        let snapshot = daw_engine::automated_project(project, block_start);

        // Build per-track source buffers from audio clips + MIDI for this block.
        let mut sources: HashMap<TrackId, AudioBuffer> = HashMap::new();
        for track in &snapshot.tracks {
            if track.kind == TrackKind::Master {
                continue;
            }
            let mut buf = AudioBuffer::new(channels, this_block);
            if !track.clips.is_empty() {
                schedule_track_block(track, source, block_start, &mut buf);
            }
            if !track.midi_clips.is_empty() {
                let synth = synths.entry(track.id).or_insert_with(|| {
                    daw_dsp::Synth::new(sr, 16, daw_dsp::Waveform::Saw, daw_dsp::Adsr::default())
                });
                let mut midi_buf = AudioBuffer::new(channels, this_block);
                daw_engine::schedule_midi_block(track, synth, pos as u64, &mut midi_buf);
                buf.add_from(&midi_buf);
            }
            sources.insert(track.id, buf);
        }

        let rendered = render_block(&snapshot, this_block, &sources)?;
        for ch in 0..channels {
            out.channel_mut(ch)[pos..pos + this_block].copy_from_slice(rendered.channel(ch));
        }
        pos += this_block;
    }

    Ok(out)
}

/// Exports the project to a stereo WAV file at `path`.
///
/// # Errors
/// Returns [`ExportError`] on render or I/O failure.
pub fn export_wav<S: AssetSource>(
    project: &Project,
    source: &S,
    path: impl AsRef<Path>,
    options: &ExportOptions,
) -> Result<(), ExportError> {
    let rendered = render_project(project, source, options)?;
    let channels: Vec<&[f32]> = (0..rendered.channels())
        .map(|ch| rendered.channel(ch))
        .collect();

    let path = path.as_ref();
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    crate::wav::write_wav(writer, &channels, project.sample_rate.hz(), options.format)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_core::clip::AudioClip;
    use daw_core::id::AssetId;
    use daw_core::time::SampleRate;
    use daw_core::track::{Track, TrackKind};

    struct ConstSource {
        value: f32,
        len: u64,
    }
    impl AssetSource for ConstSource {
        fn read(&self, _a: AssetId, _ch: usize, start: SampleTime, out: &mut [f32]) -> usize {
            let avail = self.len.saturating_sub(start.frames()) as usize;
            let n = avail.min(out.len());
            for s in out.iter_mut().take(n) {
                *s = self.value;
            }
            n
        }
    }

    fn project_with_clip(len: u64, value: f32) -> (Project, ConstSource) {
        let rate = SampleRate::new(48_000).unwrap();
        let mut p = Project::new("Export", rate);
        let asset = p.add_asset(daw_core::media::MediaAssetRef::new(
            "a.wav",
            rate,
            daw_core::audio::ChannelLayout::Stereo,
            SampleTime::new(len),
        ));
        let mut track = Track::new(TrackKind::Audio, "A");
        track.add_clip(AudioClip::new(
            asset,
            SampleTime::ZERO,
            SampleTime::new(len),
        ));
        p.add_track(track);
        (p, ConstSource { value, len })
    }

    #[test]
    fn render_project_produces_expected_length_and_level() {
        let (p, src) = project_with_clip(100, 0.5);
        let out = render_project(&p, &src, &ExportOptions::default()).unwrap();
        assert_eq!(out.frames(), 100);
        assert!((out.channel(0)[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn block_size_does_not_affect_result() {
        let (p, src) = project_with_clip(1000, 0.3);
        let a = render_project(
            &p,
            &src,
            &ExportOptions {
                block_frames: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let b = render_project(
            &p,
            &src,
            &ExportOptions {
                block_frames: 777,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn empty_project_renders_zero_length() {
        let rate = SampleRate::new(48_000).unwrap();
        let p = Project::new("Empty", rate);
        let src = ConstSource { value: 0.0, len: 0 };
        let out = render_project(&p, &src, &ExportOptions::default()).unwrap();
        assert_eq!(out.frames(), 0);
    }

    #[test]
    fn export_wav_file_has_expected_size() {
        let (p, src) = project_with_clip(10, 1.0);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.wav");
        export_wav(&p, &src, &path, &ExportOptions::default()).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        // 44-byte header + 10 frames * 2 ch * 2 bytes (PCM16) = 44 + 40 = 84.
        assert_eq!(bytes.len(), 84);
        assert_eq!(&bytes[0..4], b"RIFF");
    }

    #[test]
    fn export_creates_missing_parent_dirs() {
        let (p, src) = project_with_clip(10, 1.0);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/sub/out.wav");
        export_wav(&p, &src, &path, &ExportOptions::default()).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn explicit_length_overrides_content_length() {
        let (p, src) = project_with_clip(10, 1.0);
        let out = render_project(
            &p,
            &src,
            &ExportOptions {
                length_frames: Some(SampleTime::new(25)),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(out.frames(), 25);
        // First 10 frames have signal, rest silent.
        assert!((out.channel(0)[0] - 1.0).abs() < 1e-6);
        assert_eq!(out.channel(0)[20], 0.0);
    }
}
