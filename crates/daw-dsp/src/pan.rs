//! Pan processor (see spec §10.2 item 2).
//!
//! Applies stereo panning using a **constant-power (−3 dB center) pan law**:
//! for a pan position `p` in [-1, 1] mapped to an angle `θ = (p + 1) * π/4`,
//! the left gain is `cos θ` and the right gain is `sin θ`. At center (`p = 0`)
//! both channels get `cos(π/4) = sin(π/4) ≈ 0.707` (−3 dB), so the perceived
//! loudness is constant across the stereo field.
//!
//! This processor operates on a stereo buffer (2 channels). On a mono buffer it
//! is a no-op (there is no stereo field to pan); callers that want mono→stereo
//! panning should widen the buffer to stereo first.
//!
//! Pan position changes are smoothed over a short window to avoid zipper noise
//! (spec §10.4), using the same frame-resolved approach as the gain processor.

use crate::buffer::AudioBuffer;
use crate::processor::{AudioProcessor, ProcessConfig};
use daw_core::audio::Sample;

/// Constant-power stereo pan.
#[derive(Debug, Clone)]
pub struct PanProcessor {
    /// Target pan position in [-1, 1].
    target: Sample,
    /// Current (possibly smoothing) pan position.
    current: Sample,
    step: Sample,
    smoothing_ms: f32,
    smoothing_frames: usize,
}

impl PanProcessor {
    /// Creates a pan processor at position `pan` (clamped to [-1, 1]).
    #[must_use]
    pub fn new(pan: Sample) -> Self {
        let p = pan.clamp(-1.0, 1.0);
        Self {
            target: p,
            current: p,
            step: 0.0,
            smoothing_ms: 0.0,
            smoothing_frames: 0,
        }
    }

    /// Sets the smoothing time in milliseconds for future pan changes.
    #[must_use]
    pub fn with_smoothing_ms(mut self, ms: f32) -> Self {
        self.smoothing_ms = ms.max(0.0);
        self
    }

    /// Requests a new pan position (clamped to [-1, 1]).
    pub fn set_pan(&mut self, pan: Sample) {
        self.target = pan.clamp(-1.0, 1.0);
        if self.smoothing_frames == 0 {
            self.current = self.target;
            self.step = 0.0;
        } else {
            self.step = (self.target - self.current) / self.smoothing_frames as Sample;
        }
    }

    /// Returns the left/right gains for a pan position using the constant-power
    /// law.
    #[must_use]
    pub fn gains(pan: Sample) -> (Sample, Sample) {
        let angle = (pan.clamp(-1.0, 1.0) + 1.0) * (std::f32::consts::FRAC_PI_4);
        (angle.cos(), angle.sin())
    }

    /// Current pan position (useful for tests/metering).
    #[must_use]
    pub fn current_pan(&self) -> Sample {
        self.current
    }
}

impl AudioProcessor for PanProcessor {
    fn prepare(&mut self, config: &ProcessConfig) {
        self.smoothing_frames =
            ((self.smoothing_ms / 1000.0) * config.sample_rate.hz() as f32).round() as usize;
        if self.smoothing_frames > 0 && (self.target - self.current).abs() > Sample::EPSILON {
            self.step = (self.target - self.current) / self.smoothing_frames as Sample;
        }
    }

    fn process(&mut self, buffer: &mut AudioBuffer) {
        if buffer.channels() < 2 {
            return; // mono: nothing to pan
        }
        let frames = buffer.frames();

        if self.step == 0.0 {
            let (lg, rg) = Self::gains(self.current);
            for s in buffer.channel_mut(0).iter_mut() {
                *s *= lg;
            }
            for s in buffer.channel_mut(1).iter_mut() {
                *s *= rg;
            }
            return;
        }

        // Smoothing: recompute gains per frame from the ramping pan position.
        let start = self.current;
        let step = self.step;
        let target = self.target;

        // Left channel.
        {
            let mut p = start;
            for s in buffer.channel_mut(0).iter_mut() {
                let (lg, _) = Self::gains(p);
                *s *= lg;
                p += step;
                if (step > 0.0 && p >= target) || (step < 0.0 && p <= target) {
                    p = target;
                }
            }
        }
        // Right channel (same ramp).
        {
            let mut p = start;
            for s in buffer.channel_mut(1).iter_mut() {
                let (_, rg) = Self::gains(p);
                *s *= rg;
                p += step;
                if (step > 0.0 && p >= target) || (step < 0.0 && p <= target) {
                    p = target;
                }
            }
        }

        let mut p = self.current + step * frames as Sample;
        if (step > 0.0 && p >= target) || (step < 0.0 && p <= target) {
            p = target;
            self.step = 0.0;
        }
        self.current = p;
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

    fn stereo(v: f32) -> AudioBuffer {
        let mut b = AudioBuffer::new(2, 4);
        b.channel_mut(0).fill(v);
        b.channel_mut(1).fill(v);
        b
    }

    #[test]
    fn center_is_minus_3db_both_sides() {
        let (l, r) = PanProcessor::gains(0.0);
        assert!((l - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((r - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    }

    #[test]
    fn hard_left_mutes_right() {
        let (l, r) = PanProcessor::gains(-1.0);
        assert!((l - 1.0).abs() < 1e-6);
        assert!(r.abs() < 1e-6);
    }

    #[test]
    fn hard_right_mutes_left() {
        let (l, r) = PanProcessor::gains(1.0);
        assert!(l.abs() < 1e-6);
        assert!((r - 1.0).abs() < 1e-6);
    }

    #[test]
    fn constant_power_holds() {
        for &p in &[-1.0, -0.5, 0.0, 0.3, 1.0] {
            let (l, r) = PanProcessor::gains(p);
            assert!(
                (l * l + r * r - 1.0).abs() < 1e-5,
                "power not constant at {p}"
            );
        }
    }

    #[test]
    fn processing_center_scales_both_by_0707() {
        let mut p = PanProcessor::new(0.0);
        p.prepare(&config());
        let mut b = stereo(1.0);
        p.process(&mut b);
        let x = std::f32::consts::FRAC_1_SQRT_2;
        assert!((b.channel(0)[0] - x).abs() < 1e-6);
        assert!((b.channel(1)[0] - x).abs() < 1e-6);
    }

    #[test]
    fn mono_buffer_is_noop() {
        let mut p = PanProcessor::new(1.0);
        p.prepare(&config());
        let mut b = AudioBuffer::new(1, 4);
        b.channel_mut(0).fill(0.5);
        p.process(&mut b);
        assert_eq!(b.channel(0), &[0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn smoothing_settles_at_target() {
        let mut p = PanProcessor::new(0.0).with_smoothing_ms(1.0);
        p.prepare(&config()); // 48 frames
        p.set_pan(1.0);
        let mut b = AudioBuffer::new(2, 48);
        b.channel_mut(0).fill(1.0);
        b.channel_mut(1).fill(1.0);
        p.process(&mut b);
        assert!((p.current_pan() - 1.0).abs() < 1e-5);
    }
}
