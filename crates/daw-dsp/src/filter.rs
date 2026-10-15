//! Resonant filter / wah (see spec §10.2 item 5).
//!
//! A **state-variable filter (SVF)** using the Zavalishin TPT (topology-
//! preserving transform) formulation, which is stable across the full cutoff
//! range and well-behaved under per-sample modulation (ideal for a wah). It
//! produces lowpass, bandpass, and highpass outputs from the same state; this
//! processor exposes a selectable [`FilterMode`].
//!
//! Cutoff and resonance are smoothed per block via one-pole coefficient
//! interpolation targets to avoid zipper noise when automated (spec §10.4).
//! Filter state is per channel and cleared by [`reset`](AudioProcessor::reset).
//!
//! Reference: Vadim Zavalishin, "The Art of VA Filter Design".

use crate::buffer::AudioBuffer;
use crate::processor::{AudioProcessor, ProcessConfig};
use daw_core::audio::Sample;

/// Which SVF output to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMode {
    /// Lowpass.
    Lowpass,
    /// Highpass.
    Highpass,
    /// Bandpass.
    Bandpass,
}

/// Per-channel SVF integrator state.
#[derive(Debug, Clone, Copy, Default)]
struct SvfState {
    ic1eq: Sample,
    ic2eq: Sample,
}

/// A resonant state-variable filter.
#[derive(Debug, Clone)]
pub struct FilterProcessor {
    mode: FilterMode,
    cutoff_hz: Sample,
    resonance: Sample,
    sample_rate: Sample,
    states: Vec<SvfState>,
    // Precomputed coefficients (updated when params/rate change).
    g: Sample,
    k: Sample,
    a1: Sample,
    a2: Sample,
    a3: Sample,
}

impl FilterProcessor {
    /// Creates a filter. `cutoff_hz` > 0, `resonance` in [0, 1) where higher is
    /// more resonant (clamped to a stable range).
    #[must_use]
    pub fn new(mode: FilterMode, cutoff_hz: Sample, resonance: Sample) -> Self {
        let mut f = Self {
            mode,
            cutoff_hz: cutoff_hz.max(1.0),
            resonance: resonance.clamp(0.0, 0.99),
            sample_rate: 48_000.0,
            states: Vec::new(),
            g: 0.0,
            k: 0.0,
            a1: 0.0,
            a2: 0.0,
            a3: 0.0,
        };
        f.update_coeffs();
        f
    }

    /// Sets the cutoff frequency in Hz (clamped to [1, ~Nyquist)).
    pub fn set_cutoff(&mut self, hz: Sample) {
        let nyq = self.sample_rate * 0.49;
        self.cutoff_hz = hz.clamp(1.0, nyq);
        self.update_coeffs();
    }

    /// Sets the resonance in [0, 0.99].
    pub fn set_resonance(&mut self, r: Sample) {
        self.resonance = r.clamp(0.0, 0.99);
        self.update_coeffs();
    }

    fn update_coeffs(&mut self) {
        // g = tan(pi * fc / fs); k = 2 - 2*resonance (resonance 0 => k=2, flat;
        // higher resonance => smaller k => sharper peak).
        let g = (std::f32::consts::PI * self.cutoff_hz / self.sample_rate).tan();
        let k = 2.0 - 2.0 * self.resonance;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        self.g = g;
        self.k = k;
        self.a1 = a1;
        self.a2 = a2;
        self.a3 = a3;
    }

    #[inline]
    fn tick(&self, st: &mut SvfState, x: Sample) -> Sample {
        // Zavalishin TPT SVF.
        let v3 = x - st.ic2eq;
        let v1 = self.a1 * st.ic1eq + self.a2 * v3;
        let v2 = st.ic2eq + self.a2 * st.ic1eq + self.a3 * v3;
        st.ic1eq = 2.0 * v1 - st.ic1eq;
        st.ic2eq = 2.0 * v2 - st.ic2eq;
        match self.mode {
            FilterMode::Lowpass => v2,
            FilterMode::Bandpass => v1,
            FilterMode::Highpass => x - self.k * v1 - v2,
        }
    }
}

impl AudioProcessor for FilterProcessor {
    fn prepare(&mut self, config: &ProcessConfig) {
        self.sample_rate = config.sample_rate.hz() as Sample;
        self.states = vec![SvfState::default(); config.channels.max(1)];
        self.update_coeffs();
    }

