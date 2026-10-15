# ADR-0006: Time representation

**Status:** Accepted.

## Context

Clip/note positions and audio processing need an authoritative time model.
Floating-point seconds alone accumulate rounding error and must not be the sole
authority (spec §5.4). Audio uses sample frames; musical editing uses
beats/bars/ticks.

## Decision

Two integer-based authoritative clocks in `daw_core::time`:

- **`SampleTime`** — a `u64` count of sample frames at a known `SampleRate`.
  Authority for audio processing.
- **`MusicalTime`** — a `u64` count of PPQ ticks, with `PPQ = 960` ticks per
  quarter note. Authority for musical editing.

Supporting types: `SampleRate` (non-zero `u32`), `Tempo` (finite, positive
`f64` BPM), `TimeSignature` (non-zero numerator/denominator).

Conversions:
- `SampleTime ↔ seconds` for display/device interop only (`to_seconds`,
  `from_seconds`, the latter clamping negatives and rounding to nearest frame).
- `MusicalTime → SampleTime` at a fixed tempo (`to_samples`). Tempo-automation-
  aware conversion is deferred until tempo automation exists.

## Rationale

Integer ticks and frames give exact, reproducible positions and avoid cumulative
float error. 960 PPQ divides cleanly into common subdivisions and triplets.
Rounding behavior is defined and unit-tested
(`time::tests::musical_to_samples_*`, `seconds_round_trip_is_frame_exact`).

## Consequences

- Tempo-map (variable tempo) conversion is a future addition; the current API
  takes a single `Tempo` so the call sites are easy to find and upgrade.
- `u64` frames at 48 kHz covers ~12 million years — no practical overflow;
  saturating arithmetic is provided regardless.

## Revisit when

Adding tempo automation / a tempo map, or sub-tick resolution needs.
