//! `daw-app`: application composition and the minimal UI shell.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod app;
pub mod cli;

pub use app::{AppState, SineSource};
