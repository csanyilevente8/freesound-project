//! Hand-written canonical WAV reader (see ADR-0011).
//!
//! Decodes little-endian RIFF/WAVE containing PCM 16-bit or IEEE float 32-bit
//! samples, mono or stereo, into planar `f32`. This mirrors the writer in
//! `daw-render` so a WAV we export can be re-imported exactly. Unsupported or
//! malformed files produce a typed [`DecodeError`] rather than panicking.
//!
//! The parser walks RIFF chunks (so it tolerates extra chunks such as `LIST`
//! or `fact` before `data`) but only understands `fmt ` tags 1 (PCM) and
//! 3 (IEEE float). It does not handle `WAVE_FORMAT_EXTENSIBLE` (tag 0xFFFE).

use thiserror::Error;

use daw_core::audio::{ChannelLayout, Sample};

/// Errors produced while decoding a WAV byte stream.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecodeError {
    /// The data is too short or a chunk runs past the end of the buffer.
    #[error("truncated or malformed WAV data")]
    Truncated,
    /// Missing RIFF/WAVE magic.
    #[error("not a RIFF/WAVE file")]
    NotWave,
    /// A required chunk (`fmt ` or `data`) was absent.
    #[error("missing {0} chunk")]
    MissingChunk(&'static str),
    /// The format tag / bit depth combination is not supported.
    #[error("unsupported WAV format (tag {tag}, {bits} bits)")]
    UnsupportedFormat {
        /// WAVE format tag found.
        tag: u16,
        /// Bits per sample found.
        bits: u16,
    },
    /// The channel count is not supported (only mono/stereo in M1).
    #[error("unsupported channel count {0}")]
    UnsupportedChannels(u16),
    /// Sample rate was zero.
    #[error("invalid sample rate 0")]
    ZeroSampleRate,
}

/// A decoded WAV: planar `f32` channels plus the source metadata needed to
/// build a `MediaAssetRef`.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedAudio {
    /// One `Vec<f32>` per channel, all the same length (`frames`).
    pub channels: Vec<Vec<Sample>>,
    /// Source sample rate in Hz.
    pub sample_rate: u32,
    /// Channel layout.
    pub layout: ChannelLayout,
}

impl DecodedAudio {
    /// Number of frames per channel.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }
}

fn read_u32(d: &[u8], at: usize) -> Result<u32, DecodeError> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(DecodeError::Truncated)
}

fn read_u16(d: &[u8], at: usize) -> Result<u16, DecodeError> {
    d.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(DecodeError::Truncated)
}

