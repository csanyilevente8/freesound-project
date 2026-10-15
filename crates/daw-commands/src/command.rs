//! The command abstraction (see spec §13).
//!
//! Every user-visible editing operation is a [`Command`]. A command:
//!
//! * carries a short [`label`](Command::label) for UI/history display, and
//! * [`apply`](Command::apply)s itself to a [`Project`], returning a boxed
//!   *inverse* command that exactly undoes the change.
//!
//! Returning the inverse (rather than snapshotting the whole project) keeps
//! undo memory proportional to the edit, and makes each command responsible for
//! knowing how to reverse itself. Redo is achieved by re-applying the original
//! command (the history stores both directions — see [`crate::History`]).
//!
//! Commands validate their precondition and apply the change; the caller
//! ([`History`]) is responsible for whole-project validation after apply, so a
//! command can never leave the project in an invalid persisted state.
//!
//! [`History`]: crate::History
//! [`Project`]: daw_core::project::Project

use daw_core::project::Project;

use crate::error::CommandError;

/// A reversible editing operation over a [`Project`].
pub trait Command: Send {
    /// A short, human-readable label (e.g. "Add Track", "Move Clip").
    fn label(&self) -> String;

    /// Applies the command to `project`, returning a command that undoes it.
    ///
    /// Implementations must either fully apply and return a correct inverse, or
    /// return an error **without** mutating the project.
    ///
    /// # Errors
    /// Returns [`CommandError`] if a precondition fails. On error the project
    /// must be left unchanged.
    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError>;
}
