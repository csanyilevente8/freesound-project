# Audio Engine

Status: **Milestone 1 (in progress) — offline only.** No audio device, no
real-time thread, no transport clock. This document describes what exists and
the boundaries set up for the future real-time engine.

## What exists

### Offline block render (`daw-engine::render::render_block`)

Given per-track source buffers for a block, applies track gain, honors
mute/solo, sums to a master bus, and applies master gain. Returns the stereo
master block.

### Clip scheduling (`daw-engine::schedule`) — added in M1

`schedule_track_block` fills a track's per-block source buffer from its
`AudioClip`s. For each clip it computes the overlap of the clip's timeline range
with the current block, reads the corresponding source region through the
`AssetSource` trait, applies clip gain, and sums into the destination. This is
the offline, hardware-independent half of playback.

`AssetSource` abstracts sample reading so the scheduler does not depend on file
decoding: tests and the shell use synthetic sources (constant / sine); a
file-backed decoder can implement the same trait later. `project_length_frames`
computes content length from the maximum clip end.

### Offline export (`daw-render`) — added in M1

`render_project` drives `schedule_track_block` + `render_block` across the whole
timeline in configurable-size blocks (result is block-size-independent, tested)
and `export_wav` writes the stereo master to a canonical WAV (ADR-0009).

### Insert-effect chains (`daw-engine::effects`) — added in M1

The persistent `EffectChain`/`EffectInstance` descriptors in `daw-core` (string
`processor_type` + versioned parameter bag + bypass) are turned into live
`AudioProcessor`s by `build_processor` / `ProcessorChain`. `render_block`
applies each track's chain to its signal **before the fader**
(source → inserts → volume), and the master chain after the master fader.
Bypassed effects and **unknown processor types** are skipped (recoverable
missing-effect state, spec §10.5) rather than causing failure. The factory keeps
`daw-core` free of DSP types.

### Audio import (`daw-media`) — added in M1

`daw-media` decodes canonical PCM16/float32 WAV (hand-written reader, ADR-0011)
into planar `f32` and exposes it via `MemoryAssetSource`, which implements
`AssetSource`. This closes the offline media loop: a file exported by
`daw-render` can be re-imported and played as a clip (covered by a round-trip
test). It also provides **Catmull-Rom resampling** (`import_wav_at` converts a
source rate to the project rate) and **waveform-peak** computation
(min/max envelope buckets — the data for a future waveform view). Compressed
formats and streaming are deferred.

### Automation (`daw-engine::automation`) — added in M1

Project-level `AutomationLane`s (spec §5.5) target a stable id (track volume/pan
or an effect parameter) with step/linear interpolation. During offline render,
`automated_project` samples every lane at each block's start and writes the
values into the plain fields the renderer already reads (track volume, effect
params), so the render path needs no automation-specific branches. Per-block
sampling is deterministic; finer per-sample ramping is left to processor
parameter smoothing.

### Signal flow

```
per track:  source buffer ──> track Gain ──┐
                                            ├─> mix (sum) ──> master Gain ──> output
per track:  source buffer ──> track Gain ──┘
```

Mute removes a track before summing. Solo, when any track is soloed, keeps only
soloed tracks.

## Clipping policy

Summing does not clamp (ADR-0004). Intermediate and summed signals may exceed
[-1.0, 1.0]. A limiter/clamp belongs at the master stage and will be explicit.

## Live audio I/O (`daw-audio-io`)

Output (playback) and input (recording) both go through CPAL, confined to
`cpal_backend`:

- **Playback** — `CpalOutput` drives a real-time-safe `Player` from the output
  callback (ADR-0003/0007). Transport/gain commands and meter/position events
  cross via bounded `rtrb` rings.
- **Recording** (spec §9.7) — `CpalInput` pushes captured samples into a
  real-time `CaptureProducer` (bounded ring, never blocks/allocates); a writer
  thread drains the ring to a streaming float32 WAV (`WavStreamWriter`), with
  overrun counting, disk-error reporting, and safe finalize. On stop, the app
  layer imports the take and adds an asset + clip as one undoable transaction
  (undo removes references, never the media file).

## Deferred to later milestones

- A runtime audio graph with node/connection identities and cycle rejection
  (spec §9.3); the renderer composes processors directly with no persistent
  graph. (This is what would make per-track edits audible *during* playback —
  the "prepared-graph swap" of ADR-0007.)
- Simultaneous play **and** record (duplex); input monitoring; a record-time
  input level meter.
- Latency compensation (spec §9.6).
- Tempo-relative scheduling (clips are frame-positioned; tempo is not yet
  applied to playback).

## Real-time safety (planned)

The real-time callback rules (spec §9.2) are not yet exercised because no
callback exists. The intended approach is recorded in ADR-0007 (bounded
lock-free queues for commands/events, off-thread graph build + safe swap,
preallocated buffers). The DSP primitives are already written to be
allocation/lock/I/O-free in `process` so they can be dropped onto the audio
thread unchanged.

No part of the system currently claims to be real-time safe.
