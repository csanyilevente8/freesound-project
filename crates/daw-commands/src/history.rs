//! Undo/redo history (see spec §13).
//!
//! [`History`] executes [`Command`]s against a [`Project`] and records their
//! inverses so they can be undone and redone. Key behaviors:
//!
//! * **Validation after apply.** After a command applies, the whole project is
//!   validated. If invalid, the change is rolled back via the inverse the
//!   command returned, and an error is returned — the project is never left in
//!   an invalid state.
//! * **Redo invalidation.** Executing a new command clears the redo stack
//!   (standard linear-history semantics).
//! * **Gesture grouping.** [`History::transaction`] groups several commands
//!   into a single undoable unit, so a fader drag or an automation-draw gesture
//!   is one undo step, not thousands (spec §13).
//!
//! The history stores inverse commands, so undo memory is proportional to the
//! edits made, not to project size.

use daw_core::project::Project;
use daw_core::validate::Validate;

use crate::command::Command;
use crate::error::CommandError;

/// One undoable unit: an ordered list of (inverse) commands plus a label.
///
/// A simple edit has a single entry; a grouped gesture (via
/// [`History::transaction`]) has several. To undo, inverses are applied in
/// reverse order; the undo itself yields redo commands, and vice versa.
struct Entry {
    label: String,
    /// Commands that, applied in order, perform this unit (for redo) or undo it
    /// (for undo), depending on which stack the entry lives on.
    commands: Vec<Box<dyn Command>>,
}

/// Undo/redo history over a project.
#[derive(Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

impl History {
    /// Creates an empty history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of undoable units.
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Number of redoable units.
    #[must_use]
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Returns the label of the next undoable unit, if any.
    #[must_use]
    pub fn peek_undo(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    /// Returns the label of the next redoable unit, if any.
    #[must_use]
    pub fn peek_redo(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    /// Executes a single command, recording it for undo and clearing redo.
    ///
    /// # Errors
    /// Returns [`CommandError`] if the command fails its precondition or if the
    /// resulting project is invalid (in which case the change is rolled back).
    pub fn execute(
        &mut self,
        project: &mut Project,
        command: Box<dyn Command>,
    ) -> Result<(), CommandError> {
        let label = command.label();
        let inverse = apply_validated(project, command.as_ref())?;
        self.undo.push(Entry {
            label,
            commands: vec![inverse],
        });
        self.redo.clear();
        Ok(())
    }

    /// Groups multiple commands executed inside `body` into a single undoable
    /// unit labeled `label`.
    ///
    /// If any command in the body fails, all commands already applied in the
    /// transaction are rolled back and the error is returned (the redo stack is
    /// untouched and no partial unit is recorded).
    ///
    /// # Errors
    /// Returns the first [`CommandError`] encountered; the project is restored
    /// to its pre-transaction state on failure.
    pub fn transaction<F>(
        &mut self,
        project: &mut Project,
        label: impl Into<String>,
        body: F,
    ) -> Result<(), CommandError>
    where
        F: FnOnce(&mut Project, &mut Transaction) -> Result<(), CommandError>,
    {
        let mut tx = Transaction {
            inverses: Vec::new(),
        };

        // Run the body, then validate the whole project once at commit.
        let result = body(project, &mut tx).and_then(|()| project.validate().map_err(Into::into));

        match result {
            Ok(()) => {
                if tx.inverses.is_empty() {
                    return Ok(()); // nothing happened; don't record an empty unit
                }
                // Inverses were collected in application order; to undo we apply
                // them in reverse, so store them reversed.
                tx.inverses.reverse();
                self.undo.push(Entry {
                    label: label.into(),
                    commands: tx.inverses,
                });
                self.redo.clear();
                Ok(())
            }
            Err(e) => {
                // Roll back: apply collected inverses in reverse order.
                for inv in tx.inverses.iter().rev() {
                    let _ = inv.apply(project);
                }
                Err(e)
            }
        }
    }

    /// Undoes the most recent unit, returning its label.
    ///
    /// # Errors
    /// Returns [`CommandError`] if there is nothing to undo or if an inverse
    /// fails to apply.
    pub fn undo(&mut self, project: &mut Project) -> Result<String, CommandError> {
        let entry = self
            .undo
            .pop()
            .ok_or_else(|| CommandError::Invalid("nothing to undo".into()))?;
        let label = entry.label.clone();
        let mut redo_commands = Vec::with_capacity(entry.commands.len());
        for cmd in &entry.commands {
            let redo = apply_validated(project, cmd.as_ref())?;
            redo_commands.push(redo);
        }
        // Undo applied inverses in stored order; redo must reverse them again.
        redo_commands.reverse();
        self.redo.push(Entry {
            label: label.clone(),
            commands: redo_commands,
        });
        Ok(label)
    }

    /// Redoes the most recently undone unit, returning its label.
    ///
    /// # Errors
    /// Returns [`CommandError`] if there is nothing to redo or if a command
    /// fails to apply.
    pub fn redo(&mut self, project: &mut Project) -> Result<String, CommandError> {
        let entry = self
            .redo
            .pop()
            .ok_or_else(|| CommandError::Invalid("nothing to redo".into()))?;
        let label = entry.label.clone();
        let mut undo_commands = Vec::with_capacity(entry.commands.len());
        for cmd in &entry.commands {
            let undo = apply_validated(project, cmd.as_ref())?;
            undo_commands.push(undo);
        }
        undo_commands.reverse();
        self.undo.push(Entry {
            label: label.clone(),
            commands: undo_commands,
        });
        Ok(label)
    }
}

/// Collects inverse commands while a [`History::transaction`] body runs.
pub struct Transaction {
    inverses: Vec<Box<dyn Command>>,
}

impl Transaction {
    /// Applies a command as part of the transaction. The project is mutated
    /// immediately; validation is deferred to the end of the transaction.
    ///
    /// # Errors
    /// Returns [`CommandError`] if the command fails its precondition.
    pub fn execute(
        &mut self,
        project: &mut Project,
        command: Box<dyn Command>,
    ) -> Result<(), CommandError> {
        // Within a transaction we do not validate per-command (intermediate
        // states may be transiently invalid); validation runs once at commit.
        let inverse = command.apply(project)?;
        self.inverses.push(inverse);
        Ok(())
    }
}

/// Applies a command and validates the whole project, rolling back on failure.
fn apply_validated(
    project: &mut Project,
    command: &dyn Command,
) -> Result<Box<dyn Command>, CommandError> {
    let inverse = command.apply(project)?;
    if let Err(e) = project.validate() {
        // Roll back using the inverse the command just produced.
        let _ = inverse.apply(project);
        return Err(CommandError::from(e));
    }
    Ok(inverse)
}
