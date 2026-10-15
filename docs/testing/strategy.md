# Testing Strategy

This document describes the testing approach and the commands used to verify the
project. It reflects what is **implemented and run** in Milestone 0, plus the
plan for later milestones.

## Commands

Run from the workspace root (ensure the Rust toolchain is on `PATH`, e.g.
`source "$HOME/.cargo/env"`):

```bash
cargo fmt --all -- --check     # formatting is clean
cargo clippy --workspace --all-targets -- -D warnings   # lint, warnings = errors
cargo test --workspace         # all unit + integration tests
cargo build --workspace        # debug build
cargo run -p daw-app --bin daw -- --save /tmp/song.json  # manual smoke test
```

All of the above pass as of Milestone 0 (see the milestone report for exact
output).

## Test inventory (Milestone 0)

Deterministic, offline, no audio hardware or wall-clock dependence.

### `daw-core` (unit)
- ID uniqueness and UUID / serde round-trips.
- `SampleRate` rejects zero; seconds↔frames round-trip is frame-exact; negative
  seconds clamp to zero.
- `MusicalTime → SampleTime` at 120 bpm/48k and 60 bpm/44.1k (exact frames).
- `Tempo` / `TimeSignature` reject invalid values.
- `SampleTime` saturating add/sub.
- `Gain` rejects invalid; dB→linear sanity (0 dB ≈ 1.0, −6 dB ≈ 0.5).
- `ChannelLayout` channel counts.
- `Project` validation: single-master invariant, non-empty name, unique track
  ids, track lookup, **unique asset ids, clip references an existing asset
  (dangling-reference detection)**.
- **`MediaAssetRef` validation (non-empty path).**
- **`AudioClip`: timeline_end, overlap, trim_start (source stays aligned),
  trim_end, split_at (adjacent regions, no-op outside range), zero-duration
  invalid.**

### `daw-dsp` (unit)
- `AudioBuffer`: silent on creation, channel isolation, `add_from` overlap and
  mismatched-shape handling, absolute `peak`, `clear`.
- `GainProcessor`: unity = identity, 0.5 halves, silence mutes, no-smoothing
  jumps immediately, smoothing reaches target deterministically, reset clears.
- `mix_into`: sums sources, empty = silence, can exceed unity without clipping.
- `PanProcessor`: constant-power law (center ≈ −3 dB, hard L/R mute opposite
  side, L²+R²≈1), mono no-op, smoothing settles.
- `DistortionProcessor`: near-linear for small input, saturates toward output
  gain, odd-symmetric, bounded output, clamps negative params.
- `FilterProcessor` (SVF): lowpass attenuates highs, highpass attenuates lows,
  stable under extreme/sweeping settings (finite output), reset clears ringing,
  DC passes a lowpass.
- `DelayProcessor`: impulse reappears after the delay, feedback decays (not
  grows), feedback clamped < 1, dry mix passes input, reset clears the tail.
- `ReverbProcessor`: impulse yields a decaying tail, stable over long input at
  max room, dry mix passes input, reset clears the tail.
- `EqBand` (biquad): 0 dB peaking ≈ unity, peaking boost/cut raises/lowers
  center energy, low-shelf boost raises lows, finite + resettable.
- `CompressorProcessor`: below-threshold passes unchanged, above-threshold
  attenuates with reported gain reduction, makeup gain boosts, finite +
  resettable.

### `daw-engine` (unit + integration)
- `render_block`: empty project = silence, two tracks sum, track volume applied
  before mix, master volume applied last, mute silences, solo isolates, unknown
  track rejected, frame mismatch rejected.
- **`effects`: builds all known processors (incl. delay/reverb/eq/compressor),
  unknown = `None`, gain effect applies, bypassed effect skipped, unknown effect
  skipped (not fatal), chain order respected.**
- **`automation`: no-automation returns an equivalent project; track-volume
  automation is applied per block; effect-param automation overrides the stored
  value.**
- `schedule`: clip fills its timeline window, partial overlap at block boundary,
  source exhaustion goes silent, project length = max clip end.
- Integration (`tests/vertical_slice.rs`): render → save → reload → render is
  byte-identical; file round-trip then render yields the expected level.

### `daw-media` (unit + integration) — new in M1
- `wav_reader`: decodes PCM16 stereo, tolerates extra chunks before `data`,
  rejects non-WAVE / truncated / unsupported format.
- `import`: `MemoryAssetSource` reads the requested channel, respects start
  offset, mono feeds all channels from channel 0, unknown asset reads nothing.
