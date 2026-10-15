//! MIDI note and clip model (see spec §5.3, §11).
//!
//! A [`MidiClip`] holds note events on a track's timeline, analogous to an
//! [`AudioClip`](crate::clip::AudioClip) but containing synthesized note data
//! rather than referencing recorded media.
//!
//! **Timing:** note positions and durations are stored in **sample frames**,
//! and a note's `start` is relative to the clip's `timeline_start`. This keeps
//! MIDI scheduling consistent with the frame-based audio engine for the first
//! instrument slice. (A musical-time, tempo-relative representation can be
//! layered on later when tempo-relative scheduling lands — the time model
//! already has `MusicalTime` for that.)
//!
//! **Pitch:** MIDI note numbers 0..=127 (60 = middle C). **Velocity:** 0..=127.

use serde::{Deserialize, Serialize};

use crate::id::ClipId;
use crate::time::SampleTime;
use crate::validate::{Validate, ValidationError};

/// A single MIDI note event within a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiNote {
    /// MIDI note number, 0..=127 (60 = middle C).
    pub pitch: u8,
    /// Velocity, 0..=127.
    pub velocity: u8,
    /// Start position in frames, relative to the clip's `timeline_start`.
    pub start: SampleTime,
    /// Note duration in frames (> 0).
    pub duration: SampleTime,
}

impl MidiNote {
    /// Creates a note. Pitch/velocity are clamped to the valid 0..=127 range.
    #[must_use]
    pub fn new(pitch: u8, velocity: u8, start: SampleTime, duration: SampleTime) -> Self {
        Self {
            pitch: pitch.min(127),
            velocity: velocity.min(127),
            start,
            duration,
        }
    }

    /// Frame one past the note's end (exclusive), relative to the clip start.
    #[must_use]
    pub fn end(&self) -> SampleTime {
        self.start.saturating_add(self.duration)
    }
}

/// A MIDI clip: note events placed on a track's timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidiClip {
    /// Stable identifier.
    pub id: ClipId,
    /// Timeline position where the clip starts (frames at project rate).
    pub timeline_start: SampleTime,
    /// Clip length in frames. Notes beyond this are not played.
    pub duration: SampleTime,
    /// Note events, sorted ascending by `start` (enforced by `add_note`).
    notes: Vec<MidiNote>,
}

impl MidiClip {
    /// Creates an empty MIDI clip at `timeline_start` with the given length.
    #[must_use]
    pub fn new(timeline_start: SampleTime, duration: SampleTime) -> Self {
        Self {
            id: ClipId::new(),
            timeline_start,
            duration,
            notes: Vec::new(),
        }
    }

    /// Read-only view of the notes (sorted by start).
    #[must_use]
    pub fn notes(&self) -> &[MidiNote] {
        &self.notes
    }

    /// The timeline position one past the clip's last frame (exclusive).
    #[must_use]
    pub fn timeline_end(&self) -> SampleTime {
        self.timeline_start.saturating_add(self.duration)
    }

    /// Inserts a note, keeping the note list sorted by start time.
    pub fn add_note(&mut self, note: MidiNote) {
        let idx = self.notes.partition_point(|n| n.start <= note.start);
        self.notes.insert(idx, note);
    }

    /// Removes the first note matching `pitch` and `start` exactly, returning it.
    pub fn remove_note(&mut self, pitch: u8, start: SampleTime) -> Option<MidiNote> {
        let idx = self
            .notes
            .iter()
            .position(|n| n.pitch == pitch && n.start == start)?;
        Some(self.notes.remove(idx))
    }
}

impl Validate for MidiClip {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.duration == SampleTime::ZERO {
            return Err(ValidationError::new("midi clip duration must be > 0"));
        }
        for n in &self.notes {
            if n.duration == SampleTime::ZERO {
                return Err(ValidationError::new("midi note duration must be > 0"));
            }
            if n.pitch > 127 || n.velocity > 127 {
                return Err(ValidationError::new("midi pitch/velocity out of range"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_clamps_and_computes_end() {
        let n = MidiNote::new(200, 200, SampleTime::new(10), SampleTime::new(40));
        assert_eq!(n.pitch, 127);
        assert_eq!(n.velocity, 127);
        assert_eq!(n.end(), SampleTime::new(50));
    }

    #[test]
    fn add_note_keeps_sorted() {
        let mut c = MidiClip::new(SampleTime::ZERO, SampleTime::new(1000));
        c.add_note(MidiNote::new(
            60,
            100,
            SampleTime::new(500),
            SampleTime::new(100),
        ));
        c.add_note(MidiNote::new(
            62,
            100,
            SampleTime::new(0),
            SampleTime::new(100),
        ));
        c.add_note(MidiNote::new(
            64,
            100,
            SampleTime::new(250),
            SampleTime::new(100),
        ));
        let starts: Vec<u64> = c.notes().iter().map(|n| n.start.frames()).collect();
        assert_eq!(starts, vec![0, 250, 500]);
    }

    #[test]
    fn remove_note() {
        let mut c = MidiClip::new(SampleTime::ZERO, SampleTime::new(1000));
        c.add_note(MidiNote::new(
            60,
            100,
            SampleTime::new(0),
            SampleTime::new(100),
        ));
        assert!(c.remove_note(60, SampleTime::ZERO).is_some());
        assert!(c.remove_note(60, SampleTime::ZERO).is_none());
        assert!(c.notes().is_empty());
    }

    #[test]
    fn validate_rejects_zero_durations() {
        let c = MidiClip::new(SampleTime::ZERO, SampleTime::ZERO);
        assert!(c.validate().is_err());

        let mut c2 = MidiClip::new(SampleTime::ZERO, SampleTime::new(100));
        c2.add_note(MidiNote::new(60, 100, SampleTime::ZERO, SampleTime::ZERO));
        assert!(c2.validate().is_err());
    }

    #[test]
    fn serde_round_trip() {
        let mut c = MidiClip::new(SampleTime::new(48_000), SampleTime::new(96_000));
        c.add_note(MidiNote::new(
            60,
            100,
            SampleTime::new(0),
            SampleTime::new(24_000),
        ));
        c.add_note(MidiNote::new(
            64,
            90,
            SampleTime::new(24_000),
            SampleTime::new(24_000),
        ));
        let json = serde_json::to_string(&c).unwrap();
        let back: MidiClip = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }
}
