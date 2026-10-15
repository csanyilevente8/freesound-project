# Free Sound (working name)

A free and open-source, cross-platform desktop digital audio workstation (DAW)
written in Rust. The name is a placeholder and is not hard-coded into the
architecture.

> **Status: Milestone 1 (in progress) — offline core complete; live playback,
> recording, and an editable GUI working.** Milestone 0 and most of Milestone 1
> are done. The **egui GUI** (`daw-gui`, `gui` feature) shows the timeline,
> edits clips (move/trim/split/delete), mixes tracks (volume/mute/solo/effects),
> plays live (CPAL), and **records from the microphone** into new clips. Still
> missing: MIDI + a software instrument, and simultaneous play+record. See
> `docs/MILESTONE-1.md` and `docs/MILESTONE-0.md` for status reports and
> `docs/architecture/architecture.md` for the design + diagrams.

## Clone

```bash
git clone git@github.com:csanyilevente8/freesound-project.git
cd freesound-project
```

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
- **Live audio output** via CPAL (`daw play`), with a real-time-safe player
  (lock-free command/event rings, no allocation in the callback).
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

# Build a session and play it live on the default output device
cargo run -p daw-app --bin daw -- new ./song.json
cargo run -p daw-app --bin daw -- --export ./tone.wav
cargo run -p daw-app --bin daw -- import ./song.json ./tone.wav
cargo run -p daw-app --bin daw -- add-track ./song.json Gtr audio
cargo run -p daw-app --bin daw -- add-clip ./song.json Gtr tone.wav
cargo run -p daw-app --bin daw -- play ./song.json

# Full CLI reference
cargo run -p daw-app --bin daw -- help

# Launch the egui GUI prototype (opt-in; shows timeline + transport, plays live)
cargo run -p daw-app --features gui --bin daw-gui -- ./song.json
```

### Platform notes

- **Windows:** use PowerShell or `cmd`. Replace the `curl | sh` rustup line with
  the Windows installer from rustup.rs. All `cargo` commands are identical.
  Audio output uses WASAPI via CPAL (no extra packages).
- **macOS:** no extra system packages are required; audio output uses CoreAudio.
- **Linux:** install ALSA development headers for audio output
  (`libasound2-dev` on Debian/Ubuntu); JACK is optional. The offline
  render/export path needs no audio packages.

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
  daw-audio-io/ # real-time playback: CPAL output backend + lock-free rings
  daw-app/      # application shell + `daw` binary (GUI framework TBD)
docs/
  architecture/ # overview, audio-engine, project-format, ui
  adr/          # architecture decision records
  testing/      # testing strategy
```

## License

Proposed: **GPL-3.0-or-later** (see `docs/adr/0001-project-license.md`). A full
`LICENSE` file will be added before the first public release.
