//! Delay processor (see spec §10.2 item 7).
//!
//! A per-channel feedback delay line with time, feedback, and wet/dry mix.
//! Feedback is clamped below 1.0 so the line cannot grow unbounded (spec warns
//! against unstable feedback values). The delay buffer is preallocated in
//! [`prepare`](AudioProcessor::prepare) from a configured maximum delay time;
//! [`process`](AudioProcessor::process) is allocation-free.

use crate::buffer::AudioBuffer;
use crate::processor::{AudioProcessor, ProcessConfig};
use daw_core::audio::Sample;

/// A feedback delay with wet/dry mix.
#[derive(Debug, Clone)]
pub struct DelayProcessor {
    delay_seconds: f32,
    max_seconds: f32,
    feedback: Sample,
    /// Wet/dry mix in [0, 1]; 0 = dry only, 1 = wet only.
    mix: Sample,
    sample_rate: f32,
    delay_samples: usize,
    // Per-channel ring buffers and write heads.
    lines: Vec<Vec<Sample>>,
    heads: Vec<usize>,
}

impl DelayProcessor {
    /// Creates a delay. `delay_seconds` is the delay time, `feedback` in
    /// [0, 0.99], `mix` in [0, 1]. `max_seconds` bounds the preallocated line.
    #[must_use]
    pub fn new(delay_seconds: f32, feedback: Sample, mix: Sample, max_seconds: f32) -> Self {
        Self {
            delay_seconds: delay_seconds.max(0.0),
            max_seconds: max_seconds.max(delay_seconds).max(0.0),
            feedback: feedback.clamp(0.0, 0.99),
            mix: mix.clamp(0.0, 1.0),
            sample_rate: 48_000.0,
            delay_samples: 0,
            lines: Vec::new(),
            heads: Vec::new(),
        }
    }

    /// Sets the delay time in seconds (clamped to the configured maximum).
    pub fn set_delay_seconds(&mut self, seconds: f32) {
        self.delay_seconds = seconds.clamp(0.0, self.max_seconds);
        self.recompute_delay_samples();
    }

    /// Sets feedback in [0, 0.99].
    pub fn set_feedback(&mut self, feedback: Sample) {
        self.feedback = feedback.clamp(0.0, 0.99);
    }

    /// Sets wet/dry mix in [0, 1].
    pub fn set_mix(&mut self, mix: Sample) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    fn recompute_delay_samples(&mut self) {
        let n = (self.delay_seconds * self.sample_rate).round() as usize;
        let cap = self.lines.first().map_or(usize::MAX, Vec::len);
        self.delay_samples = n.min(cap.saturating_sub(1)).max(1);
    }
}

impl AudioProcessor for DelayProcessor {
    fn prepare(&mut self, config: &ProcessConfig) {
        self.sample_rate = config.sample_rate.hz() as f32;
        let cap = ((self.max_seconds * self.sample_rate).ceil() as usize).max(1) + 1;
        self.lines = vec![vec![0.0; cap]; config.channels.max(1)];
        self.heads = vec![0; config.channels.max(1)];
        self.recompute_delay_samples();
    }

    fn process(&mut self, buffer: &mut AudioBuffer) {
        if self.lines.len() < buffer.channels() {
            return; // not prepared for this channel count
        }
        let d = self.delay_samples;
        let fb = self.feedback;
        let mix = self.mix;
        for ch in 0..buffer.channels() {
            let line = &mut self.lines[ch];
            let cap = line.len();
            let mut head = self.heads[ch];
            for s in buffer.channel_mut(ch).iter_mut() {
                let read = (head + cap - d) % cap;
                let delayed = line[read];
                // Write input + feedback of the delayed sample.
                line[head] = *s + delayed * fb;
                // Output is the dry/wet mix.
                *s = *s * (1.0 - mix) + delayed * mix;
                head = (head + 1) % cap;
            }
            self.heads[ch] = head;
        }
    }

    fn reset(&mut self) {
        for line in &mut self.lines {
            line.iter_mut().for_each(|s| *s = 0.0);
        }
        self.heads.iter_mut().for_each(|h| *h = 0);
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

    #[test]
    fn impulse_reappears_after_delay() {
        // 10-sample delay, no feedback, full wet: an impulse at 0 reappears at 10.
        let mut d = DelayProcessor::new(10.0 / 48_000.0, 0.0, 1.0, 1.0);
        d.prepare(&config(1));
        let mut b = AudioBuffer::new(1, 32);
        b.channel_mut(0)[0] = 1.0;
        d.process(&mut b);
        assert!((b.channel(0)[10] - 1.0).abs() < 1e-6);
        assert!(b.channel(0)[5].abs() < 1e-6);
    }

    #[test]
    fn feedback_decays_not_grows() {
        let mut d = DelayProcessor::new(4.0 / 48_000.0, 0.5, 1.0, 1.0);
        d.prepare(&config(1));
        let mut b = AudioBuffer::new(1, 64);
        b.channel_mut(0)[0] = 1.0;
        d.process(&mut b);
        // Echoes at 4, 8, 12 ... decaying by 0.5 each time.
        assert!((b.channel(0)[4] - 1.0).abs() < 1e-6);
        assert!((b.channel(0)[8] - 0.5).abs() < 1e-6);
        assert!((b.channel(0)[12] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn feedback_is_clamped_below_one() {
        let d = DelayProcessor::new(0.01, 5.0, 1.0, 1.0);
        assert!(d.feedback <= 0.99);
    }

    #[test]
    fn dry_mix_passes_input() {
        let mut d = DelayProcessor::new(0.01, 0.5, 0.0, 1.0); // mix 0 = dry
        d.prepare(&config(1));
        let mut b = AudioBuffer::new(1, 8);
        b.channel_mut(0).fill(0.3);
        d.process(&mut b);
        assert!(b.channel(0).iter().all(|&s| (s - 0.3).abs() < 1e-6));
    }

    #[test]
    fn reset_clears_tail() {
        let mut d = DelayProcessor::new(4.0 / 48_000.0, 0.5, 1.0, 1.0);
        d.prepare(&config(1));
        let mut b = AudioBuffer::new(1, 16);
        b.channel_mut(0)[0] = 1.0;
        d.process(&mut b);
        d.reset();
        let mut silence = AudioBuffer::new(1, 16);
        d.process(&mut silence);
        assert_eq!(silence.peak(), 0.0);
    }
}
