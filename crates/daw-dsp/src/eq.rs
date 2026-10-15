//! Parametric EQ band (see spec §10.2 item 6).
//!
//! A single biquad band with selectable type (peaking, low-shelf, high-shelf),
//! using the standard RBJ "Audio EQ Cookbook" coefficient formulas. Multiple
//! bands can be chained to form a multi-band EQ; starting with one well-tested
//! band keeps the first increment small (spec: "begin with a small number of
//! useful bands or filters").
//!
//! State is per channel (biquad delay elements) and cleared by
//! [`reset`](AudioProcessor::reset).

use crate::buffer::AudioBuffer;
use crate::processor::{AudioProcessor, ProcessConfig};
use daw_core::audio::Sample;

/// EQ band type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqBandType {
    /// Peaking (bell) filter centered at the frequency.
    Peaking,
    /// Low shelf.
    LowShelf,
    /// High shelf.
    HighShelf,
}

#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    x1: Sample,
    x2: Sample,
    y1: Sample,
    y2: Sample,
}

/// A single parametric EQ band (biquad).
#[derive(Debug, Clone)]
pub struct EqBand {
    band_type: EqBandType,
    freq: f32,
    q: f32,
    gain_db: f32,
    sample_rate: f32,
    // Normalized coefficients (a0 divided out).
    b0: Sample,
    b1: Sample,
    b2: Sample,
    a1: Sample,
    a2: Sample,
    states: Vec<BiquadState>,
}

impl EqBand {
    /// Creates an EQ band. `freq` Hz, `q` > 0, `gain_db` boost/cut (for
    /// peaking/shelf).
    #[must_use]
    pub fn new(band_type: EqBandType, freq: f32, q: f32, gain_db: f32) -> Self {
        let mut b = Self {
            band_type,
            freq: freq.max(1.0),
            q: q.max(0.01),
            gain_db,
            sample_rate: 48_000.0,
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            states: Vec::new(),
        };
        b.update_coeffs();
        b
    }

    /// Sets the center/corner frequency in Hz.
    pub fn set_freq(&mut self, hz: f32) {
        self.freq = hz.clamp(1.0, self.sample_rate * 0.49);
        self.update_coeffs();
    }

    /// Sets the gain in dB.
    pub fn set_gain_db(&mut self, db: f32) {
        self.gain_db = db;
        self.update_coeffs();
    }

    /// Sets Q.
    pub fn set_q(&mut self, q: f32) {
        self.q = q.max(0.01);
        self.update_coeffs();
    }

    fn update_coeffs(&mut self) {
        // RBJ cookbook formulas.
        let a = 10.0_f32.powf(self.gain_db / 40.0);
        let w0 = std::f32::consts::TAU * self.freq / self.sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * self.q);

        let (b0, b1, b2, a0, a1, a2) = match self.band_type {
            EqBandType::Peaking => (
                1.0 + alpha * a,
                -2.0 * cos_w0,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos_w0,
                1.0 - alpha / a,
            ),
            EqBandType::LowShelf => {
                let sqrt_a = a.sqrt();
                let tsa = 2.0 * sqrt_a * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos_w0 + tsa),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0),
                    a * ((a + 1.0) - (a - 1.0) * cos_w0 - tsa),
                    (a + 1.0) + (a - 1.0) * cos_w0 + tsa,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
                    (a + 1.0) + (a - 1.0) * cos_w0 - tsa,
                )
            }
            EqBandType::HighShelf => {
                let sqrt_a = a.sqrt();
                let tsa = 2.0 * sqrt_a * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos_w0 + tsa),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0),
                    a * ((a + 1.0) + (a - 1.0) * cos_w0 - tsa),
                    (a + 1.0) - (a - 1.0) * cos_w0 + tsa,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos_w0),
                    (a + 1.0) - (a - 1.0) * cos_w0 - tsa,
                )
            }
        };

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }
}

impl AudioProcessor for EqBand {
    fn prepare(&mut self, config: &ProcessConfig) {
        self.sample_rate = config.sample_rate.hz() as f32;
        self.states = vec![BiquadState::default(); config.channels.max(1)];
        self.update_coeffs();
    }

    fn process(&mut self, buffer: &mut AudioBuffer) {
        if self.states.len() < buffer.channels() {
            self.states
                .resize(buffer.channels(), BiquadState::default());
        }
        for ch in 0..buffer.channels() {
            let mut st = self.states[ch];
            for s in buffer.channel_mut(ch).iter_mut() {
                let x0 = *s;
                let y0 = self.b0 * x0 + self.b1 * st.x1 + self.b2 * st.x2
                    - self.a1 * st.y1
                    - self.a2 * st.y2;
                st.x2 = st.x1;
                st.x1 = x0;
                st.y2 = st.y1;
                st.y1 = y0;
                *s = y0;
            }
            self.states[ch] = st;
        }
    }

    fn reset(&mut self) {
        for st in &mut self.states {
            *st = BiquadState::default();
        }
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

    fn sine(freq: f32, rate: f32, frames: usize) -> AudioBuffer {
        let mut b = AudioBuffer::new(1, frames);
        for (i, s) in b.channel_mut(0).iter_mut().enumerate() {
            *s = (std::f32::consts::TAU * freq * i as f32 / rate).sin();
        }
        b
    }

    #[test]
    fn zero_gain_peaking_is_near_unity() {
        let mut eq = EqBand::new(EqBandType::Peaking, 1000.0, 1.0, 0.0);
        eq.prepare(&config(1));
        let mut b = sine(1000.0, 48_000.0, 4096);
        let before = rms(&b, 0);
        eq.process(&mut b);
        assert!((rms(&b, 0) - before).abs() < 1e-2);
    }

    #[test]
    fn peaking_boost_increases_energy_at_center() {
        let mut eq = EqBand::new(EqBandType::Peaking, 1000.0, 1.0, 12.0);
        eq.prepare(&config(1));
        let mut b = sine(1000.0, 48_000.0, 8192);
        let before = rms(&b, 0);
        eq.process(&mut b);
        // Skip startup transient by comparing overall RMS; boost should raise it.
        assert!(rms(&b, 0) > before * 1.5);
    }

    #[test]
    fn peaking_cut_reduces_energy_at_center() {
        let mut eq = EqBand::new(EqBandType::Peaking, 1000.0, 1.0, -12.0);
        eq.prepare(&config(1));
        let mut b = sine(1000.0, 48_000.0, 8192);
        let before = rms(&b, 0);
        eq.process(&mut b);
        assert!(rms(&b, 0) < before * 0.7);
    }

    #[test]
    fn low_shelf_boost_raises_low_freq() {
        let mut eq = EqBand::new(EqBandType::LowShelf, 2000.0, 0.707, 12.0);
        eq.prepare(&config(1));
        let mut low = sine(100.0, 48_000.0, 8192);
        let before = rms(&low, 0);
        eq.process(&mut low);
        assert!(rms(&low, 0) > before * 1.5);
    }

    #[test]
    fn output_is_finite_and_resettable() {
        let mut eq = EqBand::new(EqBandType::HighShelf, 5000.0, 0.707, 9.0);
        eq.prepare(&config(1));
        let mut b = sine(5000.0, 48_000.0, 1024);
        eq.process(&mut b);
        assert!(b.channel(0).iter().all(|x| x.is_finite()));
        eq.reset();
        let mut silence = AudioBuffer::new(1, 64);
        eq.process(&mut silence);
        assert_eq!(silence.peak(), 0.0);
    }
}