- `resample`: equal rate is identity, doubling/halving changes length, constant
  signal preserved, ramp stays bounded/monotonic, `resample_decoded` updates
  rate + lengths.
- `peaks`: bucket count matches input incl. trailing partial, captures min/max,
  per-channel envelopes, zero bucket size clamped, empty input yields empty.
- Integration (`tests/round_trip.rs`): float32 export→import bit-exact; imported
  file plays as a clip; PCM16 round-trip approximate; `import_wav_at` resamples
  to target rate; peaks summarize imported audio.
- Integration (`daw-render/tests/automation_render.rs`): track-volume automation
  fades the rendered output; step automation switches at the point.

### `daw-audio-io` (unit) — new (live-playback spike)
- `PreparedBuffer`: fills from a position, silences past the end, zero past end,
  mono duplicates to both sides.
- `Player`: stopped outputs silence; play advances and outputs audio; master
  gain scales; seek sets position; stop returns to silence; reaching the end
  stops and emits `ReachedEnd`; meter events report position and peak.
- All device-independent (fake `rtrb` rings, no sound card) — the real-time
  callback path is exercised without hardware, satisfying the "no blocking/
  allocation on the callback" check by construction + review.

### `daw-render` (unit + integration) — new in M1
- `wav`: i16 conversion clamps, canonical PCM16 header/data sizes, float32
  bit-exact first sample, inconsistent channel lengths error.
- `export`: expected length/level, **block size does not affect result**, empty
  project = zero length, file size matches header math, explicit length override,
  parent directories created.
- Integration (`tests/export_slice.rs`): build project with clip → save/reload
  (v2) → render → export float32 WAV → verify header + first sample.

### `daw-project` (unit)
- JSON round-trip preserves the project.
- Bad magic rejected; future schema version rejected; malformed JSON rejected;
  invalid project (no master) fails validation on load.
- File save/load round-trip; atomic save leaves no temp file; save rejects an
  invalid project and writes nothing.
- **Effect chains survive save/reload (including bypass flag and params).**
- **`media_check`: all-present reports empty, detects missing paths, relink
  updates path / returns false for unknown / clears the missing report.**
- Migration: current version passes through; future version rejected;
  **v1 upgraded to current; hand-written legacy v1 document loads (defaults +
  migration).**

### `daw-app` (unit)
- New-project shell has a master track; `describe()` mentions the project and
  the offline status.

### `daw-commands` (integration) — new
- Add track → undo → redo; remove track restores at original index.
- Rename / set-volume are reversible (undo then redo).
- New command clears the redo stack (linear history).
- Add clip + move clip, undo/redo in sequence; set-clip-geometry reversible.
- **Add/reorder/bypass effect and set-effect-param are reversible (param change
  restores previous value; a brand-new param is removed on undo).**
- **Automation: adding a point creates the lane and undo removes it; replacing a
  point restores the previous value; an automation-draw gesture (many points) is
  one undo unit via `transaction`.**
- Invalid command (remove master) rolls back and errors; missing entity errors
  without changing the project.
- `transaction` groups multiple edits into one undo unit; a failed transaction
  rolls back every applied command.
- Undo/redo on empty history error cleanly.

## Principles

- **Determinism:** every test uses fixed inputs and tolerances; none depend on a
  physical sound card or exact wall-clock timing (spec §16.3).
- **Offline-first:** the engine and model are fully testable without hardware
  (spec §2.1, §16).
- **Validation coverage:** both the happy path and invalid/forward-incompatible
  inputs are tested for serialization.

## Deferred test areas (later milestones)

- Recording through a simulated capture source; slow/failed disk writer and
  full bounded queue (M2).
- MIDI note/event scheduling, quantization, loop-boundary behavior (M4).
- Routing cycle rejection / buses (M3+).
- Processor benchmarks and graph-processing time (M5).
- Cross-platform acceptance (Windows/macOS/Linux CI) (as CI is added).

Implemented (no longer deferred): clip split/trim math, all current DSP
(gain/pan/distortion/filter/delay/reverb/EQ/compressor), automation
interpolation, undo/redo inversion, WAV import/export, resampling, waveform
peaks, missing-media detection/relink, and the **real-time player** path
(device-independent tests with fake rings; actual audio-out verified manually
on macOS/CoreAudio).

## CI (planned)

A GitHub Actions (or equivalent) matrix over Windows/macOS/Linux running
`fmt --check`, `clippy -D warnings`, and `test --workspace` is planned for
early Milestone 1. It is **not** set up in Milestone 0.
