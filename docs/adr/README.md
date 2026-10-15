# Architecture Decision Records

This directory holds Architecture Decision Records (ADRs) for the project.

Each ADR captures a single significant decision: the context, the options
considered, the decision taken, and its consequences. ADRs are immutable once
accepted; to change a decision, add a new ADR that supersedes the old one.

## Status values

- **Proposed** — recommended but awaiting confirmation; reversible.
- **Accepted** — decided and in effect.
- **Accepted pending prototype** — the choice is made and recorded, but final
  confirmation depends on a time-boxed prototype passing stated gates.
- **Open / Deferred** — explicitly not yet decided; the ADR records why and
  when the decision is expected.
- **Superseded by ADR-XXXX** — replaced by a later decision.

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-project-license.md) | Project license | Proposed (GPL-3.0-or-later) |
| [0002](0002-gui-framework.md) | GUI framework | Accepted (egui/eframe; prototype passed) |
| [0003](0003-audio-io-library.md) | Audio I/O library | Accepted (CPAL; headless spike done) |
| [0004](0004-internal-sample-format.md) | Internal sample format & channel model | Accepted |
| [0005](0005-project-format.md) | Project directory & serialization format | Accepted |
| [0006](0006-time-representation.md) | Time representation | Accepted |
| [0007](0007-realtime-communication.md) | Real-time command/parameter communication | Accepted (initial) |
| [0008](0008-processor-api.md) | Processor API & graph update strategy | Accepted (initial) |
| [0009](0009-audio-export-writer.md) | Audio export writer (WAV) | Accepted (initial) |
| [0010](0010-undo-redo-command-model.md) | Undo/redo and command model | Accepted (initial) |
| [0011](0011-audio-import-decoding.md) | Audio import and decoding | Accepted (initial) |
