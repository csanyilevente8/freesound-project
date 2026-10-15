//! Save/load errors.

use thiserror::Error;

use daw_core::validate::ValidationError;

use crate::document::FORMAT_MAGIC;

/// Errors produced while saving or loading a project.
#[derive(Debug, Error)]
pub enum ProjectError {
    /// Underlying filesystem error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization/deserialization failure.
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    /// The file is not a recognized project document.
    #[error("not a {FORMAT_MAGIC} document (found format {found:?})")]
    BadMagic {
        /// The `format` value that was found.
        found: String,
    },

    /// The schema version is newer than this build can read.
    #[error("unsupported schema version {found}; this build supports up to {supported}")]
    UnsupportedVersion {
        /// Version found in the file.
        found: u32,
        /// Highest version this build understands.
        supported: u32,
    },

    /// The loaded project violated a domain invariant.
    #[error(transparent)]
    Validation(#[from] ValidationError),
}
