//! `daw-dsp`: buffer abstraction, the processor contract, and built-in DSP.
//!
//! Processors operate on [`AudioBuffer`]s and know nothing about tracks,
//! projects, files, or UI (spec §10). The processing path is designed to be
//! allocation- and lock-free after [`AudioProcessor::prepare`].

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod buffer;
pub mod compressor;
pub mod delay;
pub mod distortion;
pub mod eq;
pub mod filter;
pub mod gain;
pub mod mixer;
pub mod pan;
pub mod processor;
pub mod reverb;
pub mod synth;

pub use buffer::AudioBuffer;
pub use compressor::CompressorProcessor;
pub use delay::DelayProcessor;
pub use distortion::DistortionProcessor;
pub use eq::{EqBand, EqBandType};
pub use filter::{FilterMode, FilterProcessor};
pub use gain::GainProcessor;
pub use mixer::mix_into;
pub use pan::PanProcessor;
pub use processor::{AudioProcessor, ProcessConfig};
pub use reverb::ReverbProcessor;
pub use synth::{midi_to_hz, Adsr, Synth, Waveform};
