//! `daw-engine`: the hardware-independent processing path.
//!
//! Milestone 0 provides only the offline renderer ([`render::render_block`]).
//! The real-time transport, graph scheduling, and device adapter are deferred
//! to later milestones but will reuse the same DSP primitives from `daw-dsp`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod automation;
pub mod effects;
pub mod render;
pub mod schedule;

pub use automation::automated_project;
pub use effects::{build_processor, process_chain, ProcessorChain};
pub use render::{render_block, RenderError};
pub use schedule::{project_length_frames, schedule_track_block, AssetSource};
