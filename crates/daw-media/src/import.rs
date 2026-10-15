//! Importing audio files into the project (see spec §12, ADR-0011).
//!
//! [`import_wav`] reads a WAV file, decodes it to planar `f32`, and returns a
//! [`MediaAssetRef`] describing the immutable source plus the decoded samples.
//! Decoded samples are held by a [`MemoryAssetSource`], which implements
//! [`daw_engine::schedule::AssetSource`] so clips referencing the asset can be
//! scheduled and rendered.
//!
//! Imported media is treated as immutable: the importer only reads.

use std::collections::HashMap;
use std::path::Path;

use thiserror::Error;

use daw_core::audio::Sample;
use daw_core::id::AssetId;
use daw_core::media::MediaAssetRef;
use daw_core::time::{SampleRate, SampleTime};
use daw_engine::schedule::AssetSource;

use crate::wav_reader::{decode_wav, DecodeError, DecodedAudio};

/// Errors produced while importing an audio file.
#[derive(Debug, Error)]
pub enum ImportError {
    /// The file could not be read.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The file contents could not be decoded.
    #[error("decode error: {0}")]
    Decode(#[from] DecodeError),
}

/// The result of importing a file: the asset reference plus decoded samples.
pub struct Imported {
    /// Reference to register in the project via `Project::add_asset`.
    pub asset: MediaAssetRef,
    /// Decoded planar samples for the asset.
    pub audio: DecodedAudio,
}

/// Imports a WAV file from `path`.
///
/// The returned [`MediaAssetRef`] records the original path and the decoded
/// source's rate/channels/frames. The caller adds it to the project and feeds
/// the decoded audio into a [`MemoryAssetSource`].
///
/// # Errors
/// Returns [`ImportError`] if the file cannot be read or decoded.
pub fn import_wav(path: impl AsRef<Path>) -> Result<Imported, ImportError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)?;
    let audio = decode_wav(&bytes)?;

    // SampleRate::new only fails on 0, which decode_wav already rejects.
    let rate = SampleRate::new(audio.sample_rate).unwrap_or_default();
    let asset = MediaAssetRef::new(
        path.to_string_lossy().into_owned(),
        rate,
        audio.layout,
        SampleTime::new(audio.frames() as u64),
    );
    Ok(Imported { asset, audio })
}

/// Imports a WAV file from `path`, resampling to `target_rate` if the source
/// rate differs. The returned [`MediaAssetRef`] records the **target** rate and
/// the resampled frame count, so the asset matches the project/engine rate.
///
/// # Errors
/// Returns [`ImportError`] if the file cannot be read or decoded.
pub fn import_wav_at(
    path: impl AsRef<Path>,
    target_rate: SampleRate,
) -> Result<Imported, ImportError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)?;
    let decoded = decode_wav(&bytes)?;
    let audio = crate::resample::resample_decoded(&decoded, target_rate.hz());

    let asset = MediaAssetRef::new(
        path.to_string_lossy().into_owned(),
        target_rate,
        audio.layout,
        SampleTime::new(audio.frames() as u64),
    );
    Ok(Imported { asset, audio })
}

/// An in-memory [`AssetSource`] holding decoded samples keyed by [`AssetId`].
///
/// This is the Milestone 1 source: the whole file is decoded into memory.
/// A future streaming/file-backed source can implement the same trait for large
/// media without changing the engine (ADR-0011).
#[derive(Default)]
pub struct MemoryAssetSource {
    assets: HashMap<AssetId, Vec<Vec<Sample>>>,
}

impl MemoryAssetSource {
    /// Creates an empty source.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers decoded audio for an asset id.
    pub fn insert(&mut self, id: AssetId, audio: DecodedAudio) {
        self.assets.insert(id, audio.channels);
    }

    /// Convenience: import a WAV, register it under its generated asset id, and
    /// return the [`MediaAssetRef`] to add to the project.
    ///
    /// # Errors
    /// Returns [`ImportError`] on read/decode failure.
    pub fn import_and_register(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<MediaAssetRef, ImportError> {
        let imported = import_wav(path)?;
        self.insert(imported.asset.id, imported.audio);
        Ok(imported.asset)
    }
}

impl AssetSource for MemoryAssetSource {
    fn read(&self, asset: AssetId, channel: usize, start: SampleTime, out: &mut [f32]) -> usize {
        let Some(channels) = self.assets.get(&asset) else {
            return 0;
        };
        // Mono sources feed every requested channel from channel 0.
        let ch = if channel < channels.len() { channel } else { 0 };
        let Some(samples) = channels.get(ch) else {
            return 0;
        };
        let start = start.frames() as usize;
        if start >= samples.len() {
            return 0;
        }
        let n = (samples.len() - start).min(out.len());
        out[..n].copy_from_slice(&samples[start..start + n]);
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_core::audio::ChannelLayout;

    fn stereo_audio(l: &[f32], r: &[f32]) -> DecodedAudio {
        DecodedAudio {
            channels: vec![l.to_vec(), r.to_vec()],
            sample_rate: 48_000,
            layout: ChannelLayout::Stereo,
        }
    }

    #[test]
    fn memory_source_reads_requested_channel() {
        let id = AssetId::new();
        let mut src = MemoryAssetSource::new();
        src.insert(id, stereo_audio(&[0.1, 0.2, 0.3], &[0.4, 0.5, 0.6]));

        let mut out = [0.0f32; 3];
        let n = src.read(id, 1, SampleTime::ZERO, &mut out);
        assert_eq!(n, 3);
        assert_eq!(out, [0.4, 0.5, 0.6]);
    }

    #[test]
    fn memory_source_respects_start_offset() {
        let id = AssetId::new();
        let mut src = MemoryAssetSource::new();
        src.insert(id, stereo_audio(&[1.0, 2.0, 3.0, 4.0], &[0.0; 4]));

        let mut out = [0.0f32; 10];
        let n = src.read(id, 0, SampleTime::new(2), &mut out);
        assert_eq!(n, 2); // only frames 2,3 remain
        assert_eq!(&out[..2], &[3.0, 4.0]);
    }

    #[test]
    fn mono_feeds_all_channels_from_channel_zero() {
        let id = AssetId::new();
        let mut src = MemoryAssetSource::new();
        src.insert(
            id,
            DecodedAudio {
                channels: vec![vec![0.7, 0.8]],
                sample_rate: 48_000,
                layout: ChannelLayout::Mono,
            },
        );
        let mut out = [0.0f32; 2];
        // Request channel 1 (right) from a mono source -> uses channel 0.
        let n = src.read(id, 1, SampleTime::ZERO, &mut out);
        assert_eq!(n, 2);
        assert_eq!(out, [0.7, 0.8]);
    }

    #[test]
    fn unknown_asset_reads_nothing() {
        let src = MemoryAssetSource::new();
        let mut out = [0.0f32; 4];
        assert_eq!(src.read(AssetId::new(), 0, SampleTime::ZERO, &mut out), 0);
    }
}
