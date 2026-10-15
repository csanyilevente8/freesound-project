//! Minimal, dependency-free WAV writer (see spec §8.8).
//!
//! Supports stereo/mono output in two formats:
//! * **PCM 16-bit** (`WAVE_FORMAT_PCM`) — the common export format.
//! * **IEEE float 32-bit** (`WAVE_FORMAT_IEEE_FLOAT`) — lossless of the engine's
//!   internal `f32`, useful for bit-exact tests.
//!
//! Only canonical little-endian RIFF/WAVE is produced. A hand-written writer is
//! used instead of a dependency to keep the dependency surface minimal
//! (ADR-0001 licensing discipline); if richer format support is needed later,
//! adopting a crate such as `hound` can be recorded in an ADR.

use std::io::{self, Write};

use daw_core::audio::Sample;

/// Output sample format for the WAV file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavFormat {
    /// 16-bit signed integer PCM.
    Pcm16,
    /// 32-bit IEEE float.
    Float32,
}

impl WavFormat {
    const fn bits_per_sample(self) -> u16 {
        match self {
            WavFormat::Pcm16 => 16,
            WavFormat::Float32 => 32,
        }
    }

    /// WAVE format tag: 1 = PCM, 3 = IEEE float.
    const fn format_tag(self) -> u16 {
        match self {
            WavFormat::Pcm16 => 1,
            WavFormat::Float32 => 3,
        }
    }
}

/// Clamps a sample to [-1.0, 1.0] and converts to signed 16-bit.
///
/// This is the one place the export applies a defined clipping policy: the
/// mixer deliberately does not clamp (ADR-0004), so the final conversion to a
/// fixed-point format clamps here rather than wrapping.
fn to_i16(sample: Sample) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    // Symmetric scaling using 32767 avoids producing -32768 from -1.0 only via
    // rounding; standard and good enough for export.
    (clamped * i16::MAX as f32).round() as i16
}

/// Writes interleaved frames to `w` as a WAV file.
///
/// `channels` planar slices are expected in `planar` (each of length `frames`);
/// they are interleaved on the fly. All slices must have equal length.
///
/// # Errors
/// Returns any underlying I/O error. Returns [`io::ErrorKind::InvalidInput`] if
/// the planar channel lengths are inconsistent.
pub fn write_wav<W: Write>(
    mut w: W,
    planar: &[&[Sample]],
    sample_rate: u32,
    format: WavFormat,
) -> io::Result<()> {
    let channels = planar.len() as u16;
    let frames = planar.first().map_or(0, |c| c.len());
    if planar.iter().any(|c| c.len() != frames) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "all channels must have the same number of frames",
        ));
    }

    let bits = format.bits_per_sample();
    let bytes_per_sample = u32::from(bits) / 8;
    let block_align = u32::from(channels) * bytes_per_sample;
    let byte_rate = sample_rate * block_align;
    let data_bytes = frames as u32 * block_align;

    // RIFF header
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_bytes).to_le_bytes())?; // chunk size
    w.write_all(b"WAVE")?;

    // fmt chunk
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // PCM/float fmt chunk size
    w.write_all(&format.format_tag().to_le_bytes())?;
    w.write_all(&channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&(block_align as u16).to_le_bytes())?;
    w.write_all(&bits.to_le_bytes())?;

    // data chunk
    w.write_all(b"data")?;
    w.write_all(&data_bytes.to_le_bytes())?;

    for frame in 0..frames {
        for ch in planar {
            let s = ch[frame];
            match format {
                WavFormat::Pcm16 => w.write_all(&to_i16(s).to_le_bytes())?,
                WavFormat::Float32 => w.write_all(&s.to_le_bytes())?,
            }
        }
    }

    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i16_conversion_clamps() {
        assert_eq!(to_i16(2.0), i16::MAX);
        assert_eq!(to_i16(-2.0), -i16::MAX);
        assert_eq!(to_i16(0.0), 0);
    }

    #[test]
    fn writes_canonical_pcm16_header_and_data() {
        let left = [0.0f32, 1.0, -1.0, 0.5];
        let right = [0.0f32, -1.0, 1.0, -0.5];
        let mut out = Vec::new();
        write_wav(&mut out, &[&left, &right], 48_000, WavFormat::Pcm16).unwrap();

        assert_eq!(&out[0..4], b"RIFF");
        assert_eq!(&out[8..12], b"WAVE");
        assert_eq!(&out[12..16], b"fmt ");
        // format tag PCM = 1
        assert_eq!(u16::from_le_bytes([out[20], out[21]]), 1);
        // channels = 2
        assert_eq!(u16::from_le_bytes([out[22], out[23]]), 2);
        // sample rate
        assert_eq!(
            u32::from_le_bytes([out[24], out[25], out[26], out[27]]),
            48_000
        );
        // bits per sample = 16
        assert_eq!(u16::from_le_bytes([out[34], out[35]]), 16);
        // data chunk id
        assert_eq!(&out[36..40], b"data");
        // data size = 4 frames * 2 ch * 2 bytes = 16
        assert_eq!(u32::from_le_bytes([out[40], out[41], out[42], out[43]]), 16);
        // total length = 44 header + 16 data
        assert_eq!(out.len(), 60);
    }

    #[test]
    fn writes_float32_data_bit_exact() {
        let mono = [0.25f32, -0.75];
        let mut out = Vec::new();
        write_wav(&mut out, &[&mono], 44_100, WavFormat::Float32).unwrap();
        // format tag float = 3, bits = 32
        assert_eq!(u16::from_le_bytes([out[20], out[21]]), 3);
        assert_eq!(u16::from_le_bytes([out[34], out[35]]), 32);
        // first sample bytes
        let first = f32::from_le_bytes([out[44], out[45], out[46], out[47]]);
        assert_eq!(first, 0.25);
    }

    #[test]
    fn inconsistent_channel_lengths_error() {
        let a = [0.0f32, 1.0];
        let b = [0.0f32];
        let mut out = Vec::new();
        assert!(write_wav(&mut out, &[&a, &b], 48_000, WavFormat::Pcm16).is_err());
    }
}
