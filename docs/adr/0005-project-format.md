# ADR-0005: Project directory and serialization format

**Status:** Accepted (initial form; directory bundle deferred).

## Context

Projects must be portable, documented, versioned, migratable, and recoverable
(spec §6). The format must not serialize "arbitrary internal Rust structs" and
must carry an explicit version so old files remain loadable.

## Decision

- **Serialization:** JSON via `serde` / `serde_json`, pretty-printed.
- **Versioned document wrapper:** a dedicated `ProjectDocument` type with a
  stable `format` magic string (`"daw-project"`, product-name-independent) and
  an integer `schema_version`, written before the body. Readers validate magic
  and version before parsing further.
- **Decoupled schema:** the on-disk document is a separate type from the
  in-memory `daw_core::Project`, so the two can evolve independently and a
  migration layer (`migrate::migrate_to_current`) can upgrade older versions.
- **Atomic saves:** write to a temp file in the target directory, `fsync`, then
  `rename` over the target, so an interrupted save cannot corrupt the last good
  file.
- **Validation on load:** `Project::validate()` runs after deserialization;
  invalid documents are rejected rather than loaded.

## Deferred (not needed for M0)

The full directory bundle from spec §6.1 (`media/`, `peaks/`, `autosave/`,
`recovery/`) is deferred to when there is media to store. Milestone 0 uses a
single `project.json` file. The move to a `.dawproject/` bundle is additive: the
same `ProjectDocument` becomes the bundle's `project.json`.

## Rationale

JSON is human-readable, debuggable, diffable, and well supported — appropriate
while the schema is still moving. A binary format can be reconsidered later via
a new ADR if size/performance demands it; the versioned wrapper makes that a
contained change.

## Consequences

- Large projects may be verbose; acceptable at this stage.
- Every schema change bumps `CURRENT_SCHEMA_VERSION` and adds a migration step.
- Forward-compat: files from a newer build are rejected with a clear error.

## Revisit when

Introducing media references (bundle layout), or if JSON size/perf becomes a
problem (binary format).
