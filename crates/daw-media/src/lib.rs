//! `daw-media`: audio import/decoding behind a replaceable boundary.
//!
//! Milestone 1 decodes canonical PCM16/float32 WAV with a hand-written reader
//! and exposes decoded audio to the engine via `MemoryAssetSource`
//! (implementing `daw_engine::schedule::AssetSource`). Compressed formats
//! (`symphonia`), streaming decode, and resampling are deferred (ADR-0011).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod import;
pub mod peaks;
pub mod resample;
pub mod wav_reader;

pub use import::{import_wav, import_wav_at, ImportError, Imported, MemoryAssetSource};
pub use peaks::{compute_peaks, Peak, WaveformPeaks};
pub use resample::{resample_channel, resample_decoded};
pub use wav_reader::{decode_wav, DecodeError, DecodedAudio};
