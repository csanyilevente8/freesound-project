# ADR-0007: Real-time command/parameter communication

**Status:** Proposed (design recorded; not yet implemented — no real-time thread
exists in Milestone 0).

## Context

The UI must never directly mutate memory the audio thread is reading (spec §4,
§9.2). Control changes (parameter updates, graph edits, transport commands) have
to cross from the UI/application thread to the audio thread without locks, I/O,
or allocation on the audio side.

## Why only proposed now

Milestone 0 has no audio thread — the engine runs offline, synchronously. There
is no concurrency to coordinate yet. Locking in an implementation now would be
speculative. This ADR records the intended design so M0's structure stays
compatible with it.

## Proposed design

- **Commands (UI → audio):** a bounded, lock-free SPSC/MPSC ring (e.g.
  `rtrb`/`ringbuf`, to be evaluated) carrying small, `Copy` or pre-allocated
  command messages. The audio thread drains the queue at block boundaries. Full
  queue → the producer blocks on the *UI* side or coalesces, never the audio
  side.
- **Parameter updates:** either small command messages or per-parameter atomic
  values with smoothing on the audio side (the `GainProcessor` already supports
  ramped changes, demonstrating the smoothing half of this).
- **Events (audio → UI):** a separate bounded queue for meter snapshots,
  xruns, and status. The audio thread writes non-blocking; the UI polls.
- **Graph changes:** build/validate a new graph off-thread, then swap in a
  prepared graph at a safe boundary (triple-buffer / atomic pointer swap). The
  audio thread never constructs or destroys graph nodes.
- **Ownership:** audio-thread-owned data is only mutated by the audio thread;
  everything else arrives via the queues above.

## Consequences

- The processor contract (ADR-0008) must support a prepared/swap model.
- Dependency choice for the queue is deferred to when the real-time engine is
  built (Milestone 1).

## Revisit when

Implementing the real-time engine and device callback (Milestone 1).
