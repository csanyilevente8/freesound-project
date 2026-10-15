# Free Sound (working name)

A free and open-source, cross-platform desktop digital audio workstation (DAW)
written in Rust. The name is a placeholder and is not hard-coded into the
architecture.

> **Status: Milestone 1 (in progress) — offline clip model, project schema v2,
> and WAV export.** Milestone 0 (architecture skeleton + offline vertical slice)
> is complete. There is still no live audio, recording, arrangement editor, or
> GUI toolkit. See `docs/MILESTONE-1.md` and `docs/MILESTONE-0.md` for status
> reports and `docs/architecture/overview.md` for the design.

## What works today

- A Cargo workspace with eight crates and clear boundaries.
- A core project model with stable (UUID) identifiers and integer-based time.
- Audio **clip** and **media-asset** model with non-destructive trim/split and
  dangling-reference validation.
- A persistent **insert-effect chain** model (type id + params + bypass) that
  survives save/reload, with unknown effects kept as recoverable placeholders.
- A general **automation** model (lanes targeting track volume or effect
  params; step/linear interpolation) applied during offline render.
- Versioned, atomic project save/load (JSON, schema v4) with validation and
  real step-wise migrations (v1→v2→v3→v4).
- An offline audio-processing path (no audio device required) with clip
  scheduling, effect-chain processing, and per-block automation.
- DSP processors: **gain, pan, distortion, resonant filter, delay, reverb,
  parametric EQ, compressor**, and a summing mixer.
- **Offline WAV export** (PCM16 / float32), reachable via `daw --export`.
- **WAV import/decoding** with **cubic resampling** and **waveform-peak**
  generation, closing the offline media loop (export → re-import).
- **Missing-media detection + relink** helpers.
- An **undo/redo command model** with gesture grouping (transactions) and
  automatic validation + rollback, covering track, clip, effect, and automation
  edits.
- A minimal, framework-agnostic application shell (`daw` binary).
- 201 passing tests; clean `rustfmt` and `clippy -D warnings`.

## Requirements

- Rust (stable) with `cargo`, `rustfmt`, and `clippy`. Install via
  [rustup](https://rustup.rs):

  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

## Build and test

The same commands work on **Windows, macOS, and Linux**.

```bash
# Build everything
cargo build --workspace

# Run all tests
cargo test --workspace

# Formatting and lints
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# Run the minimal shell
cargo run -p daw-app --bin daw

# Create, save, reload, and verify a project round-trip
cargo run -p daw-app --bin daw -- --save ./song.json

# Build a demo tone project and export a stereo WAV (offline, no device)
cargo run -p daw-app --bin daw -- --export ./demo.wav
```

### Platform notes

- **Windows:** use PowerShell or `cmd`. Replace the `curl | sh` rustup line with
  the Windows installer from rustup.rs. All `cargo` commands are identical.
- **macOS:** no extra system packages are required for Milestone 0 (there is no
  audio backend yet).
- **Linux:** no extra system packages are required for Milestone 0. Audio
  backend development packages (ALSA/JACK) will be needed starting at
  Milestone 1.

## Repository layout

```
crates/
  daw-core/     # IDs, time, audio value types, project/track model, validation
  daw-project/  # versioned save/load, migration scaffolding
  daw-dsp/      # audio buffer, processor trait, gain, pan, distortion, filter, delay, reverb, EQ, compressor, mixer
  daw-engine/   # offline render + clip scheduling + effect chain + automation
  daw-render/   # offline WAV export (reuses the engine)
  daw-media/    # audio import/decoding (WAV), resampling, waveform peaks
  daw-commands/ # editing commands + undo/redo history
  daw-app/      # application shell + `daw` binary (GUI framework TBD)
docs/
  architecture/ # overview, audio-engine, project-format, ui
  adr/          # architecture decision records
  testing/      # testing strategy
```

## License

Proposed: **GPL-3.0-or-later** (see `docs/adr/0001-project-license.md`). A full
`LICENSE` file will be added before the first public release.
