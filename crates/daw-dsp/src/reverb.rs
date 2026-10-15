//! Reverb processor (see spec §10.2 item 8).
//!
//! A compact Schroeder-style algorithmic reverb: four parallel feedback comb
//! filters into two series allpass filters, per channel, with a wet/dry mix and
//! a room-size control that scales the comb feedback. This is intentionally a
//! simple, stable starting point (spec: "start with a simple algorithmic
//! reverb"), not a production hall. All buffers are preallocated in
//! [`prepare`](AudioProcessor::prepare); [`process`](AudioProcessor::process)
//! is allocation-free.

use crate::buffer::AudioBuffer;
use crate::processor::{AudioProcessor, ProcessConfig};
use daw_core::audio::Sample;

/// Comb filter delay lengths in samples (at 44.1k, classic Freeverb-ish
/// values). Scaled to the actual sample rate in `prepare`.
const COMB_LENGMS: [f32; 4] = [25.3, 26.9, 28.9, 30.7];
/// Allpass filter delay lengths in ms.
const ALLPASS_LENGMS: [f32; 2] = [5.0, 1.7];

#[derive(Debug, Clone, Default)]
struct Comb {
    buf: Vec<Sample>,
    head: usize,
    feedback: Sample,
}

impl Comb {
    fn tick(&mut self, x: Sample) -> Sample {
        let y = self.buf[self.head];
        self.buf[self.head] = x + y * self.feedback;
        self.head = (self.head + 1) % self.buf.len();
        y
    }
    fn clear(&mut self) {
        self.buf.iter_mut().for_each(|s| *s = 0.0);
        self.head = 0;
    }
}

#[derive(Debug, Clone, Default)]
struct Allpass {
    buf: Vec<Sample>,
    head: usize,
    gain: Sample,
}

impl Allpass {
    fn tick(&mut self, x: Sample) -> Sample {
        let buffered = self.buf[self.head];
        let y = -x + buffered;
        self.buf[self.head] = x + buffered * self.gain;
        self.head = (self.head + 1) % self.buf.len();
        y
    }
    fn clear(&mut self) {
        self.buf.iter_mut().for_each(|s| *s = 0.0);
        self.head = 0;
    }
}

#[derive(Debug, Clone, Default)]
struct Channel {
    combs: Vec<Comb>,
    allpasses: Vec<Allpass>,
}

/// A simple Schroeder reverb.
#[derive(Debug, Clone)]
pub struct ReverbProcessor {
    /// Room size in [0, 1], scales comb feedback.
    room: Sample,
    /// Wet/dry mix in [0, 1].
    mix: Sample,
    sample_rate: f32,
    channels: Vec<Channel>,
}

impl ReverbProcessor {
    /// Creates a reverb with `room` size and wet/dry `mix`, both clamped to
    /// [0, 1].
    #[must_use]
    pub fn new(room: Sample, mix: Sample) -> Self {
        Self {
            room: room.clamp(0.0, 1.0),
            mix: mix.clamp(0.0, 1.0),
            sample_rate: 48_000.0,
            channels: Vec::new(),
        }
    }

    /// Sets room size in [0, 1].
    pub fn set_room(&mut self, room: Sample) {
        self.room = room.clamp(0.0, 1.0);
        let fb = 0.7 + 0.28 * self.room;
        for ch in &mut self.channels {
            for c in &mut ch.combs {
                c.feedback = fb;
            }
        }
    }

    /// Sets wet/dry mix in [0, 1].
    pub fn set_mix(&mut self, mix: Sample) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    fn len_from_ms(&self, ms: f32) -> usize {
        ((ms / 1000.0) * self.sample_rate).round() as usize + 1
    }
}

impl AudioProcessor for ReverbProcessor {
    fn prepare(&mut self, config: &ProcessConfig) {
        self.sample_rate = config.sample_rate.hz() as f32;
        let fb = 0.7 + 0.28 * self.room;
        let mut channels = Vec::with_capacity(config.channels.max(1));
        for ch in 0..config.channels.max(1) {
            // Slightly detune the second+ channels for stereo width.
            let detune = 1.0 + (ch as f32) * 0.012;
            let combs = COMB_LENGMS
                .iter()
                .map(|&ms| Comb {
                    buf: vec![0.0; self.len_from_ms(ms * detune)],
                    head: 0,
                    feedback: fb,
                })
                .collect();
            let allpasses = ALLPASS_LENGMS
                .iter()
                .map(|&ms| Allpass {
                    buf: vec![0.0; self.len_from_ms(ms * detune)],
                    head: 0,
                    gain: 0.5,
                })
                .collect();
            channels.push(Channel { combs, allpasses });
        }
        self.channels = channels;
    }

    fn process(&mut self, buffer: &mut AudioBuffer) {
        if self.channels.len() < buffer.channels() {
            return;
        }
        let mix = self.mix;
        for ch in 0..buffer.channels() {
            let state = &mut self.channels[ch];
            for s in buffer.channel_mut(ch).iter_mut() {
                let dry = *s;
                // Parallel combs summed.
                let mut wet = 0.0;
                for c in &mut state.combs {
                    wet += c.tick(dry);
                }
                wet /= state.combs.len() as Sample;
                // Series allpasses.
                for a in &mut state.allpasses {
                    wet = a.tick(wet);
                }
                *s = dry * (1.0 - mix) + wet * mix;
            }
        }
    }

    fn reset(&mut self) {
        for ch in &mut self.channels {
            ch.combs.iter_mut().for_each(Comb::clear);
            ch.allpasses.iter_mut().for_each(Allpass::clear);
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

    #[test]
    fn produces_a_decaying_tail_from_an_impulse() {
        let mut r = ReverbProcessor::new(0.5, 1.0);
        r.prepare(&config(1));
        let mut b = AudioBuffer::new(1, 8000);
        b.channel_mut(0)[0] = 1.0;
        r.process(&mut b);
        // There is energy well after the impulse (a tail).
        let tail: f32 = b.channel(0)[2000..4000].iter().map(|x| x.abs()).sum();
        assert!(tail > 0.0);
        // And it is finite/stable.
        assert!(b.channel(0).iter().all(|x| x.is_finite()));
    }

    #[test]
    fn stays_stable_over_long_input() {
        let mut r = ReverbProcessor::new(1.0, 1.0); // max room = max feedback
        r.prepare(&config(2));
        let mut b = AudioBuffer::new(2, 8192);
        for ch in 0..2 {
            for (i, s) in b.channel_mut(ch).iter_mut().enumerate() {
                *s = (i as f32 * 0.01).sin() * 0.5;
            }
        }
        r.process(&mut b);
        assert!(b.channel(0).iter().all(|x| x.is_finite() && x.abs() < 10.0));
    }

    #[test]
    fn dry_mix_passes_input() {
        let mut r = ReverbProcessor::new(0.5, 0.0);
        r.prepare(&config(1));
        let mut b = AudioBuffer::new(1, 16);
        b.channel_mut(0).fill(0.2);
        r.process(&mut b);
        assert!(b.channel(0).iter().all(|&s| (s - 0.2).abs() < 1e-6));
    }

    #[test]
    fn reset_clears_tail() {
        let mut r = ReverbProcessor::new(0.8, 1.0);
        r.prepare(&config(1));
        let mut b = AudioBuffer::new(1, 1000);
        b.channel_mut(0)[0] = 1.0;
        r.process(&mut b);
        r.reset();
        let mut silence = AudioBuffer::new(1, 1000);
        r.process(&mut silence);
        assert_eq!(silence.peak(), 0.0);
    }
}