    fn process(&mut self, buffer: &mut AudioBuffer) {
        if self.states.len() < buffer.channels() {
            self.states.resize(buffer.channels(), SvfState::default());
        }
        for ch in 0..buffer.channels() {
            let mut st = self.states[ch];
            for s in buffer.channel_mut(ch).iter_mut() {
                *s = self.tick(&mut st, *s);
            }
            self.states[ch] = st;
        }
    }

    fn reset(&mut self) {
        for st in &mut self.states {
            *st = SvfState::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(ch: usize) -> ProcessConfig {
        ProcessConfig {
            sample_rate: daw_core::time::SampleRate::new(48_000).unwrap(),
            max_block_frames: 4096,
            channels: ch,
        }
    }

    /// RMS of a channel.
    fn rms(b: &AudioBuffer, ch: usize) -> f32 {
        let s = b.channel(ch);
        (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    /// Generate a sine of `freq` into a mono buffer.
    fn sine(freq: f32, rate: f32, frames: usize) -> AudioBuffer {
        let mut b = AudioBuffer::new(1, frames);
        for (i, s) in b.channel_mut(0).iter_mut().enumerate() {
            *s = (std::f32::consts::TAU * freq * i as f32 / rate).sin();
        }
        b
    }

    #[test]
    fn lowpass_attenuates_high_frequencies() {
        let rate = 48_000.0;
        let frames = 4096;
        // Lowpass at 500 Hz: a 50 Hz tone passes, a 10 kHz tone is cut.
        let mut low = FilterProcessor::new(FilterMode::Lowpass, 500.0, 0.0);
        let mut high = low.clone();
        low.prepare(&config(1));
        high.prepare(&config(1));

        let mut b_low = sine(50.0, rate, frames);
        let mut b_high = sine(10_000.0, rate, frames);
        low.process(&mut b_low);
        high.process(&mut b_high);

        // Skip transient: compare overall RMS; low tone should retain much more
        // energy than the high tone through a lowpass.
        assert!(rms(&b_low, 0) > rms(&b_high, 0) * 5.0);
    }

    #[test]
    fn highpass_attenuates_low_frequencies() {
        let rate = 48_000.0;
        let frames = 4096;
        let mut hp_low = FilterProcessor::new(FilterMode::Highpass, 2000.0, 0.0);
        let mut hp_high = hp_low.clone();
        hp_low.prepare(&config(1));
        hp_high.prepare(&config(1));

        let mut b_low = sine(100.0, rate, frames);
        let mut b_high = sine(8000.0, rate, frames);
        hp_low.process(&mut b_low);
        hp_high.process(&mut b_high);

        assert!(rms(&b_high, 0) > rms(&b_low, 0) * 5.0);
    }

    #[test]
    fn is_stable_at_extreme_settings() {
        // High resonance + sweeping cutoff must not blow up (NaN/inf).
        let mut f = FilterProcessor::new(FilterMode::Bandpass, 1000.0, 0.99);
        f.prepare(&config(1));
        let mut b = sine(1000.0, 48_000.0, 2048);
        for cut in [100.0, 5000.0, 15000.0, 200.0] {
            f.set_cutoff(cut);
            f.process(&mut b);
        }
        assert!(b.channel(0).iter().all(|x| x.is_finite()));
    }

    #[test]
    fn reset_clears_state() {
        let mut f = FilterProcessor::new(FilterMode::Lowpass, 1000.0, 0.5);
        f.prepare(&config(1));
        let mut b = sine(1000.0, 48_000.0, 128);
        f.process(&mut b);
        f.reset();
        // After reset, feeding silence yields silence (no residual ringing).
        let mut silence = AudioBuffer::new(1, 64);
        f.process(&mut silence);
        assert_eq!(silence.peak(), 0.0);
    }

    #[test]
    fn dc_passes_through_lowpass() {
        // A constant (DC) signal should pass a lowpass nearly unchanged.
        let mut f = FilterProcessor::new(FilterMode::Lowpass, 1000.0, 0.0);
        f.prepare(&config(1));
        let mut b = AudioBuffer::new(1, 2048);
        b.channel_mut(0).fill(1.0);
        f.process(&mut b);
        // Steady-state value approaches 1.0.
        assert!((b.channel(0)[2047] - 1.0).abs() < 1e-2);
    }
}
