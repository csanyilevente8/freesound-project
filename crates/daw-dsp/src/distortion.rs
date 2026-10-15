//! Distortion / drive processor (see spec §10.2 item 4).
//!
//! A `tanh` soft-clip waveshaper: each sample is shaped as
//! `output_gain * tanh(drive * x)`. `drive` (≥ 1 typically) sets how hard the
//! signal is pushed into the nonlinearity; `tanh` saturates smoothly toward
//! ±1, avoiding the harsh aliasing of a hard clip while still adding harmonics.
//! `output_gain` compensates the level after shaping.
//!
//! The transfer function is memoryless (no filter state), so it is trivially
//! real-time safe and deterministic. Parameter changes take effect on the next
//! block; per-sample smoothing is unnecessary for a static transfer curve
//! (gain-style zipper noise does not apply to the shape itself), though drive
//! is applied as a plain multiply so large jumps could click — callers that
//! automate drive heavily should ramp it via the parameter layer later.

use crate::buffer::AudioBuffer;
use crate::processor::{AudioProcessor, ProcessConfig};
use daw_core::audio::Sample;

/// A `tanh` soft-clip distortion.
#[derive(Debug, Clone)]
pub struct DistortionProcessor {
    drive: Sample,
    output: Sample,
}

impl DistortionProcessor {
    /// Creates a distortion with the given `drive` (pre-gain, clamped to ≥ 0)
    /// and `output` (post-gain, clamped to ≥ 0).
    #[must_use]
    pub fn new(drive: Sample, output: Sample) -> Self {
        Self {
            drive: drive.max(0.0),
            output: output.max(0.0),
        }
    }

    /// Sets the drive (pre-gain), clamped to ≥ 0.
    pub fn set_drive(&mut self, drive: Sample) {
        self.drive = drive.max(0.0);
    }

    /// Sets the output (post-gain), clamped to ≥ 0.
    pub fn set_output(&mut self, output: Sample) {
        self.output = output.max(0.0);
    }

    /// The memoryless transfer function applied to one sample.
    #[must_use]
    pub fn shape(&self, x: Sample) -> Sample {
        self.output * (self.drive * x).tanh()
    }
}

impl AudioProcessor for DistortionProcessor {
    fn prepare(&mut self, _config: &ProcessConfig) {}

    fn process(&mut self, buffer: &mut AudioBuffer) {
        let drive = self.drive;
        let output = self.output;
        for ch in 0..buffer.channels() {
            for s in buffer.channel_mut(ch).iter_mut() {
                *s = output * (drive * *s).tanh();
            }
        }
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ProcessConfig {
        ProcessConfig {
            sample_rate: daw_core::time::SampleRate::new(48_000).unwrap(),
            max_block_frames: 512,
            channels: 1,
        }
    }

    #[test]
    fn unity_drive_output_passes_small_signals_nearly_linear() {
        // tanh(x) ≈ x for small x.
        let d = DistortionProcessor::new(1.0, 1.0);
        let y = d.shape(0.01);
        assert!((y - 0.01).abs() < 1e-4);
    }

    #[test]
    fn saturates_toward_output_gain() {
        let d = DistortionProcessor::new(100.0, 1.0);
        // Large input drives tanh toward 1.0.
        assert!((d.shape(1.0) - 1.0).abs() < 1e-3);
        assert!((d.shape(-1.0) + 1.0).abs() < 1e-3);
    }

    #[test]
    fn output_gain_scales_result() {
        let d = DistortionProcessor::new(100.0, 0.5);
        assert!((d.shape(1.0) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn is_odd_symmetric() {
        let d = DistortionProcessor::new(3.0, 1.0);
        assert!((d.shape(0.4) + d.shape(-0.4)).abs() < 1e-6);
    }

    #[test]
    fn bounded_output_never_exceeds_output_gain() {
        let d = DistortionProcessor::new(50.0, 2.0);
        for x in [-10.0, -1.0, 0.0, 0.5, 10.0] {
            assert!(d.shape(x).abs() <= 2.0 + 1e-6);
        }
    }

    #[test]
    fn process_applies_to_buffer() {
        let mut d = DistortionProcessor::new(1.0, 1.0);
        d.prepare(&config());
        let mut b = AudioBuffer::new(1, 3);
        b.channel_mut(0).copy_from_slice(&[0.0, 0.01, -0.01]);
        d.process(&mut b);
        assert!(b.channel(0)[0].abs() < 1e-6);
        assert!((b.channel(0)[1] - 0.01).abs() < 1e-4);
    }

    #[test]
    fn negative_params_clamped_to_zero() {
        let d = DistortionProcessor::new(-5.0, -1.0);
        assert_eq!(d.shape(0.5), 0.0); // output gain 0
    }
}
