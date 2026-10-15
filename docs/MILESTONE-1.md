# Milestone 1 — Progress Report (Slices 1–2)

**Goal (P1):** audio clip model + import, timeline/tracks/playhead UI, device
playback, track/master gain, meters, WAV offline export, undo/redo for core
arrangement actions. **Exit criteria:** import an audio file, place it on a
timeline, play it, adjust level, save/reopen, and export a stereo mix.

This report covers the **offline-testable slices** done so far. The device
playback + GUI slice (which needs ADR-0002/0003) is **not** yet implemented.

---

## Slice 4 — Audio import/decoding (ADR-0011)

New `daw-media` crate:
- Hand-written canonical WAV reader (PCM16 / float32, mono/stereo), tolerant of
  extra RIFF chunks, with typed errors for truncated/non-WAVE/unsupported input.
- `import_wav` → `MediaAssetRef` + decoded planar `f32`; `MemoryAssetSource`
  implements `daw_engine::AssetSource` so imported files play as clips.
- Decision: WAV via hand-written reader now (no new dependency); `symphonia`
  for compressed formats later, behind the same boundary. Resampling and
  streaming deferred.
- Tests incl. an export→import round-trip (float32 bit-exact, PCM16 approximate,
  imported file plays as a clip).

## Slice 5 — More DSP processors (toward P3)

Added to `daw-dsp`: `PanProcessor` (constant-power −3 dB-center law, smoothed),
`DistortionProcessor` (tanh soft-clip with drive/output), `FilterProcessor`
(Zavalishin TPT state-variable filter, LP/HP/BP, resonant, stable under
modulation). Deterministic tests for each.

## Slice 6 — Effect/insert-chain model + bypass + persistence (P3 groundwork)

- `daw-core::effect`: `EffectInstance` (stable id, string `processor_type`,
  bypass, versioned param bag) and ordered `EffectChain`; attached to `Track`.
  **Schema v3** with a v2→v3 migration. Unknown processor types survive load as
  recoverable placeholders (spec §10.5).
- `daw-engine::effects`: factory mapping descriptors → live processors;
  `ProcessorChain` skips bypassed/unknown effects; `render_block` now runs each
  track's chain (before fader) and the master chain. Chain order respected.
- `daw-commands`: add/remove/insert/move (reorder) effect, set bypass, set/
  remove parameter — all undoable.
- Effect chains survive save/reload (bypass + params), covered by a test.

No new runtime dependency across slices 4–6.

---

## Slice 7 — Automation model (spec §5.5)

- `daw-core::automation`: `AutomationLane` (target + interpolation + sorted
  points), `AutomationTarget` (track volume/pan or effect param, by stable id),
  `AutomationPoint`, step/linear `value_at` with flat end-extrapolation.
  `project.automation` added (**schema v4** + v3→v4 migration); lanes validated
  against existing targets (dangling-reference detection).
- `daw-engine::automated_project`: samples lanes per block and writes values
  into the fields the renderer reads, so render needs no automation branches.
  Wired into `render_project`.
- `daw-commands`: `SetAutomationPoint` / `RemoveAutomationPoint`, undoable, with
  an automation-draw gesture grouped into one undo unit via `transaction`.
- Integration test: track-volume automation fades the exported output.

## Slice 8 — More DSP (delay, reverb, EQ, compressor)

Added to `daw-dsp`: `DelayProcessor` (feedback delay, clamped feedback, wet/dry),
`ReverbProcessor` (Schroeder comb+allpass), `EqBand` (RBJ biquad: peaking/
low-shelf/high-shelf), `CompressorProcessor` (feed-forward, attack/release,
makeup, exposes gain-reduction metering). All registered in the engine effect
factory, so they work in chains and are automatable. Deterministic tests each.

## Slice 9 — Resampling

`daw-media::resample`: Catmull-Rom cubic sample-rate conversion;
`import_wav_at` imports and resamples to the project rate. Deterministic tests
(length scaling, DC/ramp preservation, finiteness).

## Slice 10 — Waveform peaks

`daw-media::peaks`: pure min/max envelope (buckets) over planar channels — the
data for a future waveform view; a rebuildable cache, not authoritative audio.

## Slice 11 — Missing-media detection + relink

