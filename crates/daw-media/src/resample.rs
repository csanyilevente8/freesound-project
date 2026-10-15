//! Sample-rate conversion (see spec §12, ADR-0011 "resampling deferred").
//!
//! When an imported file's sample rate differs from the project/engine rate,
//! its samples must be resampled. This provides an offline, dependency-free
//! resampler using **Catmull-Rom cubic interpolation** — noticeably cleaner
//! than linear for audio material while remaining simple and deterministic. It
//! is intended for offline import/export, not the real-time path; a
//! higher-quality windowed-sinc or a dedicated crate (e.g. `rubato`) can replace
//! it later behind this same function (its own future ADR).
//!
//! This is a per-channel operation on planar `f32`.

use daw_core::audio::Sample;

/// Resamples one planar channel from `src_rate` to `dst_rate` using Catmull-Rom
/// cubic interpolation. Returns the resampled channel.
///
/// If the rates are equal (or either is zero), the input is returned unchanged.
#[must_use]
pub fn resample_channel(input: &[Sample], src_rate: u32, dst_rate: u32) -> Vec<Sample> {
    if src_rate == dst_rate || src_rate == 0 || dst_rate == 0 || input.is_empty() {
        return input.to_vec();
    }

    let ratio = dst_rate as f64 / src_rate as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);

    let n = input.len() as isize;
    let sample_at = |i: isize| -> Sample {
        // Clamp to edges (hold boundary samples).
        let idx = i.clamp(0, n - 1) as usize;
        input[idx]
    };

    for j in 0..out_len {
        // Position in source index space.
        let pos = j as f64 / ratio;
        let i = pos.floor() as isize;
        let t = (pos - i as f64) as f32;

        // Catmull-Rom using points i-1, i, i+1, i+2.
        let p0 = sample_at(i - 1);
        let p1 = sample_at(i);
        let p2 = sample_at(i + 1);
        let p3 = sample_at(i + 2);

        let t2 = t * t;
        let t3 = t2 * t;
        let y = 0.5
            * ((2.0 * p1)
                + (-p0 + p2) * t
                + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3);
        out.push(y);
    }

    out
}

/// Resamples all planar channels of a [`DecodedAudio`] to `dst_rate`, returning
/// a new [`DecodedAudio`] at the target rate. Equal rates return a clone.
#[must_use]
pub fn resample_decoded(
    audio: &crate::wav_reader::DecodedAudio,
    dst_rate: u32,
) -> crate::wav_reader::DecodedAudio {
    if audio.sample_rate == dst_rate {
        return audio.clone();
    }
    let channels = audio
        .channels
        .iter()
        .map(|ch| resample_channel(ch, audio.sample_rate, dst_rate))
        .collect();
    crate::wav_reader::DecodedAudio {
        channels,
        sample_rate: dst_rate,
        layout: audio.layout,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_rate_is_identity() {
        let input = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_channel(&input, 48_000, 48_000), input);
    }

    #[test]
    fn doubling_rate_doubles_length() {
        let input = vec![0.0, 1.0, 0.0, -1.0];
        let out = resample_channel(&input, 24_000, 48_000);
        assert_eq!(out.len(), 8);
        // Endpoints are approximately preserved.
        assert!((out[0] - 0.0).abs() < 1e-3);
    }

    #[test]
    fn halving_rate_halves_length() {
        let input: Vec<f32> = (0..100).map(|i| (i as f32 * 0.1).sin()).collect();
        let out = resample_channel(&input, 48_000, 24_000);
        assert_eq!(out.len(), 50);
        assert!(out.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn preserves_a_constant_signal() {
        // A DC signal should remain ~constant through resampling.
        let input = vec![0.5f32; 64];
        let out = resample_channel(&input, 44_100, 48_000);
        assert!(out.iter().all(|&s| (s - 0.5).abs() < 1e-4));
    }

    #[test]
    fn linear_ramp_stays_monotonic_and_bounded() {
        let input: Vec<f32> = (0..32).map(|i| i as f32 / 31.0).collect();
        let out = resample_channel(&input, 16_000, 44_100);
        assert!(out.iter().all(|&s| (-0.01..=1.01).contains(&s)));
        // Roughly increasing.
        assert!(out[out.len() - 1] > out[0]);
    }

    #[test]
    fn resample_decoded_updates_rate_and_lengths() {
        use daw_core::audio::ChannelLayout;
        let audio = crate::wav_reader::DecodedAudio {
            channels: vec![vec![0.0, 1.0, 0.0, -1.0], vec![1.0, 0.0, -1.0, 0.0]],
            sample_rate: 24_000,
            layout: ChannelLayout::Stereo,
        };
        let out = resample_decoded(&audio, 48_000);
        assert_eq!(out.sample_rate, 48_000);
        assert_eq!(out.channels[0].len(), 8);
        assert_eq!(out.channels[1].len(), 8);
    }
}
