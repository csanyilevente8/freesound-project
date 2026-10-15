//! Offline rendering path (see spec §8.8 and §9.1).
//!
//! This module renders a [`Project`] to an in-memory [`AudioBuffer`] **without
//! any audio hardware**. It is the hardware-independent core that the future
//! real-time engine and the WAV exporter will both build on.
//!
//! Milestone 0 scope: each non-master track contributes a caller-supplied
//! source buffer (there is no clip/media model yet), gets its track volume
//! applied, is summed into the master bus, and finally the master track volume
//! is applied. Mute/solo are honored. This exercises the full
//! project → gain → mix → master pipeline deterministically.
//!
//! Real-time safety note: this path intentionally allocates (it is offline and
//! runs off the audio thread). The *shared* DSP primitives it uses
//! ([`GainProcessor`], [`mix_into`]) are allocation-free in `process`, so the
//! same primitives can be reused by the real-time engine later.

use std::collections::HashMap;

use daw_core::audio::ChannelLayout;
use daw_core::id::TrackId;
use daw_core::project::Project;
use daw_core::track::TrackKind;
use daw_dsp::buffer::AudioBuffer;
use daw_dsp::gain::GainProcessor;
use daw_dsp::mixer::mix_into;
use daw_dsp::processor::{AudioProcessor, ProcessConfig};