/// Decodes a canonical WAV byte stream into planar `f32`.
///
/// # Errors
/// Returns [`DecodeError`] for malformed, truncated, or unsupported input.
pub fn decode_wav(data: &[u8]) -> Result<DecodedAudio, DecodeError> {
    if data.len() < 12 {
        return Err(DecodeError::Truncated);
    }
    if &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(DecodeError::NotWave);
    }

    // Walk chunks starting at offset 12.
    let mut pos = 12usize;
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // (tag, channels, rate, bits)
    let mut data_range: Option<(usize, usize)> = None;

    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = read_u32(data, pos + 4)? as usize;
        let body = pos + 8;
        let end = body.checked_add(size).ok_or(DecodeError::Truncated)?;
        if end > data.len() {
            return Err(DecodeError::Truncated);
        }

        if id == b"fmt " {
            if size < 16 {
                return Err(DecodeError::Truncated);
            }
            let tag = read_u16(data, body)?;
            let channels = read_u16(data, body + 2)?;
            let rate = read_u32(data, body + 4)?;
            let bits = read_u16(data, body + 14)?;
            fmt = Some((tag, channels, rate, bits));
        } else if id == b"data" {
            data_range = Some((body, end));
        }

        // Chunks are word-aligned: skip a pad byte if size is odd.
        pos = end + (size & 1);
    }

    let (tag, channels, rate, bits) = fmt.ok_or(DecodeError::MissingChunk("fmt "))?;
    let (dstart, dend) = data_range.ok_or(DecodeError::MissingChunk("data"))?;

    if rate == 0 {
        return Err(DecodeError::ZeroSampleRate);
    }
    let layout = match channels {
        1 => ChannelLayout::Mono,
        2 => ChannelLayout::Stereo,
        n => return Err(DecodeError::UnsupportedChannels(n)),
    };

    let body = &data[dstart..dend];
    let samples: Vec<Sample> = match (tag, bits) {
        (1, 16) => {
            if body.len() % 2 != 0 {
                return Err(DecodeError::Truncated);
            }
            body.chunks_exact(2)
                .map(|b| {
                    let v = i16::from_le_bytes([b[0], b[1]]);
                    v as f32 / i16::MAX as f32
                })
                .collect()
        }
        (3, 32) => {
            if body.len() % 4 != 0 {
                return Err(DecodeError::Truncated);
            }
            body.chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .collect()
        }
        _ => return Err(DecodeError::UnsupportedFormat { tag, bits }),
    };

    // De-interleave into planar channels.
    let nch = channels as usize;
    let frames = samples.len() / nch;
    let mut planar: Vec<Vec<Sample>> = vec![Vec::with_capacity(frames); nch];
    for frame in samples.chunks_exact(nch) {
        for (ch, &s) in frame.iter().enumerate() {
            planar[ch].push(s);
        }
    }

    Ok(DecodedAudio {
        channels: planar,
        sample_rate: rate,
        layout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build a minimal canonical PCM16 stereo WAV in memory.
    fn pcm16_stereo(frames: &[(i16, i16)], rate: u32) -> Vec<u8> {
        let data_bytes = (frames.len() * 2 * 2) as u32;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        v.extend_from_slice(b"WAVE");
        v.extend_from_slice(b"fmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes()); // PCM
        v.extend_from_slice(&2u16.to_le_bytes()); // channels
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * 4).to_le_bytes()); // byte rate
        v.extend_from_slice(&4u16.to_le_bytes()); // block align
        v.extend_from_slice(&16u16.to_le_bytes()); // bits
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data_bytes.to_le_bytes());
        for (l, r) in frames {
            v.extend_from_slice(&l.to_le_bytes());
            v.extend_from_slice(&r.to_le_bytes());
        }
        v
    }

    #[test]
    fn decodes_pcm16_stereo() {
        let wav = pcm16_stereo(&[(i16::MAX, 0), (0, i16::MIN + 1)], 44_100);
        let d = decode_wav(&wav).unwrap();
        assert_eq!(d.sample_rate, 44_100);
        assert_eq!(d.layout, ChannelLayout::Stereo);
        assert_eq!(d.frames(), 2);
        assert!((d.channels[0][0] - 1.0).abs() < 1e-4);
        assert!((d.channels[1][1] + 1.0).abs() < 1e-3);
    }

    #[test]
    fn tolerates_extra_chunk_before_data() {
        let mut wav = pcm16_stereo(&[(100, -100)], 48_000);
        // Insert a LIST chunk after fmt (before data) by rebuilding.
        // Simpler: splice a fake chunk right after the 36-byte header prefix.
        let insert_at = 36; // start of "data"
        let mut chunk = Vec::new();
        chunk.extend_from_slice(b"LIST");
        chunk.extend_from_slice(&4u32.to_le_bytes());
        chunk.extend_from_slice(b"INFO");
        wav.splice(insert_at..insert_at, chunk);
        // Fix RIFF size.
        let new_size = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&new_size.to_le_bytes());

        let d = decode_wav(&wav).unwrap();
        assert_eq!(d.frames(), 1);
    }

    #[test]
    fn rejects_non_wave() {
        assert_eq!(
            decode_wav(b"not a wave file!!").unwrap_err(),
            DecodeError::NotWave
        );
    }

    #[test]
    fn rejects_truncated() {
        assert_eq!(decode_wav(b"RIFF").unwrap_err(), DecodeError::Truncated);
    }

    #[test]
    fn rejects_unsupported_format() {
        // 8-bit PCM (tag 1, bits 8) is not supported.
        let mut wav = pcm16_stereo(&[(0, 0)], 48_000);
        wav[34] = 8; // bits per sample low byte
        assert!(matches!(
            decode_wav(&wav),
            Err(DecodeError::UnsupportedFormat { bits: 8, .. })
        ));
    }
}
