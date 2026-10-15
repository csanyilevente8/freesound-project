//! Media asset references (see spec §5.3, §6, §12).
//!
//! A [`MediaAssetRef`] is a *reference* to source audio, not the audio itself.
//! The project stores references; the actual samples live in files on disk and
//! are loaded/decoded by the engine without blocking the real-time callback
//! (deferred to the device-playback slice). Keeping imported source media
//! immutable is a core principle (spec §12): moving/trimming a clip must never
//! rewrite the source.
//!
//! The path is stored as a plain string so the project format carries no
//! platform-specific `PathBuf` representation; path resolution and relink
//! workflows are an application-layer concern.

use serde::{Deserialize, Serialize};

use crate::audio::ChannelLayout;
use crate::id::AssetId;
use crate::time::{SampleRate, SampleTime};
use crate::validate::{Validate, ValidationError};

/// A reference to an immutable source-audio asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaAssetRef {
    /// Stable identifier used by clips to reference this asset.
    pub id: AssetId,
    /// Location of the source media, relative to the project or absolute.
    /// Stored as a string to keep the format platform-independent.
    pub path: String,
    /// Sample rate of the source media as imported.
    pub sample_rate: SampleRate,
    /// Channel layout of the source media.
    pub channels: ChannelLayout,
    /// Total length of the source media in sample frames.
    pub frames: SampleTime,
}

impl MediaAssetRef {
    /// Creates a new asset reference with a freshly generated id.
    #[must_use]
    pub fn new(
        path: impl Into<String>,
        sample_rate: SampleRate,
        channels: ChannelLayout,
        frames: SampleTime,
    ) -> Self {
        Self {
            id: AssetId::new(),
            path: path.into(),
            sample_rate,
            channels,
            frames,
        }
    }
}

impl Validate for MediaAssetRef {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.path.trim().is_empty() {
            return Err(ValidationError::new("media asset path must not be empty"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(frames: u64) -> MediaAssetRef {
        MediaAssetRef::new(
            "media/audio/take1.wav",
            SampleRate::new(48_000).unwrap(),
            ChannelLayout::Stereo,
            SampleTime::new(frames),
        )
    }

    #[test]
    fn valid_asset_passes() {
        assert!(asset(1000).validate().is_ok());
    }

    #[test]
    fn empty_path_is_invalid() {
        let mut a = asset(1000);
        a.path = "   ".into();
        assert!(a.validate().is_err());
    }
}
