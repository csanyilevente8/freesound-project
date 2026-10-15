//! Track domain model (see spec §5.2).
//!
//! Milestone 0 models the minimum needed to represent a project and exercise
//! the offline mixing path: identity, naming, ordering, mute/solo, and gain.
//! Clips, routing, effects, and automation are intentionally deferred.

use serde::{Deserialize, Serialize};

use crate::audio::Gain;
use crate::clip::AudioClip;
use crate::effect::EffectChain;
use crate::id::TrackId;

/// The kind of a track. Determines how it participates in the signal flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    /// Plays audio clips (and, later, records from an input).
    Audio,
    /// Holds MIDI clips and feeds a built-in instrument.
    Midi,
    /// Sums routed signals; may host effects.
    Bus,
    /// Final output processing and metering. Exactly one per project.
    Master,
}

/// A track within a project.
///
/// `volume` is a linear [`Gain`]; `mute`/`solo` are persistent mix state.
/// Record-arm and monitoring are *operational* state and are deliberately not
/// stored here (spec §5.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Stable identifier.
    pub id: TrackId,
    /// Track kind.
    pub kind: TrackKind,
    /// User-visible name. May change freely; references use [`TrackId`].
    pub name: String,
    /// Linear output gain applied during mixing.
    pub volume: Gain,
    /// Stereo pan position in [-1, 1] (0 = center). Applied during mixing via a
    /// constant-power pan law.
    #[serde(default)]
    pub pan: f32,
    /// Optional display color (e.g. "#73C7B8"). Cosmetic; UI-only.
    #[serde(default)]
    pub color: Option<String>,
    /// Whether the track is muted.
    pub mute: bool,
    /// Whether the track is soloed.
    pub solo: bool,
    /// Audio clips placed on this track's timeline. Empty for non-audio tracks.
    #[serde(default)]
    pub clips: Vec<AudioClip>,
    /// MIDI clips on this track (for MIDI/instrument tracks). Processes through
    /// the track's built-in instrument (spec §11).
    #[serde(default)]
    pub midi_clips: Vec<crate::midi::MidiClip>,
    /// Ordered insert-effect chain (spec §5.6). Processes in list order.
    #[serde(default)]
    pub effects: EffectChain,
}

impl Track {
    /// Creates a new track of the given kind with default mix state.
    #[must_use]
    pub fn new(kind: TrackKind, name: impl Into<String>) -> Self {
        Self {
            id: TrackId::new(),
            kind,
            name: name.into(),
            volume: Gain::UNITY,
            pan: 0.0,
            color: None,
            mute: false,
            solo: false,
            clips: Vec::new(),
            midi_clips: Vec::new(),
            effects: EffectChain::new(),
        }
    }

    /// Creates the single master track.
    #[must_use]
    pub fn master() -> Self {
        Self::new(TrackKind::Master, "Master")
    }

    /// Adds a clip to the track and returns its id.
    pub fn add_clip(&mut self, clip: AudioClip) -> crate::id::ClipId {
        let id = clip.id;
        self.clips.push(clip);
        id
    }

    /// Adds a MIDI clip to the track and returns its id.
    pub fn add_midi_clip(&mut self, clip: crate::midi::MidiClip) -> crate::id::ClipId {
        let id = clip.id;
        self.midi_clips.push(clip);
        id
    }

    /// Returns the index of a clip by id, if present.
    #[must_use]
    pub fn clip_index(&self, id: crate::id::ClipId) -> Option<usize> {
        self.clips.iter().position(|c| c.id == id)
    }

    /// Looks up a clip mutably by id.
    #[must_use]
    pub fn clip_mut(&mut self, id: crate::id::ClipId) -> Option<&mut AudioClip> {
        self.clips.iter_mut().find(|c| c.id == id)
    }

    /// Removes a clip by id, returning `(index, clip)` if it existed.
    pub fn remove_clip(&mut self, id: crate::id::ClipId) -> Option<(usize, AudioClip)> {
        let idx = self.clip_index(id)?;
        Some((idx, self.clips.remove(idx)))
    }

    /// Inserts a clip at a specific index (clamped to the current length).
    pub fn insert_clip(&mut self, index: usize, clip: AudioClip) {
        let index = index.min(self.clips.len());
        self.clips.insert(index, clip);
    }
}
