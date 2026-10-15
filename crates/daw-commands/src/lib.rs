//! `daw-commands`: editing commands and undo/redo over the core project model.
//!
//! Every user-visible edit is a [`Command`] that validates, applies, and
//! inverts itself. [`History`] runs commands, validates the project after each,
//! and records inverses for undo/redo. [`History::transaction`] groups a
//! gesture into one undoable unit (spec §13). This crate is UI-independent.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod command;
pub mod commands;
pub mod error;
pub mod history;

pub use command::Command;
pub use error::CommandError;
pub use history::{History, Transaction};
