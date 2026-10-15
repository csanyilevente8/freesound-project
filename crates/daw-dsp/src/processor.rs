//! The audio processor contract (see spec §10.1 and ADR-0008).
//!
//! Milestone 0 uses an **in-place** processing model: a processor reads and
//! writes the same [`AudioBuffer`]. This keeps the first processors simple and
//! allocation-free. The trait is deliberately minimal; MIDI/event delivery,
//! latency reporting, and bypass are documented as future extensions in the
//! ADR and will be added when a feature needs them, rather than speculatively.

use crate::buffer::AudioBuffer;
use daw_core::time::SampleRate;

/// Configuration passed to a processor before playback/rendering begins.
///
/// `prepare` is the only place a processor may allocate or do expensive setup.
/// After `prepare`, `process` must be allocation- and lock-free.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessConfig {
    /// Sample rate the processor will run at.
    pub sample_rate: SampleRate,
    /// Maximum number of frames a single `process` call may contain. The
    /// engine may call `process` with fewer frames, but never more.
    pub max_block_frames: usize,
    /// Number of channels the processor will operate on.
    pub channels: usize,
}

/// A real-time audio processor.
///
/// Implementors must guarantee that [`process`](AudioProcessor::process) does
/// not allocate, lock, perform I/O, or block (spec §9.2). `Send` is required so
/// a prepared processor can be handed to the audio thread.
pub trait AudioProcessor: Send {
    /// Prepares internal state for the given configuration. May allocate.
    fn prepare(&mut self, config: &ProcessConfig);

    /// Processes one block in place. Must be real-time safe.
    fn process(&mut self, buffer: &mut AudioBuffer);

    /// Resets transient state (e.g. filter history) without reallocating.
    fn reset(&mut self);
}
