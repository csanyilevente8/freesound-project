//! Clip scheduling: fill a track's per-block source buffer from its audio
//! clips (see spec §8.4, §9.1, §9.5).
//!
//! This is the offline, hardware-independent half of playback: given a track's
//! clips and a way to read samples from each referenced asset, it writes the
//! audio for a block `[block_start, block_start + frames)` on the timeline into
//! a destination [`AudioBuffer`].
//!
//! Reading asset samples is abstracted behind [`AssetSource`] so the scheduler
//! does not depend on file decoding: tests use a synthetic in-memory source,
//! and a file-backed decoder can implement the same trait later without
//! touching this logic.

use daw_core::clip::AudioClip;
use daw_core::id::AssetId;
use daw_core::time::SampleTime;
use daw_core::track::Track;
use daw_dsp::buffer::AudioBuffer;

/// Supplies decoded samples for assets, one planar channel slice at a time.
pub trait AssetSource {
    /// Reads up to `out.len()` frames of `channel` from `asset`, starting at
    /// `start` frames into the source, into `out`. Returns the number of frames
    /// written; frames beyond the end of the source (or past the asset's
    /// channel count) are left untouched (silence) and not counted.
    fn read(&self, asset: AssetId, channel: usize, start: SampleTime, out: &mut [f32]) -> usize;
}

/// Fills `dst` with the sum of `track`'s clips over the block starting at
/// `block_start`. `dst` is cleared first. Allocation-free aside from what the
/// caller's [`AssetSource`] may do (file-backed sources must keep reads
/// non-blocking on the real-time path — not a concern for offline export).
pub fn schedule_track_block<S: AssetSource>(
    track: &Track,
    source: &S,
    block_start: SampleTime,
    dst: &mut AudioBuffer,
) {
    dst.clear();
    let frames = dst.frames();
    let channels = dst.channels();
    let block_end = block_start.saturating_add(SampleTime::new(frames as u64));

    for clip in &track.clips {
        // Compute the overlap of the clip's timeline range with this block.
        let clip_start = clip.timeline_start;
        let clip_end = clip.timeline_end();
        let overlap_start = clip_start.max(block_start);
        let overlap_end = clip_end.min(block_end);
        if overlap_start >= overlap_end {
            continue; // clip not audible in this block
        }

        let overlap_frames = (overlap_end.frames() - overlap_start.frames()) as usize;
        // Position within the destination block where this clip's audio lands.
        let dst_offset = (overlap_start.frames() - block_start.frames()) as usize;
        // Position within the source asset to start reading from.
        let src_start = SampleTime::new(
            clip.source_offset.frames() + (overlap_start.frames() - clip_start.frames()),
        );

        let clip_gain = clip.gain.factor();

        for ch in 0..channels {
            let dst_slice = &mut dst.channel_mut(ch)[dst_offset..dst_offset + overlap_frames];
            // Offline export may allocate a scratch buffer; the real-time path
            // will preallocate scratch instead (ADR-0007).
            let mut tmp = vec![0.0f32; overlap_frames];
            let read = source.read(clip.asset, ch, src_start, &mut tmp);
            for (d, s) in dst_slice.iter_mut().zip(tmp.iter()).take(read) {
                *d += *s * clip_gain;
            }
        }
    }
}

/// Returns the last timeline frame used by any clip across all tracks, i.e. the
/// project's content length in frames. Zero if there are no clips.
#[must_use]
pub fn project_length_frames(tracks: &[Track]) -> SampleTime {
    tracks
        .iter()
        .flat_map(|t| t.clips.iter())
        .map(AudioClip::timeline_end)
        .max()
        .unwrap_or(SampleTime::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_core::id::AssetId;
    use daw_core::track::{Track, TrackKind};

    /// A synthetic source: every requested sample is a constant per asset.
    struct ConstSource {
        asset: AssetId,
        value: f32,
        len: u64,
    }

    impl AssetSource for ConstSource {
        fn read(&self, asset: AssetId, _ch: usize, start: SampleTime, out: &mut [f32]) -> usize {
            if asset != self.asset {
                return 0;
            }
            let available = self.len.saturating_sub(start.frames()) as usize;
            let n = available.min(out.len());
            for s in out.iter_mut().take(n) {
                *s = self.value;
            }
            n
        }
    }

    fn stereo(frames: usize) -> AudioBuffer {
        AudioBuffer::new(2, frames)
    }

    #[test]
    fn clip_fills_its_timeline_window() {
        let asset = AssetId::new();
        let src = ConstSource {
            asset,
            value: 0.5,
            len: 1000,
        };
        let mut track = Track::new(TrackKind::Audio, "A");
        // Clip at timeline 4, duration 4 -> fills frames [4,8) of a block [0,10).
        track.add_clip(AudioClip::new(
            asset,
            SampleTime::new(4),
            SampleTime::new(4),
        ));

        let mut dst = stereo(10);
        schedule_track_block(&track, &src, SampleTime::ZERO, &mut dst);

        let ch0 = dst.channel(0);
        assert_eq!(&ch0[0..4], &[0.0, 0.0, 0.0, 0.0]);
        assert_eq!(&ch0[4..8], &[0.5, 0.5, 0.5, 0.5]);
        assert_eq!(&ch0[8..10], &[0.0, 0.0]);
    }

    #[test]
    fn clip_partial_overlap_at_block_boundary() {
        let asset = AssetId::new();
        let src = ConstSource {
            asset,
            value: 1.0,
            len: 1000,
        };
        let mut track = Track::new(TrackKind::Audio, "A");
        // Clip [8, 16); block [0,10) should get frames [8,10) filled.
        track.add_clip(AudioClip::new(
            asset,
            SampleTime::new(8),
            SampleTime::new(8),
        ));

        let mut dst = stereo(10);
        schedule_track_block(&track, &src, SampleTime::ZERO, &mut dst);
        assert_eq!(&dst.channel(0)[8..10], &[1.0, 1.0]);
        assert_eq!(dst.channel(0)[7], 0.0);
    }

    #[test]
    fn source_offset_is_respected_via_shorter_source() {
        // Source only has 2 frames; a 4-frame clip reading from offset 0 should
        // fill 2 frames then go silent (source exhausted).
        let asset = AssetId::new();
        let src = ConstSource {
            asset,
            value: 1.0,
            len: 2,
        };
        let mut track = Track::new(TrackKind::Audio, "A");
        track.add_clip(AudioClip::new(asset, SampleTime::ZERO, SampleTime::new(4)));

        let mut dst = stereo(4);
        schedule_track_block(&track, &src, SampleTime::ZERO, &mut dst);
        assert_eq!(dst.channel(0), &[1.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn project_length_is_max_clip_end() {
        let asset = AssetId::new();
        let mut t = Track::new(TrackKind::Audio, "A");
        t.add_clip(AudioClip::new(
            asset,
            SampleTime::new(0),
            SampleTime::new(100),
        ));
        t.add_clip(AudioClip::new(
            asset,
            SampleTime::new(50),
            SampleTime::new(200),
        ));
        assert_eq!(project_length_frames(&[t]), SampleTime::new(250));
    }
}
