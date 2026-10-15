# Architecture Overview

Working name: **Free Sound** (placeholder — not hard-coded anywhere in the code;
internal crates use a neutral `daw-` prefix and the file format uses the magic
string `daw-project`).

This document describes the architecture **as implemented through Milestone 1
(in progress)**. It is a modular monolith (spec §4): one desktop program with
well-separated internal modules. See `docs/adr/` for the decisions behind each
choice.

## Workspace layout

```
freesound/
├── Cargo.toml                 # workspace, pinned dependency versions
├── crates/
│   ├── daw-core/              # IDs, time, audio value types, model, validation
│   ├── daw-project/           # versioned save/load, migration scaffolding
│   ├── daw-dsp/               # buffer, processor trait, gain/pan/distortion/filter/delay/reverb/eq/compressor, mixer
│   ├── daw-engine/            # offline render + clip scheduling + effect chain + automation
│   ├── daw-render/            # offline WAV export (reuses the engine)
│   ├── daw-media/             # audio import/decoding (WAV), resampling, waveform peaks
│   ├── daw-commands/          # editing commands + undo/redo history
│   ├── daw-audio-io/          # real-time playback: CPAL backend + lock-free rings
│   └── daw-app/               # app shell + `daw` binary (UI framework TBD)
├── docs/
│   ├── architecture/          # this file + per-area docs
│   ├── adr/                   # architecture decision records
│   └── testing/               # testing strategy
└── free_sound_project_spec.md
```

Nine crates exist. `daw-render` (export), `daw-commands` (undo/redo),
`daw-media` (import), and `daw-audio-io` (real-time playback) were added in
Milestone 1 as real boundaries emerged. The remaining spec crate (`daw-midi`)
is intentionally **not** created yet — crates are split when a real boundary
emerges, not speculatively (spec §4.2).

## Dependency graph (crates)

```
daw-core ──┬── daw-dsp ──┐
           │             ├── daw-engine ──┬── daw-render ──┐
           │             │                ├── daw-media ───┤
           ├── daw-project ───────────────┼────────────────┤
           ├── daw-commands ──────────────┼────────────────┤
           │                              │                │
           └──────────────────────────────┴── daw-app (core, project, engine, render, media, commands)
```

`daw-core` depends on nothing internal. `daw-dsp`, `daw-project`, and
`daw-commands` depend only on core. `daw-engine` depends on core + dsp.
`daw-render` and `daw-media` depend on core + dsp + engine. `daw-app` composes
everything. There are no cycles.

## Layer mapping (spec §4.1)

| Spec layer | Location | Status |
|---|---|---|
| Domain/core | `daw-core` | project/track + media assets + audio clips |
| Project/session | `daw-project` | save/load, versioning (schema v2), migration |
| Application services | `daw-app::AppState` | new/demo project, describe, export driver |
| Commands/undo-redo | `daw-commands` | command trait + history + core edit commands |
| Audio engine | `daw-engine` | offline render + clip scheduling + effect chain + automation |
| DSP/instruments | `daw-dsp` | gain, pan, distortion, filter, delay, reverb, EQ, compressor, mixer |
| Rendering/export | `daw-render` | offline WAV export (PCM16/float32) |
| Media import | `daw-media` | WAV decode, resampling, waveform peaks, in-memory `AssetSource` |
| Platform adapters | `daw-audio-io` | CPAL output backend behind a trait; real-time player + lock-free rings |
| UI | `daw-app` binary | text/CLI shell; framework decided (egui, ADR-0002, pending prototype) |

## Interface ownership (spec §4.3) — how it is enforced

- Core types contain **no** GUI or audio-device-library types. `daw-core`'s only
  external dependencies are `serde`, `uuid`, `thiserror`.
- The project **format** is a dedicated versioned document type
  (`daw_project::ProjectDocument`), not a raw dump of internal structs.
- DSP processors operate on `AudioBuffer` + `ProcessConfig` only; they never see
  tracks, projects, files, or UI.
- The app shell exposes a framework-agnostic `AppState`; no GUI dependency
  exists, so a future frontend cannot leak into core.
- `#![forbid(unsafe_code)]` is set on every crate.

## Real-time safety posture (Milestone 0)

The offline render path (`daw_engine::render_block`) is **not** on an audio
thread and intentionally allocates (it is offline). The *shared DSP primitives*
it reuses are built to be allocation- and lock-free in `process`:

- `AudioBuffer` allocates only in `new`; `clear`, `add_from`, `channel_mut`, and
  `peak` are allocation-free.
- `GainProcessor::process` is a plain multiply / ramp with no allocation, locks,
  or I/O. Smoothing frames are resolved in `prepare`.
- `mix_into` only clears and adds in place.

No claim is made that a real-time *engine* exists or is RT-safe — there is no
audio thread yet. The design that will make it RT-safe is recorded in ADR-0007.

## Dependency inventory

All versions are pinned in the workspace `Cargo.toml`. All are MIT/Apache-2.0,
which is compatible with the project's proposed GPL-3.0-or-later license
(ADR-0001).

| Crate | Version | Purpose | License | Platforms | Replaceable? |
|---|---|---|---|---|---|
| `serde` | 1.0.210 | Derive-based (de)serialization | MIT/Apache-2.0 | all | yes (format behind `daw-project`) |
| `serde_json` | 1.0.128 | JSON encoding of project docs | MIT/Apache-2.0 | all | yes (ADR-0005 allows binary later) |
| `uuid` | 1.10.0 (v4) | Stable entity identifiers | MIT/Apache-2.0 | all | yes (behind `daw_core::id` newtypes) |
| `thiserror` | 1.0.64 | Ergonomic error enums | MIT/Apache-2.0 | all | yes (std error impls) |
| `cpal` | 0.18.2 | Cross-platform audio output device (ADR-0003) | MIT/Apache-2.0 | Win/mac/Linux | yes (behind `daw-audio-io` trait) |
| `rtrb` | 0.4.0 | Real-time-safe SPSC rings for command/event (ADR-0007) | MIT/Apache-2.0 | all | yes (behind the ring API) |
| `tempfile` | 3.13.0 (dev only) | Temp dirs in tests | MIT/Apache-2.0 | all | yes |

The offline portion of Milestone 1 added no runtime dependency (hand-written
WAV reader/writer, ADR-0009/0011). The **live-playback** slice added the first
audio-adjacent runtime deps — `cpal` and `rtrb` — both confined to
`daw-audio-io`; CPAL/rtrb types do not appear in core/engine. `symphonia` for
compressed import remains deferred (ADR-0011).

Audio playback uses the OS audio device (CoreAudio/WASAPI/ALSA via CPAL); there
is still no networking, telemetry, accounts, or cloud dependency — the
application works offline and renders without any device (spec §1 item 8).

## Known limitations

See `docs/testing/strategy.md` and the Milestone 0 report for the full list.
In short: no device I/O, no playback/recording, no clips/media, no arrangement
editor, no automation, no MIDI, no effects beyond gain, and the GUI framework is
not yet chosen.
