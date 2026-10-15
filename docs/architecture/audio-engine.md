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

## Deferred to later milestones

- Real-time transport (play/stop/record), sample-position clock (spec §9.5).
- Device adapter and callback (ADR-0003, next in Milestone 1).
- A runtime audio graph with node/connection identities and cycle rejection
  (spec §9.3); the renderer composes processors directly with no persistent
  graph.
- Audio **import/decoding** and resampling (spec §12) — asset references and
  scheduling exist; the decoder behind `AssetSource` does not.
- Metering snapshots to the UI, xrun/dropout reporting (spec §9.8).
- Latency concepts and compensation (spec §9.6).
- Recording pipeline with a writer thread and bounded ring buffer (spec §9.7).

## Real-time safety (planned)

The real-time callback rules (spec §9.2) are not yet exercised because no
callback exists. The intended approach is recorded in ADR-0007 (bounded
lock-free queues for commands/events, off-thread graph build + safe swap,
preallocated buffers). The DSP primitives are already written to be
allocation/lock/I/O-free in `process` so they can be dropped onto the audio
thread unchanged.

No part of the system currently claims to be real-time safe.
