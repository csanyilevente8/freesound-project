//! Audio-related value types shared across crates: channel layout and gain.
//!
//! These live in `daw-core` because they are part of the stable domain model
//! and must not leak audio-device-library types (see spec §4.3).

use serde::{Deserialize, Serialize};

/// The internal audio sample format for the whole engine is `f32`
/// (see ADR-0004). This alias documents that intent at call sites.
pub type Sample = f32;

/// Channel layout of an audio signal. Kept minimal for Milestone 0; richer
/// multichannel layouts can be added later without breaking existing data
/// because this is a non-exhaustive-friendly explicit enum with a count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelLayout {
    /// Single channel.
    Mono,
    /// Two channels (left, right).
    #[default]
    Stereo,
}

impl ChannelLayout {
    /// Returns the number of channels in this layout.
    #[must_use]
    pub const fn channel_count(self) -> usize {
        match self {
            ChannelLayout::Mono => 1,
            ChannelLayout::Stereo => 2,
        }
    }
}

/// A linear gain factor. 1.0 is unity. Stored linearly (not in dB) so that
/// audio-thread application is a plain multiply with no per-sample `powf`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Gain(f32);

impl Gain {
    /// Unity gain (no change).
    pub const UNITY: Self = Self(1.0);

    /// Silence.
    pub const SILENCE: Self = Self(0.0);

    /// Creates a linear gain, rejecting non-finite or negative values.
    #[must_use]
    pub fn linear(factor: f32) -> Option<Self> {
        if factor.is_finite() && factor >= 0.0 {
            Some(Self(factor))
        } else {
            None
        }
    }

    /// Creates a gain from a decibel value. `-inf`/very low dB maps toward
    /// silence. Non-finite dB (except -inf handled by `powf`) is rejected.
    #[must_use]
    pub fn from_db(db: f32) -> Option<Self> {
        if db.is_nan() || db == f32::INFINITY {
            return None;
        }
        Self::linear(10.0_f32.powf(db / 20.0))
    }

    /// Returns the linear factor.
    #[must_use]
    pub const fn factor(self) -> f32 {
        self.0
    }
}

impl Default for Gain {
    fn default() -> Self {
        Self::UNITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_counts() {
        assert_eq!(ChannelLayout::Mono.channel_count(), 1);
        assert_eq!(ChannelLayout::Stereo.channel_count(), 2);
    }

    #[test]
    fn gain_rejects_invalid() {
        assert!(Gain::linear(-0.1).is_none());
        assert!(Gain::linear(f32::NAN).is_none());
        assert!(Gain::linear(f32::INFINITY).is_none());
        assert!(Gain::linear(0.0).is_some());
    }

    #[test]
    fn gain_db_unity() {
        let g = Gain::from_db(0.0).unwrap();
        assert!((g.factor() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn gain_db_minus_six_is_about_half() {
        let g = Gain::from_db(-6.0206).unwrap();
        assert!((g.factor() - 0.5).abs() < 1e-3);
    }
}
