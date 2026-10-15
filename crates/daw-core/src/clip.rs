//! Audio clip model (see spec §5.3, §8.4).
//!
//! A clip is a **non-destructive** reference to a region of a source asset,
//! placed on the timeline. It distinguishes three independent quantities:
//!
//! * `timeline_start` — where the clip begins on the arrangement timeline.
//! * `source_offset`  — where playback starts *within* the source asset.
//! * `duration`       — how many frames the clip plays.
//!
//! Moving, trimming, and splitting only change these numbers; the source asset
//! is never rewritten (spec §5.3, §12). Splitting produces two clips that
//! reference the same asset over adjacent source regions.

use serde::{Deserialize, Serialize};

use crate::audio::Gain;
use crate::id::{AssetId, ClipId};
use crate::time::SampleTime;
use crate::validate::{Validate, ValidationError};

/// A non-destructive audio clip on a track's timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioClip {
    /// Stable identifier.
    pub id: ClipId,
    /// The source asset this clip plays from.
    pub asset: AssetId,
    /// Timeline position where the clip starts (in frames at project rate).
    pub timeline_start: SampleTime,
    /// Offset into the source asset where playback begins.
    pub source_offset: SampleTime,
    /// Number of frames the clip plays.
    pub duration: SampleTime,
    /// Per-clip linear gain applied to the clip's audio.
    pub gain: Gain,
}

impl AudioClip {
    /// Creates a clip that plays `duration` frames of `asset` starting at
    /// `timeline_start`, reading from the beginning of the source.
    #[must_use]
    pub fn new(asset: AssetId, timeline_start: SampleTime, duration: SampleTime) -> Self {
        Self {
            id: ClipId::new(),
            asset,
            timeline_start,
            source_offset: SampleTime::ZERO,
            duration,
            gain: Gain::UNITY,
        }
    }

    /// The timeline position one past the clip's last frame (exclusive end).
    #[must_use]
    pub fn timeline_end(&self) -> SampleTime {
        self.timeline_start.saturating_add(self.duration)
    }

    /// Returns `true` if this clip's timeline range overlaps `other`'s.
    /// Ranges are half-open `[start, end)`.
    #[must_use]
    pub fn overlaps(&self, other: &AudioClip) -> bool {
        self.timeline_start < other.timeline_end() && other.timeline_start < self.timeline_end()
    }

    /// Trims `frames` off the **start** of the clip, advancing both the
    /// timeline position and the source offset so the audio under the new
    /// start stays aligned. Clamped so the clip never inverts.
    pub fn trim_start(&mut self, frames: SampleTime) {
        let delta = frames.frames().min(self.duration.frames());
        let delta = SampleTime::new(delta);
        self.timeline_start = self.timeline_start.saturating_add(delta);
        self.source_offset = self.source_offset.saturating_add(delta);
        self.duration = self.duration.saturating_sub(delta);
    }

    /// Trims `frames` off the **end** of the clip (reduces duration only).
    pub fn trim_end(&mut self, frames: SampleTime) {
        self.duration = self.duration.saturating_sub(frames);
    }

    /// Splits the clip at an absolute `timeline` position, returning the new
    /// right-hand clip. `self` becomes the left-hand part.
    ///
    /// Returns `None` if the split point is at or outside the clip's range, in
    /// which case nothing is modified (there would be an empty piece).
    #[must_use]
    pub fn split_at(&mut self, timeline: SampleTime) -> Option<AudioClip> {
        if timeline <= self.timeline_start || timeline >= self.timeline_end() {
            return None;
        }
        let left_frames = timeline.frames() - self.timeline_start.frames();
        let left_frames = SampleTime::new(left_frames);
        let right_duration = self.duration.saturating_sub(left_frames);

        let right = AudioClip {
            id: ClipId::new(),
            asset: self.asset,
            timeline_start: timeline,
            source_offset: self.source_offset.saturating_add(left_frames),
            duration: right_duration,
            gain: self.gain,
        };

        self.duration = left_frames;
        Some(right)
    }
}

impl Validate for AudioClip {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.duration == SampleTime::ZERO {
            return Err(ValidationError::new("audio clip duration must be > 0"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(start: u64, dur: u64) -> AudioClip {
        AudioClip::new(AssetId::new(), SampleTime::new(start), SampleTime::new(dur))
    }

    #[test]
    fn timeline_end_is_start_plus_duration() {
        let c = clip(100, 50);
        assert_eq!(c.timeline_end(), SampleTime::new(150));
    }

    #[test]
    fn overlap_detection() {
        let a = clip(0, 100);
        let b = clip(50, 100);
        let c = clip(100, 100); // starts exactly where a ends -> no overlap
        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&c));
    }

    #[test]
    fn trim_start_keeps_source_aligned() {
        let mut c = clip(100, 100);
        c.trim_start(SampleTime::new(30));
        assert_eq!(c.timeline_start, SampleTime::new(130));
        assert_eq!(c.source_offset, SampleTime::new(30));
        assert_eq!(c.duration, SampleTime::new(70));
    }

    #[test]
    fn trim_start_clamps_to_duration() {
        let mut c = clip(0, 40);
        c.trim_start(SampleTime::new(1000));
        assert_eq!(c.duration, SampleTime::ZERO);
    }

    #[test]
    fn trim_end_reduces_duration_only() {
        let mut c = clip(100, 100);
        c.trim_end(SampleTime::new(40));
        assert_eq!(c.timeline_start, SampleTime::new(100));
        assert_eq!(c.duration, SampleTime::new(60));
    }

    #[test]
    fn split_produces_adjacent_regions_over_same_asset() {
        let mut left = clip(100, 100);
        left.source_offset = SampleTime::new(10);
        let asset = left.asset;
        let right = left.split_at(SampleTime::new(140)).unwrap();

        // Left: 100..140 (40 frames), source 10..50
        assert_eq!(left.duration, SampleTime::new(40));
        assert_eq!(left.source_offset, SampleTime::new(10));
        // Right: 140..200 (60 frames), source 50..110
        assert_eq!(right.timeline_start, SampleTime::new(140));
        assert_eq!(right.duration, SampleTime::new(60));
        assert_eq!(right.source_offset, SampleTime::new(50));
        // Same source asset.
        assert_eq!(right.asset, asset);
        // No gap/overlap at the seam.
        assert_eq!(left.timeline_end(), right.timeline_start);
    }

    #[test]
    fn split_outside_range_is_noop() {
        let mut c = clip(100, 100);
        assert!(c.split_at(SampleTime::new(100)).is_none()); // at start
        assert!(c.split_at(SampleTime::new(200)).is_none()); // at end
        assert!(c.split_at(SampleTime::new(50)).is_none()); // before
        assert_eq!(c.duration, SampleTime::new(100)); // unchanged
    }

    #[test]
    fn zero_duration_clip_is_invalid() {
        let c = clip(0, 0);
        assert!(c.validate().is_err());
    }
}
