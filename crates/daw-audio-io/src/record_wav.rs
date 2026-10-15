//! Streaming WAV writer for recording (see spec §9.7).
//!
//! Unlike the whole-buffer exporter in `daw-render`, recording does not know
//! the length up front, so this writer streams frames and patches the RIFF /
//! data chunk sizes on [`finalize`](WavStreamWriter::finalize). It writes
//! **interleaved IEEE float32** (lossless of the engine's `f32` capture; no
//! quantization), which keeps the writer-thread work minimal.
//!
//! A [`RecordSink`] trait abstracts the destination so tests can inject a fake
//! or a deliberately-failing sink (simulating a full/slow disk) without touching
//! the filesystem — satisfying the spec's "test slow/failed storage" requirement.

use std::fs::File;
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use daw_core::audio::Sample;

/// A destination for recorded audio frames. Implementors receive interleaved
/// `f32` blocks and a final flush.
pub trait RecordSink {
    /// Appends `interleaved` samples (L,R,L,R,… for stereo). Returns an error on
    /// write failure (e.g. disk full) — the writer thread will stop and report.
    fn write_block(&mut self, interleaved: &[Sample]) -> io::Result<()>;

    /// Finalizes the destination (patch headers, flush, sync). Called once.
    fn finalize(&mut self) -> io::Result<()>;
}

/// A streaming float32 WAV writer over any `Write + Seek` (e.g. a file).
pub struct WavStreamWriter<W: Write + Seek> {
    inner: W,
    channels: u16,
    frames_written: u64,
    finalized: bool,
}

impl<W: Write + Seek> WavStreamWriter<W> {
    /// Creates a writer and emits a placeholder header. `channels` and
    /// `sample_rate` describe the stream; data is float32.
    ///
    /// # Errors
    /// Returns any I/O error writing the initial header.
    pub fn new(mut inner: W, channels: u16, sample_rate: u32) -> io::Result<Self> {
        write_header(&mut inner, channels, sample_rate, 0)?;
        Ok(Self {
            inner,
            channels,
            frames_written: 0,
            finalized: false,
        })
    }

    /// Number of frames written so far.
    #[must_use]
    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }
}

/// Writes the 44-byte canonical float32 WAV header with the given data length.
fn write_header<W: Write>(
    w: &mut W,
    channels: u16,
    sample_rate: u32,
    data_bytes: u32,
) -> io::Result<()> {
    let bits = 32u16;
    let block_align = u32::from(channels) * u32::from(bits) / 8;
    let byte_rate = sample_rate * block_align;
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_bytes).to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&3u16.to_le_bytes())?; // IEEE float
    w.write_all(&channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&(block_align as u16).to_le_bytes())?;
    w.write_all(&bits.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_bytes.to_le_bytes())?;
    Ok(())
}

impl<W: Write + Seek> RecordSink for WavStreamWriter<W> {
    fn write_block(&mut self, interleaved: &[Sample]) -> io::Result<()> {
        for &s in interleaved {
            self.inner.write_all(&s.to_le_bytes())?;
        }
        if self.channels > 0 {
            self.frames_written += (interleaved.len() / self.channels as usize) as u64;
        }
        Ok(())
    }

    fn finalize(&mut self) -> io::Result<()> {
        if self.finalized {
            return Ok(());
        }
        self.finalized = true;
        let data_bytes = (self.frames_written * u64::from(self.channels) * 4) as u32;
        // Patch RIFF size (offset 4) and data size (offset 40).
        self.inner.seek(SeekFrom::Start(4))?;
        self.inner.write_all(&(36 + data_bytes).to_le_bytes())?;
        self.inner.seek(SeekFrom::Start(40))?;
        self.inner.write_all(&data_bytes.to_le_bytes())?;
        self.inner.flush()
    }
}

/// Convenience: a file-backed streaming WAV writer.
///
/// # Errors
/// Returns an I/O error if the file cannot be created or the header written.
pub fn create_wav_file(
    path: impl AsRef<Path>,
    channels: u16,
    sample_rate: u32,
) -> io::Result<WavStreamWriter<BufWriter<File>>> {
    if let Some(dir) = path.as_ref().parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let file = BufWriter::new(File::create(path)?);
    WavStreamWriter::new(file, channels, sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn streams_and_patches_header() {
        let buf = Cursor::new(Vec::new());
        let mut w = WavStreamWriter::new(buf, 2, 48_000).unwrap();
        // 3 stereo frames.
        w.write_block(&[0.1, -0.1, 0.2, -0.2, 0.3, -0.3]).unwrap();
        assert_eq!(w.frames_written(), 3);
        w.finalize().unwrap();

        let bytes = w.inner.into_inner();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        // data size patched = 3 frames * 2 ch * 4 bytes = 24
        let data_size = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);
        assert_eq!(data_size, 24);
        // riff size = 36 + 24
        let riff = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        assert_eq!(riff, 60);
        assert_eq!(bytes.len(), 44 + 24);
        // first sample round-trips
        let s0 = f32::from_le_bytes([bytes[44], bytes[45], bytes[46], bytes[47]]);
        assert!((s0 - 0.1).abs() < 1e-6);
    }

    #[test]
    fn finalize_is_idempotent() {
        let buf = Cursor::new(Vec::new());
        let mut w = WavStreamWriter::new(buf, 1, 44_100).unwrap();
        w.write_block(&[0.5, 0.6]).unwrap();
        w.finalize().unwrap();
        w.finalize().unwrap(); // no-op, no error
        assert_eq!(w.frames_written(), 2);
    }
}