`daw-project::media_check`: `find_missing_media` (injectable existence check)
and `relink_asset` by stable id — pure helpers for the reopen/relink workflow
(spec §6.2, §7.11), testable without a filesystem.

No new runtime dependency across slices 7–11.

---

## Slice 12 — Live playback (CPAL headless spike; ADR-0003, ADR-0007)

New `daw-audio-io` crate (first audio runtime deps: `cpal` 0.18, `rtrb` 0.4):
- `BlockSource` trait + `PreparedBuffer` (whole project rendered offline, then
  streamed) — real-time-safe producer.
- `Player`: audio-side core — transport, lock-free command drain, master gain,
  meter/`ReachedEnd` events. Device-independent; 11 unit tests with fake rings.
- `cpal_backend`: the only module touching CPAL; drives the player from the
  device callback, converting f32 → device format in fixed stack chunks.
- `PlaybackHandle` + `playback_channel`: control-side API over bounded rings.
- ADR-0007 moved to **Accepted**; ADR-0003 to **Accepted** (spike passed).
- `daw play <proj.json> [gain]` CLI command: renders at the device rate and
  plays live. Verified on macOS/CoreAudio (clean `ReachedEnd` at the expected
  duration). Actual audibility is a manual check; the pipeline is validated.

---

## Slice 3 — Undo/redo command model (added after slices 1–2)

New `daw-commands` crate (ADR-0010):
- `Command` trait: `label()` + `apply(&mut Project) -> Box<dyn Command>` (the
  inverse). On error the project is left unchanged.
- `History`: executes commands, validates the whole project after each apply and
  rolls back via the inverse if invalid, records inverses for undo, clears redo
  on new edits. `transaction(label, body)` groups a gesture into one undoable
  unit with all-or-nothing rollback (spec §13).
- Commands: add/remove/insert/rename track, set track volume/mute,
  add/remove/insert/move clip, set clip geometry (trim/split results).
- 12 integration tests: apply/undo/redo, inverse correctness, redo
  invalidation, validation rollback, missing-entity errors, gesture grouping,
  and transaction rollback.

No new runtime dependency.

---

## What was implemented

### Slice 1 — Clip & media model + schema v2
- `daw_core::media::MediaAssetRef` — a reference to immutable source audio
  (id, path as a platform-independent string, sample rate, channels, frames).
- `daw_core::clip::AudioClip` — non-destructive clip distinguishing
  `timeline_start`, `source_offset`, and `duration` (spec §5.3). Operations:
  `timeline_end`, `overlaps`, `trim_start` (keeps source aligned), `trim_end`,
  `split_at` (adjacent regions over the same asset).
- `Project` now holds `assets`; `Track` now holds `clips` (both
  `#[serde(default)]` for forward-compat).
- Validation extended: unique asset ids, valid assets/clips, unique clip ids,
  and **every clip must reference an existing asset** (dangling-reference
  detection).
- **Schema v2** with a real `v1 → v2` migration and a hand-written v1-document
  load test — exercising the migration mechanism built in M0.

### Slice 2 — Clip scheduling + offline WAV export
- `daw_engine::schedule` — `AssetSource` trait (sample reading abstracted away
  from file decoding), `schedule_track_block` (fills a track block from its
  clips with correct timeline/source-offset math and block-boundary overlap),
  and `project_length_frames`.
- `daw-render` crate:
  - `wav` — a hand-written, dependency-free canonical WAV writer (PCM16 +
    float32, mono/stereo) with the export clipping policy (ADR-0009).
  - `export` — `render_project` (whole-timeline, block-size-independent) and
    `export_wav`.
- The `daw` shell gained `--export <path.wav>`, which builds a demo tone project
  (synthetic `SineSource`) and writes a 1-second stereo WAV.

## Architecture decisions
- **ADR-0009 (new):** export writer = hand-written WAV (PCM16/float32). Import
  decoding deferred to its own ADR.
- No new runtime dependency added (WAV writer hand-written; scheduling reads via
  `AssetSource`).
- **ADR-0002 / ADR-0003 (updated):** GUI framework = **egui** and audio backend
  = **CPAL**, both **accepted pending a prototype** with explicit gates
  (egui: an accessibility/keyboard gate; CPAL: a headless real-time-safe
  playback spike). Not yet implemented — we remain offline until the UI/playback
  phase.

