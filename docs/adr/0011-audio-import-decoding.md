# ADR-0011: Audio import and decoding

**Status:** Accepted (initial — WAV decoding via a hand-written reader;
compressed-format decoding via `symphonia` deferred behind a trait).

## Context

Milestone 1 needs to import audio so clips can play real files (spec §12). The
spec says to import common **uncompressed** formats first and to pick decoding
dependencies only after a license/platform review (§3.2, §12). Decoding must sit
behind a replaceable boundary and must not block the real-time callback.

## Options

1. **Hand-written WAV reader (no dependency).** Covers canonical PCM16 / float32
   WAV — the exact formats we already export (ADR-0009). Tiny, zero added
   dependency surface, fully under our control. Does not cover compressed
   formats or exotic WAV variants.
2. **`hound`** (MIT/Apache-2.0). Mature WAV read/write. Adds a dependency for
   something we can write in ~100 lines; still WAV-only.
3. **`symphonia`** (MPL-2.0). Pure-Rust demux+decode for WAV, FLAC, MP3, AAC,
   OGG/Vorbis, and more; cross-platform; actively maintained. Heavier dependency
   tree. **License note:** MPL-2.0 is file-level copyleft and is compatible with
   our GPL-3.0-or-later application (ADR-0001) — MPL-2.0 code can be combined
   with GPL code, with the MPL files remaining under MPL. No GPL conflict.

## Decision

Two layers:

- **Boundary:** a `Decoder`/import function in `daw-media` (new crate) that
  produces a decoded, planar `f32` buffer plus a `MediaAssetRef`. The engine
  already reads samples through `daw_engine::AssetSource`; a decoded-in-memory
  source implements that trait.
- **Milestone 1 implementation:** a **hand-written WAV reader** (PCM16 /
  float32, mono/stereo, canonical RIFF), mirroring the existing writer. This
  gives a complete offline round-trip (export a WAV, re-import it) with **no new
  dependency**.

`symphonia` is the chosen path for **compressed/other formats later**; it is not
pulled in now because WAV import satisfies the P1 exit criteria and keeps the
dependency inventory minimal. When compressed import is scheduled, adopting
`symphonia` is a contained change behind the same boundary and will update this
ADR's status.

## Consequences

- M1 import supports canonical PCM16 / float32 WAV only. Unsupported/corrupt/
  truncated files are detected and reported (spec §12) with a typed error.
- Imported media is treated as immutable source (spec §12); the importer reads,
  it never rewrites the file.
- Resampling (source rate ≠ engine/export rate) is **not** implemented yet; the
  importer records the source rate and import rejects/flags mismatches until a
  resampler is chosen (its own future decision).
- Decoding currently loads the whole file into memory. Streaming large files
  without loading them entirely (spec §12) is deferred; the `AssetSource` trait
  already allows a future streaming/file-backed implementation.
- No new runtime dependency added in this slice.

## Revisit when

Adding compressed-format import (adopt `symphonia`), streaming decode for large
files, or resampling.
