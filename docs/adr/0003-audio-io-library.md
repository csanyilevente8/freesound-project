# ADR-0003: Audio I/O library

**Status:** Accepted pending prototype — **CPAL** is the chosen initial
cross-platform audio adapter, behind a Free Sound-owned interface, to be
confirmed by a headless playback spike (gates below). Implementation is not
started (we remain offline until the UI/playback phase).

## Context

Live playback and recording need cross-platform device access (WASAPI/CoreAudio
/ALSA/JACK) with a real-time callback (spec §9). The chosen library must be a
replaceable *device-I/O layer*, not the engine itself (§3.1), and its types must
not leak into core APIs (§4.3). The offline render path
(`daw_engine::render_block`) already produces planar `f32` blocks and will be
reused unchanged by the real-time path.

## Decision

Adopt **CPAL** as the initial audio backend, **behind a project-owned device
trait** in a future `daw-audio-io` crate (not yet created). CPAL types must not
appear in `daw-core`/`daw-engine`/`daw-dsp`. The decision is confirmed once a
headless playback spike passes the gates below.

### Why CPAL

- Pure-Rust, cross-platform (WASAPI/CoreAudio/ALSA), widely used, actively
  maintained, permissive license (MIT/Apache-2.0, GPL-compatible per ADR-0001).
- Abstracts host/device enumeration and the callback, which is the bulk of the
  boilerplate.

### Explicitly out of CPAL's scope (our responsibility)

- **Real-time safety is NOT provided by CPAL.** CPAL hands us a callback on a
  high-priority thread; everything in ADR-0007 — a bounded lock-free command/
  event queue, no allocation/locks/I/O in the callback, and swapping a prepared
  graph at a safe boundary — remains our work. "Choosing CPAL" does not
  discharge the real-time engine design; it must exist from the first playback
  slice, because retrofitting thread-safety after UI code mutates shared state
  is the classic failure the spec warns against (§4, §9.2).
- **Variable block size.** CPAL does not guarantee a fixed callback buffer size.
  The adapter must handle variable/short blocks; `ProcessConfig.max_block_frames`
  already models an upper bound rather than a fixed size.
- **ASIO (Windows)** needs the SDK and a separate CPAL feature/build; treat as
  additional backend work, not part of the initial slice.
- **Advanced Linux routing (JACK)** may need a dedicated backend later.

### Interface scope (keep it minimal)

Define only what the current engine needs:
- open a default output stream at a requested sample rate / channel count,
- a callback that pulls planar `f32` blocks from the engine (via the ADR-0007
  queue) and converts to the backend's interleaved format at the boundary,
- start/stop, and a non-blocking error/status report.

No device-enumeration abstraction, no format-negotiation layer, and no input/
recording surface until a concrete requirement (device picker, recording)
demands it.

## Prototype acceptance gates (headless, no GUI)

1. **Plays the existing offline-rendered buffer** through the default output
   device on macOS + one of Windows/Linux, audibly correct.
2. **Real-time discipline:** the callback performs no heap allocation, no
   locking, and no I/O; control/data crosses via the ADR-0007 bounded queue.
   Verified by code review + a test that exercises the queue with a fake device
   (no real card in CI).
3. **Variable block size tolerated:** feeding blocks smaller than
   `max_block_frames` produces correct output.
4. **Clean start/stop and device-error reporting** without blocking the audio
   thread.

Record the spike result (pass/fail per gate) as a short note appended here when
the playback phase begins.

## Consequences

- A `daw-audio-io` crate wraps CPAL behind the trait above; the engine gains a
  real-time wrapper that owns the queue and prepared graph.
- Replacing CPAL (e.g. native WASAPI/JACK backends) later touches only
  `daw-audio-io`.
- CPAL (and any ASIO/JACK features) are new dependencies evaluated for license/
  platform at adoption and added to the inventory.
- The offline path is unaffected and remains the authoritative, testable
  reference the real-time path is compared against.

## Revisit when

Starting the playback phase (run the spike), adding recording/input, or needing
ASIO/JACK.
