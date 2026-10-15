//! `daw-render`: offline rendering and export.
//!
//! Reuses the hardware-independent engine render path to produce output files
//! (WAV in Milestone 1). No audio device is required.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod export;
pub mod wav;

pub use export::{export_wav, render_project, ExportError, ExportOptions};
pub use wav::{write_wav, WavFormat};
