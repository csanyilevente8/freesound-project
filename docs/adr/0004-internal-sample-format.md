# ADR-0004: Internal sample format and channel model

**Status:** Accepted.

## Context

The engine needs one internal sample format and a channel-layout model (spec
§9.4). It must handle mono/stereo explicitly and leave room for multichannel,
without assuming everything is stereo.

## Decision

- **Sample format:** `f32`, non-interleaved (planar) buffers. Exposed as
  `daw_core::audio::Sample` (= `f32`) and `daw_dsp::AudioBuffer`, which stores
  `channels * frames` samples channel-major and hands out per-channel slices.
- **Channel model:** an explicit `ChannelLayout` enum (`Mono`, `Stereo`) with a
  `channel_count()` accessor. Richer layouts are added as new variants later;
  code that matches on the enum will be forced to handle new cases.
- **Clipping policy:** the summing mixer does **not** clamp; summed signals may
  exceed [-1.0, 1.0]. Any limiting is the master stage's responsibility and
  must be explicit (spec §9.4). Documented and tested
  (`mixer::tests::mixing_can_exceed_unity_without_clipping`).

## Rationale

`f32` planar is the common DSP layout, avoids per-sample stride math, and keeps
device-format conversion at the adapter boundary. Explicit layout enum prevents
silent stereo assumptions.

## Consequences

- Device adapters must convert interleaved ↔ planar and i16/i24/f32 ↔ f32 at the
  boundary (Milestone 1).
- `f64` processing (if ever needed for specific DSP) would be a localized
  decision inside a processor, not a global format change.

## Revisit when

A processor needs `f64` internally, or when adding >2-channel layouts.
