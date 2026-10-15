# ADR-0009: Audio export writer (WAV)

**Status:** Accepted (initial — hand-written WAV writer; decoder/other formats
deferred).

## Context

Milestone 1 requires stereo WAV export (spec §8.8): write the offline-rendered
master to a file with selectable sample format. Spec §12 also calls for *import*
(decoding) later, with decoding dependencies chosen only after license/platform
review (§3.2). This ADR covers the **writer** needed now; the decoder is a
separate, later decision.

## Options for the writer

1. **Hand-written RIFF/WAVE writer** (no dependency). WAV's canonical PCM/float
   layout is simple and stable. Full control; zero added dependency surface.
2. **`hound` crate.** Mature, popular WAV read/write. Adds a dependency and
   pulls its own transitive tree.
3. **`symphonia` / `wav` / others.** Heavier; aimed more at decoding.

## Decision

Use a **small hand-written writer** (`daw_render::wav`) supporting canonical
little-endian PCM16 and IEEE-float32, mono/stereo. Rationale:

- The WAV write path is tiny and well-specified; a dependency is not justified
  yet (keeps the dependency inventory minimal — ADR-0001 licensing discipline).
- Float32 output gives bit-exact tests of the engine's internal `f32`.
- The clipping policy is applied here on the PCM16 conversion (clamp to
  [-1.0, 1.0]), consistent with ADR-0004 (the mixer does not clamp).

## Consequences

- Only canonical PCM16 / float32 WAV is produced; no extensible-format, no
  compressed formats, no metadata chunks. Sufficient for export.
- **Import/decoding is NOT addressed here.** When import arrives, a decoder
  library (e.g. `symphonia`) will be evaluated in its own ADR with license and
  platform review. Until then, the engine's `AssetSource` trait is fed by
  synthetic/test sources.
- If richer export (bit depths, WAVE_FORMAT_EXTENSIBLE, other containers) is
  needed, adopting `hound`/`symphonia` can be recorded as a superseding ADR; the
  writer is isolated behind `daw_render::write_wav`.

## Revisit when

Adding audio import/decoding, or when export needs formats beyond PCM16/float32
WAV.
