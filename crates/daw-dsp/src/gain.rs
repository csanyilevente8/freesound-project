//! Gain processor (see spec §10.2 item 1).
//!
//! Applies a linear gain to every sample. Supports a target gain with simple
//! per-sample linear smoothing to avoid zipper noise on parameter changes
//! (spec §10.4). Smoothing frame counts are resolved in `prepare` from the
//! actual sample rate; `process` is allocation- and lock-free.

use crate::buffer::AudioBuffer;
use crate::processor::{AudioProcessor, ProcessConfig};
use daw_core::audio::{Gain, Sample};

/// A linear gain processor with optional linear smoothing of gain changes.
#[derive(Debug, Clone)]
pub struct GainProcessor {
    /// Target gain the processor is moving toward.
    target: Sample,
    /// Current (possibly mid-smoothing) gain actually applied.
    current: Sample,
    /// Per-sample increment used while smoothing. 0 means "no ramp in flight".
    step: Sample,
    /// Configured smoothing time in milliseconds.
    smoothing_ms: f32,
    /// Smoothing window in frames, resolved from the sample rate in `prepare`.
    smoothing_frames: usize,
}

impl GainProcessor {
    /// Creates a gain processor at the given initial gain with no smoothing.
    #[must_use]
    pub fn new(gain: Gain) -> Self {
        Self {
            target: gain.factor(),
            current: gain.factor(),
            step: 0.0,
            smoothing_ms: 0.0,
            smoothing_frames: 0,
        }
    }

    /// Sets the smoothing time in milliseconds applied to future gain changes.
    /// The frame count is resolved in [`AudioProcessor::prepare`].
    #[must_use]
    pub fn with_smoothing_ms(mut self, ms: f32) -> Self {
        self.smoothing_ms = ms.max(0.0);
        self
    }

    /// Requests a new target gain. The change is applied over the configured
    /// smoothing window (or immediately if smoothing is zero / not prepared).
    pub fn set_gain(&mut self, gain: Gain) {
        self.target = gain.factor();
        if self.smoothing_frames == 0 {
            self.current = self.target;
            self.step = 0.0;
        } else {
            self.step = (self.target - self.current) / self.smoothing_frames as Sample;
        }
    }

    /// Returns the currently applied gain factor (useful for tests/metering).
    #[must_use]
    pub fn current_factor(&self) -> Sample {
        self.current
    }
}

impl AudioProcessor for GainProcessor {
    fn prepare(&mut self, config: &ProcessConfig) {
        self.smoothing_frames =
            ((self.smoothing_ms / 1000.0) * config.sample_rate.hz() as f32).round() as usize;
        // If a change was requested before prepare, (re)compute the ramp now.
        if self.smoothing_frames > 0 && (self.target - self.current).abs() > Sample::EPSILON {
            self.step = (self.target - self.current) / self.smoothing_frames as Sample;
        } else if (self.target - self.current).abs() <= Sample::EPSILON {
            self.step = 0.0;
        }
    }

    fn process(&mut self, buffer: &mut AudioBuffer) {
        let frames = buffer.frames();
        let channels = buffer.channels();

        if self.step == 0.0 {
            let g = self.current;
            if (g - 1.0).abs() <= Sample::EPSILON {
                return; // unity gain: nothing to do
            }
            for ch in 0..channels {
                for s in buffer.channel_mut(ch).iter_mut() {
                    *s *= g;
                }
            }
            return;
        }

        // Smoothing in flight: apply the same per-frame ramp value across
        // every channel so channels stay phase-aligned.
        let start = self.current;
        let step = self.step;
        let target = self.target;
        for ch in 0..channels {
            let mut g = start;
            for s in buffer.channel_mut(ch).iter_mut() {
                *s *= g;
                g += step;
                if (step > 0.0 && g >= target) || (step < 0.0 && g <= target) {
                    g = target;
                }
            }
        }

        // Advance the shared smoothing state by `frames` and settle if done.
        let mut g = self.current + step * frames as Sample;
        if (step > 0.0 && g >= target) || (step < 0.0 && g <= target) {
            g = target;
            self.step = 0.0;
        }
        self.current = g;
    }

    fn reset(&mut self) {
        self.current = self.target;
        self.step = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ProcessConfig {
        ProcessConfig {
            sample_rate: daw_core::time::SampleRate::new(48_000).unwrap(),
            max_block_frames: 512,
            channels: 2,
        }
    }

    #[test]
    fn unity_gain_is_identity() {
        let mut p = GainProcessor::new(Gain::UNITY);
        p.prepare(&config());
        let mut buf = AudioBuffer::new(2, 4);
        buf.channel_mut(0).fill(0.5);
        buf.channel_mut(1).fill(-0.5);
        p.process(&mut buf);
        assert_eq!(buf.channel(0), &[0.5, 0.5, 0.5, 0.5]);
        assert_eq!(buf.channel(1), &[-0.5, -0.5, -0.5, -0.5]);
    }

    #[test]
    fn half_gain_halves_samples() {
        let mut p = GainProcessor::new(Gain::linear(0.5).unwrap());
        p.prepare(&config());
        let mut buf = AudioBuffer::new(1, 3);
        buf.channel_mut(0).copy_from_slice(&[1.0, -0.4, 0.2]);
        p.process(&mut buf);
        assert_eq!(buf.channel(0), &[0.5, -0.2, 0.1]);
    }

    #[test]
    fn silence_gain_mutes() {
        let mut p = GainProcessor::new(Gain::SILENCE);
        p.prepare(&config());
        let mut buf = AudioBuffer::new(1, 4);
        buf.channel_mut(0).fill(0.9);
        p.process(&mut buf);
        assert_eq!(buf.peak(), 0.0);
    }

    #[test]
    fn smoothing_reaches_target_and_is_deterministic() {
        let mut p = GainProcessor::new(Gain::SILENCE).with_smoothing_ms(1.0);
        p.prepare(&config()); // 1ms at 48k = 48 frames of ramp
        p.set_gain(Gain::UNITY);
        let mut buf = AudioBuffer::new(1, 48);
        buf.channel_mut(0).fill(1.0);
        p.process(&mut buf);
        assert!((p.current_factor() - 1.0).abs() < 1e-5);
        let out = buf.channel(0);
        assert!(out[0] < 0.1);
        assert!(out[47] > 0.9);
    }

    #[test]
    fn no_smoothing_jumps_immediately() {
        let mut p = GainProcessor::new(Gain::UNITY);
        p.prepare(&config());
        p.set_gain(Gain::linear(2.0).unwrap());
        let mut buf = AudioBuffer::new(1, 2);
        buf.channel_mut(0).fill(1.0);
        p.process(&mut buf);
        assert_eq!(buf.channel(0), &[2.0, 2.0]);
    }

    #[test]
    fn reset_clears_ramp() {
        let mut p = GainProcessor::new(Gain::SILENCE).with_smoothing_ms(10.0);
        p.prepare(&config());
        p.set_gain(Gain::UNITY);
        p.reset();
        assert_eq!(p.current_factor(), Gain::UNITY.factor());
    }
}
