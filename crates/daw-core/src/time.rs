//! Time representation (see spec §5.4 and ADR-0006).
//!
//! Two authoritative, integer-based clocks are used:
//!
//! * [`SampleTime`] — a position in sample frames at a known [`SampleRate`].
//!   This is the authority for audio processing.
//! * [`MusicalTime`] — a position in pulses-per-quarter-note (PPQ) ticks.
//!   This is the authority for musical editing.
//!
//! Floating-point seconds are deliberately **not** used as an authoritative
//! representation, to avoid cumulative rounding error. Conversions to/from
//! seconds are provided for display and device interop only.

use serde::{Deserialize, Serialize};

/// Standard pulses-per-quarter-note resolution for musical time.
///
/// 960 PPQ is a widely used value that divides cleanly into common note
/// subdivisions (triplets, 32nd notes, etc.).
pub const PPQ: u32 = 960;

/// A sample rate in Hz (frames per second). Always strictly positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SampleRate(u32);

impl SampleRate {
    /// Creates a sample rate, returning `None` if `hz == 0`.
    #[must_use]
    pub const fn new(hz: u32) -> Option<Self> {
        if hz == 0 {
            None
        } else {
            Some(Self(hz))
        }
    }

    /// Returns the sample rate in Hz.
    #[must_use]
    pub const fn hz(self) -> u32 {
        self.0
    }
}

impl Default for SampleRate {
    fn default() -> Self {
        Self(48_000)
    }
}

/// A position or duration in sample frames. Non-negative by construction when
/// created via [`SampleTime::new`]; the raw constructor allows any `u64`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct SampleTime(u64);

impl SampleTime {
    /// The zero position.
    pub const ZERO: Self = Self(0);

    /// Creates a sample-time from a frame count.
    #[must_use]
    pub const fn new(frames: u64) -> Self {
        Self(frames)
    }

    /// Returns the frame count.
    #[must_use]
    pub const fn frames(self) -> u64 {
        self.0
    }

    /// Converts to seconds at the given sample rate (for display/interop only).
    #[must_use]
    pub fn to_seconds(self, rate: SampleRate) -> f64 {
        self.0 as f64 / f64::from(rate.hz())
    }

    /// Converts seconds to the nearest whole frame at the given sample rate.
    ///
    /// Negative inputs are clamped to zero. Rounding is round-half-to-even via
    /// [`f64::round`] semantics applied after scaling.
    #[must_use]
    pub fn from_seconds(seconds: f64, rate: SampleRate) -> Self {
        if seconds <= 0.0 {
            return Self::ZERO;
        }
        let frames = (seconds * f64::from(rate.hz())).round();
        Self(frames as u64)
    }

    /// Saturating addition.
    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Saturating subtraction (never underflows below zero).
    #[must_use]
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

/// A position or duration in PPQ ticks ([`PPQ`] ticks per quarter note).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct MusicalTime(u64);

impl MusicalTime {
    /// The zero position.
    pub const ZERO: Self = Self(0);

    /// Creates a musical time from a raw tick count.
    #[must_use]
    pub const fn from_ticks(ticks: u64) -> Self {
        Self(ticks)
    }

    /// Creates a musical time from a whole number of quarter notes (beats).
    #[must_use]
    pub const fn from_quarter_notes(beats: u64) -> Self {
        Self(beats * PPQ as u64)
    }

    /// Returns the raw tick count.
    #[must_use]
    pub const fn ticks(self) -> u64 {
        self.0
    }

