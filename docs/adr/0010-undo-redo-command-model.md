# ADR-0010: Undo/redo and command model

**Status:** Accepted (initial — core arrangement commands; more added as
features arrive).

## Context

Most user-visible edits must be undoable (spec §13): track create/delete/rename,
clip move/trim/split/duplicate/delete, parameter changes, automation edits,
routing, MIDI edits, project settings. The model must also group high-frequency
gestures (a fader drag, an automation draw) into a single undoable action, and
must never leave the project invalid. It must be UI-independent.

## Options

1. **Snapshot the whole project per edit.** Simple, but memory/time scales with
   project size, not edit size; poor for large projects and frequent edits.
2. **Command objects that produce their own inverse.** Each command applies a
   change and returns a command that reverses it. Undo memory is proportional to
   the edit. This is the classic command pattern and matches the spec wording
   ("provide enough information to reverse it").
3. **Event-sourcing / persistent data structures.** Powerful but a much larger
   commitment than needed now.

## Decision

Adopt option 2 in a new `daw-commands` crate:

- `Command` trait: `label()` + `apply(&mut Project) -> Result<Box<dyn Command>>`
  returning the inverse. On error, the command must not mutate the project.
- `History`: executes commands, **validates the whole project after apply** and
  rolls back via the returned inverse if invalid, records inverses on an undo
  stack, and clears the redo stack on a new edit (linear history). Undo pushes
  redo commands and vice versa.
- `History::transaction(label, body)`: groups several commands into one undoable
  unit (gesture grouping, spec §13). Validation runs once at commit;
  intermediate states may be transiently invalid. On failure the whole
  transaction rolls back.

Initial commands: add/remove/insert/rename track, set track volume/mute,
add/remove/insert/move clip, set clip geometry (covers trim/split results).

## Consequences

- Commands own their inverse logic, keeping undo memory small.
- The project is never persisted in an invalid state: `History` validates and
  rolls back.
- Parameter gestures (fader/automation) will use `transaction` so they are one
  undo step, not thousands (to be wired when those UIs exist).
- Recording will need special handling (spec §13): undoing a recorded clip must
  not destroy the only copy of its media. That is deferred to the recording
  milestone and will be its own command with media-safe inversion.
- Whether a command is safe during playback and how engine updates are
  scheduled (spec §13) ties into ADR-0007 and is addressed when the real-time
  engine exists.

## Revisit when

Adding automation/parameter gestures, routing edits, MIDI edits, or recording
(media-safe undo).
