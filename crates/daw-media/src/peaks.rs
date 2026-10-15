//! Waveform peak data (see spec §6.1 "peaks", §12).
//!
//! Computes a compact min/max envelope of audio for drawing a waveform. This is
//! the **data** only — rendering it is a UI concern (deferred). Peaks are a
//! rebuildable cache, never authoritative audio (spec §6.2, §12): losing them
//! must never affect the source, so this is a pure function over samples.
//!
//! A peak file bins the signal into fixed-size windows and stores the minimum
//! and maximum sample in each window, which is what a waveform view draws as a
//! vertical bar per pixel column.

use daw_core::audio::Sample;

/// One bucket of the waveform envelope: the min and max sample in a window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Peak {
    /// Minimum sample value in the window.
    pub min: Sample,
    /// Maximum sample value in the window.
    pub max: Sample,
}

/// A per-channel waveform envelope at a fixed window size (samples per bucket).
#[derive(Debug, Clone, PartialEq)]
pub struct WaveformPeaks {
    /// Samples per bucket this envelope was computed at.
    pub samples_per_bucket: usize,
    /// One `Vec<Peak>` per channel.
    pub channels: Vec<Vec<Peak>>,
}

impl WaveformPeaks {
    /// Number of buckets per channel (equal for all channels).
    #[must_use]
    pub fn buckets(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }
}

/// Computes a min/max waveform envelope for planar channels.
///
/// `samples_per_bucket` is clamped to at least 1. Each bucket covers
/// `samples_per_bucket` consecutive frames; a trailing partial bucket is
/// included. Empty input yields empty channels.
#[must_use]
pub fn compute_peaks(channels: &[Vec<Sample>], samples_per_bucket: usize) -> WaveformPeaks {
    let spb = samples_per_bucket.max(1);
    let out = channels
        .iter()
        .map(|ch| {
            ch.chunks(spb)
                .map(|window| {
                    let mut min = Sample::INFINITY;
                    let mut max = Sample::NEG_INFINITY;
                    for &s in window {
                        min = min.min(s);
                        max = max.max(s);
                    }
                    if window.is_empty() {
                        Peak { min: 0.0, max: 0.0 }
                    } else {
                        Peak { min, max }
                    }
                })
                .collect()
        })
        .collect();
    WaveformPeaks {
        samples_per_bucket: spb,
        channels: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_count_matches_input() {
        let ch = vec![vec![0.0; 1000]];
        let p = compute_peaks(&ch, 100);
        assert_eq!(p.buckets(), 10);
        assert_eq!(p.samples_per_bucket, 100);
    }

    #[test]
    fn trailing_partial_bucket_included() {
        let ch = vec![vec![0.0; 250]];
        let p = compute_peaks(&ch, 100);
        assert_eq!(p.buckets(), 3); // 100, 100, 50
    }

    #[test]
    fn captures_min_and_max() {
        let ch = vec![vec![-0.5, 0.9, 0.1, -0.2]];
        let p = compute_peaks(&ch, 4);
        assert_eq!(
            p.channels[0][0],
            Peak {
                min: -0.5,
                max: 0.9
            }
        );
    }

    #[test]
    fn per_channel_envelopes() {
        let channels = vec![vec![1.0, -1.0], vec![0.5, 0.25]];
        let p = compute_peaks(&channels, 2);
        assert_eq!(p.channels.len(), 2);
        assert_eq!(
            p.channels[0][0],
            Peak {
                min: -1.0,
                max: 1.0
            }
        );
        assert_eq!(
            p.channels[1][0],
            Peak {
                min: 0.25,
                max: 0.5
            }
        );
    }

    #[test]
    fn zero_bucket_size_is_clamped() {
        let ch = vec![vec![0.1, 0.2]];
        let p = compute_peaks(&ch, 0);
        assert_eq!(p.samples_per_bucket, 1);
        assert_eq!(p.buckets(), 2);
    }

    #[test]
    fn empty_input_yields_empty() {
        let p = compute_peaks(&[vec![]], 100);
        assert_eq!(p.buckets(), 0);
    }
}