    /// Converts musical time to sample time at a fixed tempo.
    ///
    /// This assumes a constant tempo; tempo automation is deferred to a later
    /// milestone (spec §5.4). The result is rounded to the nearest frame.
    #[must_use]
    pub fn to_samples(self, tempo: Tempo, rate: SampleRate) -> SampleTime {
        // seconds = ticks / PPQ * (60 / bpm)
        let quarter_notes = self.0 as f64 / f64::from(PPQ);
        let seconds = quarter_notes * (60.0 / tempo.bpm());
        SampleTime::from_seconds(seconds, rate)
    }
}

/// A tempo in beats (quarter notes) per minute. Always strictly positive and
/// finite.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tempo(f64);

impl Tempo {
    /// Creates a tempo, returning `None` if `bpm` is not finite and positive.
    #[must_use]
    pub fn new(bpm: f64) -> Option<Self> {
        if bpm.is_finite() && bpm > 0.0 {
            Some(Self(bpm))
        } else {
            None
        }
    }

    /// Returns the tempo in BPM.
    #[must_use]
    pub const fn bpm(self) -> f64 {
        self.0
    }
}

impl Default for Tempo {
    fn default() -> Self {
        Self(120.0)
    }
}

/// A time signature such as 4/4. Both fields are strictly positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignature {
    /// Beats per bar (numerator).
    pub numerator: u16,
    /// Note value that represents one beat (denominator), e.g. 4 = quarter.
    pub denominator: u16,
}

impl TimeSignature {
    /// Creates a time signature, returning `None` if either field is zero.
    #[must_use]
    pub const fn new(numerator: u16, denominator: u16) -> Option<Self> {
        if numerator == 0 || denominator == 0 {
            None
        } else {
            Some(Self {
                numerator,
                denominator,
            })
        }
    }
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_rate_rejects_zero() {
        assert!(SampleRate::new(0).is_none());
        assert_eq!(SampleRate::new(44_100).unwrap().hz(), 44_100);
    }

    #[test]
    fn seconds_round_trip_is_frame_exact() {
        let rate = SampleRate::new(48_000).unwrap();
        let t = SampleTime::new(96_000); // exactly 2.0s
        assert!((t.to_seconds(rate) - 2.0).abs() < f64::EPSILON);
        assert_eq!(SampleTime::from_seconds(2.0, rate), t);
    }

    #[test]
    fn from_seconds_clamps_negative() {
        let rate = SampleRate::default();
        assert_eq!(SampleTime::from_seconds(-5.0, rate), SampleTime::ZERO);
    }

    #[test]
    fn musical_to_samples_one_beat_at_120bpm() {
        // 120 bpm => 0.5s per quarter note. At 48k => 24000 frames.
        let rate = SampleRate::new(48_000).unwrap();
        let tempo = Tempo::new(120.0).unwrap();
        let one_beat = MusicalTime::from_quarter_notes(1);
        assert_eq!(one_beat.to_samples(tempo, rate), SampleTime::new(24_000));
    }

    #[test]
    fn musical_to_samples_bar_at_60bpm() {
        // 60 bpm => 1s per beat. 4 beats => 4s. At 44100 => 176400 frames.
        let rate = SampleRate::new(44_100).unwrap();
        let tempo = Tempo::new(60.0).unwrap();
        let four_beats = MusicalTime::from_quarter_notes(4);
        assert_eq!(four_beats.to_samples(tempo, rate), SampleTime::new(176_400));
    }

    #[test]
    fn tempo_rejects_invalid() {
        assert!(Tempo::new(0.0).is_none());
        assert!(Tempo::new(-1.0).is_none());
        assert!(Tempo::new(f64::NAN).is_none());
        assert!(Tempo::new(f64::INFINITY).is_none());
    }

    #[test]
    fn time_signature_rejects_zero() {
        assert!(TimeSignature::new(0, 4).is_none());
        assert!(TimeSignature::new(4, 0).is_none());
        assert!(TimeSignature::new(7, 8).is_some());
    }

    #[test]
    fn saturating_ops() {
        assert_eq!(
            SampleTime::new(5).saturating_sub(SampleTime::new(10)),
            SampleTime::ZERO
        );
        assert_eq!(
            SampleTime::new(u64::MAX).saturating_add(SampleTime::new(1)),
            SampleTime::new(u64::MAX)
        );
    }
}
