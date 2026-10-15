# ADR-0008: Processor API and graph update strategy

**Status:** Accepted (initial contract; will be extended as features require).

## Context

DSP processors must be small, composable, testable units that know nothing about
tracks, projects, files, or UI (spec §10). The spec offers a conceptual
`AudioProcessor` trait and lists open questions (in-place vs separate buffers,
channel negotiation, latency, smoothing, MIDI delivery, bypass, state
save/restore, offline behavior).

## Decision (initial)

```rust
pub struct ProcessConfig { sample_rate, max_block_frames, channels }

pub trait AudioProcessor: Send {
    fn prepare(&mut self, config: &ProcessConfig); // may allocate
    fn process(&mut self, buffer: &mut AudioBuffer); // RT-safe, in-place
    fn reset(&mut self);                             // clear transient state
}
```

Resolved for Milestone 0:

- **In-place processing.** `process` reads and writes one `AudioBuffer`.
  Simplest correct model for the first processors; avoids a second buffer and
  copies. Processors needing separate in/out can be revisited when one appears.
- **Allocation discipline.** All allocation happens in `prepare`; `process` is
  allocation- and lock-free. `Send` lets a prepared processor move to the audio
  thread.
- **Parameter smoothing.** Demonstrated by `GainProcessor` (linear ramp over a
  configurable window, resolved to frames in `prepare`).
- **Reset.** Clears transient state (ramps/filter history) without reallocating.

## Explicitly deferred (added when a feature needs them)

- MIDI/event delivery to `process` (needed for instruments — Milestone 4).
- Processor latency reporting (for latency compensation — later).
- Bypass semantics, state save/restore/versioning (effect persistence — M3).
- Channel-layout negotiation beyond a fixed `channels` (multichannel — later).
- Denormal handling (perf hardening — M5).

## Graph update strategy

Milestone 0 has no runtime graph — the offline renderer composes processors
directly per block. The future real-time graph will follow ADR-0007: build and
prepare a new graph off-thread, swap it in at a safe boundary; keep topology
separate from parameter values so parameter changes never rebuild the graph.

## Consequences

- Extending the trait (e.g. adding an `events` parameter) is a breaking change
  to processors; doing it when the first consumer exists keeps churn low.
- The in-place model is tested across gain, mixing, and the end-to-end slice.

## Revisit when

Adding instruments (events), effect persistence (state/bypass), or latency
compensation.