/// Errors that can occur while rendering a project offline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// A supplied source buffer did not match the render block's frame count.
    FrameCountMismatch {
        /// The offending track.
        track: TrackId,
        /// Frames expected by the render block.
        expected: usize,
        /// Frames found in the supplied source buffer.
        found: usize,
    },
    /// A supplied source buffer references a track that is not in the project.
    UnknownTrack(TrackId),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::FrameCountMismatch {
                track,
                expected,
                found,
            } => write!(
                f,
                "track {track} source has {found} frames, expected {expected}"
            ),
            RenderError::UnknownTrack(id) => write!(f, "unknown track {id}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Renders one block of `frames` samples for `project`.
///
/// `sources` maps a track id to that track's dry source buffer for this block.
/// Tracks without an entry contribute silence. The output has the project's
/// default channel layout ([`ChannelLayout::Stereo`]).
///
/// Solo handling: if any non-master track is soloed, only soloed tracks are
/// audible. Muted tracks are always silent.
///
/// # Errors
/// Returns [`RenderError`] if a source references an unknown track or has a
/// mismatched frame count.
pub fn render_block(
    project: &Project,
    frames: usize,
    sources: &HashMap<TrackId, AudioBuffer>,
) -> Result<AudioBuffer, RenderError> {
    let channels = ChannelLayout::default().channel_count();
    let config = ProcessConfig {
        sample_rate: project.sample_rate,
        max_block_frames: frames,
        channels,
    };

    // Validate supplied sources up front.
    for (id, buf) in sources {
        if project.track(*id).is_none() {
            return Err(RenderError::UnknownTrack(*id));
        }
        if buf.frames() != frames {
            return Err(RenderError::FrameCountMismatch {
                track: *id,
                expected: frames,
                found: buf.frames(),
            });
        }
    }

    let any_solo = project
        .tracks
        .iter()
        .any(|t| t.kind != TrackKind::Master && t.solo);

    // Build per-track contributions into the master sum.
    let mut contributions: Vec<AudioBuffer> = Vec::new();
    for track in &project.tracks {
        if track.kind == TrackKind::Master {
            continue;
        }
        let audible = !track.mute && (!any_solo || track.solo);
        if !audible {
            continue;
        }

        // Start from the track's dry source (or silence).
        let mut buf = match sources.get(&track.id) {
            Some(src) => src.clone(),
            None => AudioBuffer::new(channels, frames),
        };

        // Insert-effect chain processes the dry signal before the fader
        // (bypassed/unknown effects are skipped). Spec §8.6 signal flow:
        // source -> inserts -> track mixer (volume).
        if !track.effects.is_empty() {
            crate::effects::process_chain(&track.effects, &config, &mut buf);
        }

        // Apply track volume via the shared gain processor.
        let mut gain = GainProcessor::new(track.volume);
        gain.prepare(&config);
        gain.process(&mut buf);

        contributions.push(buf);
    }

    // Sum into the master bus.
    let mut master = AudioBuffer::new(channels, frames);
    mix_into(&mut master, &contributions);

    // Apply master track volume, then its insert chain (master bus effects).
    if let Some(master_track) = project.master() {
        let mut gain = GainProcessor::new(master_track.volume);
        gain.prepare(&config);
        gain.process(&mut master);
        if !master_track.effects.is_empty() {
            crate::effects::process_chain(&master_track.effects, &config, &mut master);
        }
    }

    Ok(master)
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_core::audio::Gain;
    use daw_core::time::SampleRate;
    use daw_core::track::Track;

    fn stereo_const(frames: usize, value: f32) -> AudioBuffer {
        let mut b = AudioBuffer::new(2, frames);
        b.channel_mut(0).fill(value);
        b.channel_mut(1).fill(value);
        b
    }

    fn project() -> Project {
        Project::new("Mix", SampleRate::new(48_000).unwrap())
    }

    #[test]
    fn empty_project_renders_silence() {
        let p = project();
        let out = render_block(&p, 16, &HashMap::new()).unwrap();
        assert_eq!(out.channels(), 2);
        assert_eq!(out.frames(), 16);
        assert_eq!(out.peak(), 0.0);
    }

    #[test]
    fn two_tracks_sum_at_master() {
        let mut p = project();
        let a = p.add_track(Track::new(TrackKind::Audio, "A"));
        let b = p.add_track(Track::new(TrackKind::Audio, "B"));
        let mut sources = HashMap::new();
        sources.insert(a, stereo_const(8, 0.25));
        sources.insert(b, stereo_const(8, 0.25));
        let out = render_block(&p, 8, &sources).unwrap();
        assert!((out.channel(0)[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn track_volume_is_applied_before_mix() {
        let mut p = project();
        let id = {
            let mut t = Track::new(TrackKind::Audio, "A");
            t.volume = Gain::linear(0.5).unwrap();
            p.add_track(t)
        };
        let mut sources = HashMap::new();
        sources.insert(id, stereo_const(4, 1.0));
        let out = render_block(&p, 4, &sources).unwrap();
        assert!((out.channel(0)[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn master_volume_is_applied_last() {
        let mut p = project();
        if let Some(master) = p.tracks.iter_mut().find(|t| t.kind == TrackKind::Master) {
            master.volume = Gain::linear(0.5).unwrap();
        }
        let id = p.add_track(Track::new(TrackKind::Audio, "A"));
        let mut sources = HashMap::new();
        sources.insert(id, stereo_const(4, 1.0));
        let out = render_block(&p, 4, &sources).unwrap();
        assert!((out.channel(0)[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mute_silences_track() {
        let mut p = project();
        let id = {
            let mut t = Track::new(TrackKind::Audio, "A");
            t.mute = true;
            p.add_track(t)
        };
        let mut sources = HashMap::new();
        sources.insert(id, stereo_const(4, 1.0));
        let out = render_block(&p, 4, &sources).unwrap();
        assert_eq!(out.peak(), 0.0);
    }

    #[test]
    fn solo_isolates_soloed_tracks() {
        let mut p = project();
        let soloed = {
            let mut t = Track::new(TrackKind::Audio, "Solo");
            t.solo = true;
            p.add_track(t)
        };
        let other = p.add_track(Track::new(TrackKind::Audio, "Other"));
        let mut sources = HashMap::new();
        sources.insert(soloed, stereo_const(4, 0.3));
        sources.insert(other, stereo_const(4, 0.7));
        let out = render_block(&p, 4, &sources).unwrap();
        assert!((out.channel(0)[0] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn unknown_track_source_is_rejected() {
        let p = project();
        let mut sources = HashMap::new();
        sources.insert(TrackId::new(), stereo_const(4, 1.0));
        assert!(matches!(
            render_block(&p, 4, &sources),
            Err(RenderError::UnknownTrack(_))
        ));
    }

    #[test]
    fn frame_mismatch_is_rejected() {
        let mut p = project();
        let id = p.add_track(Track::new(TrackKind::Audio, "A"));
        let mut sources = HashMap::new();
        sources.insert(id, stereo_const(3, 1.0));
        assert!(matches!(
            render_block(&p, 4, &sources),
            Err(RenderError::FrameCountMismatch { .. })
        ));
    }
}
