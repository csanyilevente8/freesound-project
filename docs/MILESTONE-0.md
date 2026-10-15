# Milestone 0 — Status Report

**Goal:** architecture skeleton and an offline vertical slice. Confirm the
project model, serialization, and offline audio processing work end to end,
without live audio, recording, an arrangement editor, or a committed GUI
toolkit.

---

## 1. Repository assessment (before this work)

- **No git repository** and **no Rust toolchain** installed on the machine.
- Only the specification (`free_sound_project_spec.md`) and two empty
  documentation directories (`docs/adr/`, `docs/architecture/`) existed. No
  `Cargo.toml`, no source, no tests, no `README`.
- Conclusion: effectively greenfield. Nothing to preserve beyond the spec and
  the directory conventions, which were followed.

Setup performed: installed the stable Rust toolchain via `rustup`
(`rustc/cargo/rustfmt/clippy` 1.99.0, user-local under `~/.cargo` and
`~/.rustup`) and ran `git init`.

## 2. Implemented functionality (working now)

- **Cargo workspace** with five crates and acyclic boundaries:
  `daw-core`, `daw-project`, `daw-dsp`, `daw-engine`, `daw-app`.
- **Core model** (`daw-core`): UUID-backed stable IDs (newtyped per entity),
  integer time model (`SampleTime`, `MusicalTime` @ 960 PPQ, `SampleRate`,
  `Tempo`, `TimeSignature`), audio value types (`Sample=f32`, `ChannelLayout`,
  `Gain`), `Track`/`Project` model, and a `Validate` trait with project
  invariants.
- **Versioned persistence** (`daw-project`): `ProjectDocument` wrapper with a
  stable `daw-project` magic + integer `schema_version`; JSON save/load; atomic
  write-temp-then-rename; validation on load; migration scaffolding with
  forward-version rejection.
- **DSP** (`daw-dsp`): planar `AudioBuffer`, the `AudioProcessor` trait +
  `ProcessConfig`, a `GainProcessor` with linear parameter smoothing, and a
  `mix_into` summing mixer.
- **Offline engine** (`daw-engine`): `render_block` renders a project's tracks
  through track gain → summing → master gain, honoring mute/solo, with no audio
  hardware.
- **Shell** (`daw-app`): framework-agnostic `AppState` and a `daw` binary that
  creates/displays an empty project and can save+reload to verify the
  round-trip.

## 3. Architecture decisions

ADRs created in `docs/adr/`:

| ADR | Decision | Status |
|-----|----------|--------|
| 0001 | License = GPL-3.0-or-later | **Proposed** (confirmed for M0; final at release) |
| 0002 | GUI framework | **Open** — deferred to M1 |
| 0003 | Audio I/O library | **Open** — deferred to M1 |
| 0004 | `f32` planar buffers; explicit `ChannelLayout`; no auto-clip in mixer | **Accepted** |
| 0005 | Versioned JSON document; atomic saves; directory bundle deferred | **Accepted** |
| 0006 | Integer `SampleTime` + `MusicalTime` (960 PPQ); no float-seconds authority | **Accepted** |
| 0007 | Bounded lock-free queues + off-thread graph swap | **Proposed** (no RT thread yet) |
| 0008 | In-place `AudioProcessor` trait; extend when features need it | **Accepted (initial)** |

Decisions still open (intentionally): GUI framework (0002), audio I/O (0003),
and — not yet written because no feature needs them — media decoding/resampling,
MIDI I/O, and installer/packaging (spec §19 items 9–11).

## 4. Files changed (important additions)

```
Cargo.toml                         workspace + pinned deps
.gitignore
README.md                          build/test instructions (all platforms)
crates/daw-core/                   id, time, audio, track, project, validate (+ lib)
crates/daw-project/                document, error, migrate, io (+ lib)
crates/daw-dsp/                    buffer, processor, gain, mixer (+ lib)
crates/daw-engine/                 render (+ lib) + tests/vertical_slice.rs
crates/daw-app/                    app (AppState), main (daw binary), lib
docs/architecture/                 overview, audio-engine, project-format, ui
docs/adr/                          README + ADR-0001..0008
docs/testing/strategy.md
docs/MILESTONE-0.md                this report
```

