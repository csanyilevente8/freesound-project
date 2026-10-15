//! Validation of domain invariants.
//!
//! Validation is kept separate from serialization: `daw-project` validates
//! after deserialization, but the rules themselves live with the domain model.

use thiserror::Error;

/// An error describing a violated domain invariant.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("validation error: {message}")]
pub struct ValidationError {
    /// Human-readable description of the violated invariant.
    pub message: String,
}

impl ValidationError {
    /// Creates a validation error with the given message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Types that can validate their internal invariants.
pub trait Validate {
    /// Returns `Ok(())` if all invariants hold, otherwise the first violation.
    ///
    /// # Errors
    /// Returns a [`ValidationError`] describing the first invariant violation.
    fn validate(&self) -> Result<(), ValidationError>;
}
