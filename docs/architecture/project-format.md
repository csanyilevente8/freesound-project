# Project Format

Status: **Milestone 0 — single versioned JSON file.** The full directory bundle
is deferred (ADR-0005).

## On-disk shape (M0)

A single JSON file (e.g. `song.json`) containing a `ProjectDocument`:

```json
{
  "format": "daw-project",
  "schema_version": 1,
  "project": {
    "id": "…uuid…",
    "name": "Untitled",
    "sample_rate": 48000,
    "tempo": 120.0,
    "time_signature": { "numerator": 4, "denominator": 4 },
    "tracks": [
      { "id": "…uuid…", "kind": "master", "name": "Master",
        "volume": 1.0, "mute": false, "solo": false }
    ]
  }
}
```

- `format` is a stable, product-name-independent magic string. Loading rejects
  any other value (`ProjectError::BadMagic`).
- `schema_version` is an integer. A version newer than this build supports is
  rejected (`ProjectError::UnsupportedVersion`). Older versions are routed
  through `migrate::migrate_to_current`. **Current version: 4.**
  - **v1** (M0): project, tracks (no clips), no media assets.
  - **v2** (M1): adds `project.assets` (media references) and `track.clips`
    (audio clips).
  - **v3** (M1): adds `track.effects` (insert-effect chains).
  - **v4** (M1): adds `project.automation` (automation lanes). All new fields
    use `#[serde(default)]`, so an older file still deserializes (empty
    collections) and the step-wise migration (v1→v2→v3→v4) stamps the version.
    Hand-written v1-document load is covered by a test.
- The in-memory `daw_core::Project` is a **separate** type from this on-disk
  document, so internal structs are never serialized directly.

## Save semantics

`daw_project::save`:

1. Validates the project (`Project::validate`). Invalid projects are never
   written.
2. Serializes to pretty JSON.
3. Writes to a temp file `.<name>.tmp` in the same directory, flushes, fsyncs.
4. Atomically renames over the target.

An interrupted save therefore cannot corrupt the previous good file.

## Load semantics

`daw_project::load`:

1. Reads the file.
2. Parses the document (malformed JSON → `ProjectError::Serde`).
3. Checks magic and version; migrates if needed.
4. Validates domain invariants (exactly one master track, unique track ids,
   non-empty name).

## Deferred (spec §6)

- Directory bundle `MySong.dawproject/` with `media/`, `peaks/`, `autosave/`,
  `recovery/`, `presets/`. Becomes additive: this document becomes the bundle's
  `project.json`.
- Media **import/decoding** (the asset *references* exist as of v2, but the
  samples are read through `daw_engine::AssetSource`; a file decoder is a future
  ADR). Collect/copy-media and content-identity dedupe.
- Autosave at intervals; crash recovery points.
- UI preferences stored separately from musical content.

## Versioning policy

Any change to the serialized shape bumps `CURRENT_SCHEMA_VERSION` and adds a
step in `migrate_to_current`. Round-trip and forward-version-rejection are
covered by tests in `daw-project`.
