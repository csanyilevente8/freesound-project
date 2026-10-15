//! Compressor (see spec §10.2 item 9).
//!
//! A feed-forward peak compressor with threshold, ratio, attack, release, and
//! makeup gain. The detector tracks the peak of the (summed) input with
//! separate attack/release time constants; the gain-reduction is computed from
//! a hard-knee static curve in the dB domain and applied equally to all
//! channels (so stereo imaging is preserved). The most recent gain reduction is
//! exposed via [`gain_reduction_db`](CompressorProcessor::gain_reduction_db)
//! for metering (spec: "expose reduction metering when practical").
//!
//! Buffers/state are preallocated in [`prepare`](AudioProcessor::prepare);
//! [`process`](AudioProcessor::process) is allocation-free.

use crate::buffer::AudioBuffer;
use crate::processor::{AudioProcessor, ProcessConfig};

/// A peak feed-forward compressor.
#[derive(Debug, Clone)]
pub struct CompressorProcessor {
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    makeup_db: f32,
    sample_rate: f32,
    // Smoothing coefficients derived from attack/release.
    attack_coeff: f32,
    release_coeff: f32,
    // Detector envelope (linear) and last gain reduction (dB, negative = cut).
    envelope: f32,
    last_reduction_db: f32,
}

impl CompressorProcessor {
    /// Creates a compressor. `threshold_db` (e.g. -18), `ratio` ≥ 1
    /// (e.g. 4 = 4:1), `attack_ms`/`release_ms` > 0, `makeup_db` post-gain.
    #[must_use]
    pub fn new(
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        makeup_db: f32,
    ) -> Self {
        let mut c = Self {
            threshold_db,
            ratio: ratio.max(1.0),
            attack_ms: attack_ms.max(0.01),
            release_ms: release_ms.max(0.01),
            makeup_db,
            sample_rate: 48_000.0,
            attack_coeff: 0.0,
            release_coeff: 0.0,
            envelope: 0.0,
            last_reduction_db: 0.0,
        };
        c.update_coeffs();
        c
    }

    /// The most recent gain reduction in dB (≤ 0; 0 = no compression).
    #[must_use]
    pub fn gain_reduction_db(&self) -> f32 {
        self.last_reduction_db
    }

    /// Sets the threshold in dB.
    pub fn set_threshold_db(&mut self, db: f32) {
        self.threshold_db = db;
    }

    /// Sets the ratio (≥ 1).
    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio.max(1.0);
    }

    fn update_coeffs(&mut self) {
        // One-pole smoothing coefficient for a given time constant.
        let coeff = |ms: f32, sr: f32| (-1.0 / ((ms / 1000.0) * sr)).exp();
        self.attack_coeff = coeff(self.attack_ms, self.sample_rate);
        self.release_coeff = coeff(self.release_ms, self.sample_rate);
    }

    #[inline]
    fn lin_to_db(x: f32) -> f32 {
        if x <= 1e-9 {
            -180.0
        } else {
            20.0 * x.log10()
        }
    }

    #[inline]
    fn db_to_lin(db: f32) -> f32 {
        10.0_f32.powf(db / 20.0)
    }
}

impl AudioProcessor for CompressorProcessor {
    fn prepare(&mut self, config: &ProcessConfig) {
        self.sample_rate = config.sample_rate.hz() as f32;
        self.envelope = 0.0;
        self.last_reduction_db = 0.0;
        self.update_coeffs();
    }

    fn process(&mut self, buffer: &mut AudioBuffer) {
        let channels = buffer.channels();
        if channels == 0 {
            return;
        }
        let frames = buffer.frames();
        let makeup = Self::db_to_lin(self.makeup_db);
        let mut min_reduction_db = 0.0_f32;

        for i in 0..frames {
            // Detector input: peak across channels at this frame.
            let mut peak = 0.0_f32;
            for ch in 0..channels {
                peak = peak.max(buffer.channel(ch)[i].abs());
            }

            // Envelope follower with attack/release.
            let coeff = if peak > self.envelope {
                self.attack_coeff
            } else {
                self.release_coeff
            };
            self.envelope = coeff * self.envelope + (1.0 - coeff) * peak;

            // Static gain curve (hard knee) in dB.
            let env_db = Self::lin_to_db(self.envelope);
            let reduction_db = if env_db > self.threshold_db {
                let over = env_db - self.threshold_db;
                -(over - over / self.ratio) // negative = attenuation
            } else {
                0.0
            };
            min_reduction_db = min_reduction_db.min(reduction_db);

            let gain = Self::db_to_lin(reduction_db) * makeup;
            for ch in 0..channels {
                buffer.channel_mut(ch)[i] *= gain;
            }
        }

        self.last_reduction_db = min_reduction_db;
    }

    fn reset(&mut self) {
        self.envelope = 0.0;
        self.last_reduction_db = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(ch: usize) -> ProcessConfig {
        ProcessConfig {
            sample_rate: daw_core::time::SampleRate::new(48_000).unwrap(),
            max_block_frames: 8192,
            channels: ch,
        }
    }

    fn rms(b: &AudioBuffer, ch: usize) -> f32 {
        let s = b.channel(ch);
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    #[test]
    fn below_threshold_passes_unchanged() {
        // Threshold -6 dB; a quiet -20 dB signal should be untouched (no makeup).
        let mut c = CompressorProcessor::new(-6.0, 4.0, 1.0, 50.0, 0.0);
        c.prepare(&config(1));
        let level = CompressorProcessor::db_to_lin(-20.0);
        let mut b = AudioBuffer::new(1, 1000);
        b.channel_mut(0).fill(level);
        c.process(&mut b);
        assert!((rms(&b, 0) - level).abs() < 1e-3);
        assert!(c.gain_reduction_db() > -0.5);
    }

    #[test]
    fn above_threshold_is_attenuated() {
        // Loud signal well above threshold gets compressed.
        let mut c = CompressorProcessor::new(-12.0, 4.0, 0.1, 50.0, 0.0);
        c.prepare(&config(1));
        let loud = CompressorProcessor::db_to_lin(0.0); // 1.0, 12 dB over
        let mut b = AudioBuffer::new(1, 4000);
        b.channel_mut(0).fill(loud);
        c.process(&mut b);
        // Steady-state output should be below the input level.
        assert!(b.channel(0)[3999].abs() < loud);
        // And gain reduction should be reported (negative dB).
        assert!(c.gain_reduction_db() < -1.0);
    }

    #[test]
    fn makeup_gain_boosts_output() {
        let mut c = CompressorProcessor::new(-100.0, 1.0, 1.0, 1.0, 6.0); // ratio 1 => only makeup
        c.prepare(&config(1));
        let mut b = AudioBuffer::new(1, 100);
        b.channel_mut(0).fill(0.1);
        c.process(&mut b);
        // +6 dB ≈ x2.
        assert!((b.channel(0)[99].abs() - 0.2).abs() < 0.02);
    }

    #[test]
    fn output_is_finite_and_resettable() {
        let mut c = CompressorProcessor::new(-18.0, 8.0, 1.0, 100.0, 3.0);
        c.prepare(&config(2));
        let mut b = AudioBuffer::new(2, 2000);
        for ch in 0..2 {
            b.channel_mut(ch).fill(0.9);
        }
        c.process(&mut b);
        assert!(b.channel(0).iter().all(|x| x.is_finite()));
        c.reset();
        assert_eq!(c.gain_reduction_db(), 0.0);
    }
}
