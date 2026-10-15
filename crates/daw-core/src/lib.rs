//! `daw-core`: the stable domain model for the DAW.
//!
//! This crate defines identifiers, time representation, audio value types, the
//! track and project model, and validation. It is deliberately free of GUI,
//! audio-device, and serialization-framework concerns (serde derives on plain
//! data are the only exception, and no format is committed here). See the
//! architecture docs under `docs/architecture/` and the ADRs under `docs/adr/`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod audio;
pub mod automation;
pub mod clip;
pub mod effect;
pub mod id;
pub mod media;
pub mod midi;
pub mod project;
pub mod time;
pub mod track;
pub mod validate;

pub use audio::{ChannelLayout, Gain, Sample};
pub use automation::{AutomationLane, AutomationPoint, AutomationTarget, Interpolation};
pub use clip::AudioClip;
pub use effect::{EffectChain, EffectInstance, ParamValue};
pub use id::{AssetId, ClipId, EffectId, ParameterId, ProjectId, TrackId};
pub use media::MediaAssetRef;
pub use midi::{MidiClip, MidiNote};
pub use project::Project;
pub use time::{MusicalTime, SampleRate, SampleTime, Tempo, TimeSignature, PPQ};
pub use track::{Track, TrackKind};
pub use validate::{Validate, ValidationError};
