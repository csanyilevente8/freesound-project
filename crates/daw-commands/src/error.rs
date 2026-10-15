//! Command errors.

use thiserror::Error;

use daw_core::validate::ValidationError;

/// Errors a command can produce when applied or inverted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandError {
    /// A referenced entity (track, clip, asset) was not found.
    #[error("entity not found: {0}")]
    NotFound(String),

    /// The command's precondition was not met.
    #[error("invalid command: {0}")]
    Invalid(String),

    /// Applying the command would leave the project invalid.
    #[error(transparent)]
    Validation(#[from] ValidationError),
}
