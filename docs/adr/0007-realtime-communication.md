# ADR-0007: Real-time command/parameter communication

**Status:** Accepted (initial) — implemented for the first live-playback slice.
The queue dependency is `rtrb`; the design is intentionally minimal and expands
when concrete features need it.

## Context

The UI must never directly mutate memory the audio thread is reading (spec §4,
§9.2). Control changes (transport commands, parameter updates) have to cross
from the UI/application thread to the audio thread without locks, I/O, or
allocation on the audio side. Likewise the audio thread must report meters/
status back without blocking.

## Decision

- **Commands (control → audio):** a bounded, lock-free **SPSC** ring (`rtrb`,
  MIT/Apache-2.0) carrying small `Copy` command messages (`Transport::Play`,
  `Stop`, `Seek(frames)`, `SetMasterGain(f32)`, …). The audio callback drains
  the queue at the start of each block. If the ring is full the **producer**
  (control side) handles it (drop/coalesce/retry) — the audio side never blocks.
- **Events (audio → control):** a second bounded SPSC ring for meter snapshots
  (peak per channel), the current playhead frame, and xrun/underflow counts.
  The audio side `push`es non-blocking (dropping on full); the control side
  polls. GUI widgets are never touched from the audio thread.
- **Parameter smoothing:** continuous params that could click (gain, pan,
  cutoff) are ramped on the audio side; `GainProcessor` already does this.
- **Audio-owned state:** everything the callback reads/writes (transport
  position, prepared processors, scratch buffers) is owned by the audio side and
  only mutated there; all external influence arrives via the command ring.
- **Graph/parameter changes beyond the above:** deferred. The current slice
  plays a prepared render with transport + master-gain control only. A
  prepared-graph swap (atomic pointer / triple-buffer) will be added when live
  effect edits during playback are needed; it is not required to play audio.

## Real-time callback rules (enforced in review + tests)

The audio callback must not: allocate/free, lock, do file/network I/O, block on
another thread, log, or do unbounded work (spec §9.2). It drains the command
ring (bounded), pulls one block from the prepared source, applies master gain,
writes meter/playhead events (bounded, drop-on-full), and returns. A test drives
the block-production path with a fake sink to assert it runs without touching
these.

## Consequences

- First real third-party audio-adjacent dependency: `rtrb` (plus `cpal` from
  ADR-0003). Both MIT/Apache-2.0, GPL-compatible (ADR-0001).
- The command/event message enums live in `daw-audio-io` (the real-time layer),
  not in `daw-core` — core stays free of real-time concerns.
- Variable callback block size (ADR-0003) is handled: the producer serves
  whatever frame count the callback requests, up to a prepared maximum.

## Revisit when

Adding live effect-graph edits during playback (prepared-graph swap), recording
(a capture ring to a writer thread), or MIDI event scheduling.