## 5. Verification results (exact commands)

Run with `source "$HOME/.cargo/env"` first.

| Command | Result |
|---------|--------|
| `cargo build --workspace` | **OK** — all five crates compile |
| `cargo fmt --all -- --check` | **OK** — no diffs |
| `cargo clippy --workspace --all-targets -- -D warnings` | **OK** — no warnings |
| `cargo test --workspace` | **OK** — **58 passed, 0 failed** |
| `cargo run -p daw-app --bin daw -- --save <path>` | **OK** — prints project, saves, reloads, verifies round-trip |

Test distribution: daw-core 21, daw-dsp 15, daw-engine 8 + integration 2,
daw-project 10, daw-app 2.

## 6. Acceptance criteria (spec §15 P0 + §20)

| Criterion | Status |
|-----------|--------|
| Rust workspace, clear module boundaries, reproducible build | **Passed** |
| Core project model with stable IDs | **Passed** |
| Versioned save/load with validation | **Passed** |
| Offline audio buffer processing (no hardware) | **Passed** |
| Gain processor | **Passed** |
| Simple summing/mixing | **Passed** |
| Minimal UI shell that opens/displays an empty project | **Passed** (text shell; GUI deferred) |
| Tests: round-trip, invalid data, gain, mixing | **Passed** |
| Documentation + ADRs + dependency/license inventory | **Passed** |
| Builds on Windows/macOS/Linux | **Verified on macOS**; **Not verified** on Windows/Linux (no CI yet — same toolchain-only commands, pure-Rust deps, so expected to build) |
| Cross-platform CI | **Not implemented** (planned early M1) |

## 7. Known limitations (not implemented in M0)

- No live audio I/O, no playback, no recording, no MIDI, no transport/clock.
- No clip/media model, import, decoding, resampling, or waveform peaks.
- No arrangement editor, mixer UI, piano roll, effect editors, or browser.
- No GUI toolkit chosen; the shell is text-only behind `AppState`.
- Only one processor (gain); distortion/filter/EQ/delay/reverb/compressor and
  the instrument are deferred.
- No automation, undo/redo, routing graph, or buses beyond the model fields.
- Project format is a single JSON file, not the directory bundle; no
  autosave/recovery yet.
- No real-time thread exists; **no real-time-safety claim is made.**
- CI across platforms is not set up; cross-platform build is verified only on
  macOS.

## 8. Dependency & licensing summary

All pinned; all MIT/Apache-2.0 (compatible with proposed GPL-3.0-or-later):
`serde` 1.0.210, `serde_json` 1.0.128, `uuid` 1.10.0, `thiserror` 1.0.64, and
`tempfile` 3.13.0 (dev-only). No networking/telemetry/cloud. Full inventory in
`docs/architecture/overview.md`.

## 9. Next milestone (proposed, not yet started)

**Milestone 1 — Playback & arrangement (spec P1), smallest testable first slice.**

Suggested first increment, kept independently testable:

1. **ADR-0003 decision** (audio I/O; evaluate CPAL) and **ADR-0002** (GUI;
   evaluate egui) — decide before coding against them.
2. **Audio clip + media model** in `daw-core` (asset reference, timeline start,
   source offset, duration) with validation and serialization (schema v2 +
   migration v1→v2 to exercise the migration path).
3. **WAV offline export** in a new `daw-render` boundary reusing `render_block`
   — testable without any device (inspect frame count/levels).
4. Only then: a `daw-audio-io` device adapter behind a trait, feeding the same
   render path for real-time playback.

Acceptance target for M1: import an audio file, place it on a timeline, play it,
adjust level, save/reopen, and export a stereo WAV — with deterministic tests
for the clip math, export, and (fake-device) playback path.

This milestone is **not** implemented yet, per the workflow.
