//! `daw-app`: application composition and the minimal UI shell.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod app;
pub mod cli;
pub mod playback;
pub mod record;

#[cfg(feature = "gui")]
pub mod gui;

pub use app::{AppState, SineSource};