## Files changed (important)
```
Cargo.toml                              + daw-render member/dep
crates/daw-core/src/media.rs            NEW  media asset reference
crates/daw-core/src/clip.rs             NEW  audio clip + trim/split
crates/daw-core/src/track.rs            + clips field, add_clip
crates/daw-core/src/project.rs          + assets, accessors, validation
crates/daw-core/src/lib.rs              + exports
crates/daw-project/src/document.rs      schema v1 -> v2
crates/daw-project/src/migrate.rs       v1->v2 migration step + test
crates/daw-project/src/io.rs            legacy-v1 load test, version test fix
crates/daw-engine/src/schedule.rs       NEW  clip scheduler + AssetSource
crates/daw-engine/src/lib.rs            + exports
crates/daw-render/                      NEW crate: wav, export (+ integration test)
crates/daw-app/src/app.rs               + SineSource, demo_project, describe update
crates/daw-app/src/main.rs              + --export mode
docs/adr/0009-audio-export-writer.md    NEW
docs/architecture/*, docs/adr/README.md updated
```

## Verification results (exact commands, run on macOS)

| Command | Result |
|---------|--------|
| `cargo build --workspace` | **OK** (8 crates) |
| `cargo fmt --all -- --check` | **OK** — no diffs |
| `cargo clippy --workspace --all-targets -- -D warnings` | **OK** — no warnings |
| `cargo test --workspace` | **OK** — **201 passed, 0 failed** |
| `cargo run -p daw-app --bin daw -- --export <path>` | **OK** — writes a valid 1 s stereo PCM16 WAV |

The exported WAV header/data were independently validated (tag=1/PCM,
2 channels, 48000 Hz, 16-bit, 192000 data bytes = 1 s).

New tests since M0 (+30): clip math/validation, media validation, asset/clip
project invariants, v1→v2 migration + legacy load, WAV writer, clip scheduler,
export (incl. block-size invariance), and an export integration test.

## Acceptance criteria (spec P1)

| Criterion | Status |
|-----------|--------|
| Audio clip model | **Passed** |
| Audio **import** (file decoding) | **Passed** for WAV (`daw-media`); compressed formats deferred (ADR-0011) |
| Timeline ruler, tracks, playhead, clip selection/movement (UI) | **Not implemented** (egui chosen, ADR-0002, pending prototype) |
| Audio playback through a device adapter | **Passed** (CPAL output via `daw-audio-io`; `daw play`) |
| Track and master gain | **Passed** (M0 + applied in render) |
| Basic meters | **Partial** — buffer `peak()` and compressor gain-reduction exist; a UI meter is deferred to the UI phase |
| WAV offline export | **Passed** |
| Undo/redo for core arrangement actions | **Passed** (command model in `daw-commands`) |

**P1 overall: partially complete.** The offline, testable foundation of P1 is
in place; the device-playback and GUI parts are the remaining work and are
blocked on two deliberately-open ADRs.

## Known limitations
- No audio file decoding yet — clip audio comes from synthetic/test sources.
- No GUI, no transport/playback, no device I/O, no metering UI, no undo/redo.
- Export is PCM16/float32 WAV only; no other formats/bit depths.
- `schedule_track_block` allocates a small scratch `Vec` per clip/channel in the
  offline path (explicitly acceptable offline; the real-time path will
  preallocate scratch per ADR-0007).
- Verified on macOS only; no cross-platform CI yet.

## Next milestone slice (proposed, not started)
**Decide ADR-0003 (audio I/O; evaluate CPAL) and ADR-0002 (GUI; evaluate egui),
then build:**
1. A `daw-audio-io` device adapter behind a narrow trait, converting the
   engine's planar f32 ↔ the backend's interleaved format.
2. A real-time engine wrapper feeding the device callback from the shared render
   path, using bounded lock-free queues (ADR-0007) for transport/parameter
   commands and meter events.
3. A minimal arrangement view (chosen GUI) showing tracks, clips, and a
   playhead, with level faders.

Each step kept testable: the device adapter gets a deterministic fake-device
test; the real-time wrapper is validated for no-allocation on the callback path.
