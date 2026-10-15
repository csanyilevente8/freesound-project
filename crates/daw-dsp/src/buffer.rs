//! Non-interleaved audio buffers (planar `f32`).
//!
//! The engine uses planar `f32` buffers (one contiguous slice per channel),
//! which is the natural layout for most DSP and avoids per-sample channel
//! stride arithmetic. Device adapters convert to/from interleaved formats at
//! the boundary (not in Milestone 0).
//!
//! Buffers own their storage and are **preallocated**; the real-time path must
//! not allocate (spec §9.2). Allocation here happens only in `new`/`resize`,
//! which are called during preparation, never inside `process`.

use daw_core::audio::Sample;

/// A planar, owned audio buffer: `channels` slices each of `frames` samples.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    channels: usize,
    frames: usize,
    /// Flat storage of length `channels * frames`, channel-major.
    data: Vec<Sample>,
}

impl AudioBuffer {
    /// Allocates a silent buffer with `channels` channels of `frames` frames.
    #[must_use]
    pub fn new(channels: usize, frames: usize) -> Self {
        Self {
            channels,
            frames,
            data: vec![0.0; channels * frames],
        }
    }

    /// Number of channels.
    #[must_use]
    pub const fn channels(&self) -> usize {
        self.channels
    }

    /// Number of frames per channel.
    #[must_use]
    pub const fn frames(&self) -> usize {
        self.frames
    }

    /// Fills the entire buffer with silence (zeros). Allocation-free.
    pub fn clear(&mut self) {
        self.data.iter_mut().for_each(|s| *s = 0.0);
    }

    /// Immutable view of a single channel's samples.
    ///
    /// # Panics
    /// Panics if `channel >= channels`.
    #[must_use]
    pub fn channel(&self, channel: usize) -> &[Sample] {
        let start = channel * self.frames;
        &self.data[start..start + self.frames]
    }

    /// Mutable view of a single channel's samples.
    ///
    /// # Panics
    /// Panics if `channel >= channels`.
    pub fn channel_mut(&mut self, channel: usize) -> &mut [Sample] {
        let start = channel * self.frames;
        &mut self.data[start..start + self.frames]
    }

    /// Adds (mixes) `other` into `self`, sample-by-sample, up to the overlap of
    /// their channel/frame counts. Channels/frames present in only one buffer
    /// are left unchanged. Allocation-free.
    pub fn add_from(&mut self, other: &AudioBuffer) {
        let channels = self.channels.min(other.channels);
        let frames = self.frames.min(other.frames);
        for ch in 0..channels {
            let dst = {
                let start = ch * self.frames;
                &mut self.data[start..start + frames]
            };
            let src = {
                let start = ch * other.frames;
                &other.data[start..start + frames]
            };
            for (d, s) in dst.iter_mut().zip(src.iter()) {
                *d += *s;
            }
        }
    }

    /// Returns the peak absolute sample value across all channels.
    #[must_use]
    pub fn peak(&self) -> Sample {
        self.data.iter().fold(0.0, |m, s| m.max(s.abs()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_is_silent() {
        let b = AudioBuffer::new(2, 8);
        assert_eq!(b.channels(), 2);
        assert_eq!(b.frames(), 8);
        assert_eq!(b.peak(), 0.0);
    }

    #[test]
    fn channel_access_is_isolated() {
        let mut b = AudioBuffer::new(2, 4);
        b.channel_mut(0).fill(1.0);
        assert_eq!(b.channel(0), &[1.0, 1.0, 1.0, 1.0]);
        assert_eq!(b.channel(1), &[0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn add_from_sums_overlap() {
        let mut a = AudioBuffer::new(2, 4);
        a.channel_mut(0).fill(0.5);
        let mut b = AudioBuffer::new(2, 4);
        b.channel_mut(0).fill(0.25);
        b.channel_mut(1).fill(1.0);
        a.add_from(&b);
        assert_eq!(a.channel(0), &[0.75, 0.75, 0.75, 0.75]);
        assert_eq!(a.channel(1), &[1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn add_from_handles_mismatched_shapes() {
        let mut a = AudioBuffer::new(2, 4);
        a.channel_mut(0).fill(1.0);
        let mut mono_short = AudioBuffer::new(1, 2);
        mono_short.channel_mut(0).fill(1.0);
        a.add_from(&mono_short); // only ch0, frames 0..2 affected
        assert_eq!(a.channel(0), &[2.0, 2.0, 1.0, 1.0]);
        assert_eq!(a.channel(1), &[0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn peak_is_absolute() {
        let mut b = AudioBuffer::new(1, 3);
        b.channel_mut(0).copy_from_slice(&[-0.9, 0.2, -0.1]);
        assert!((b.peak() - 0.9).abs() < 1e-6);
    }

    #[test]
    fn clear_zeroes() {
        let mut b = AudioBuffer::new(2, 4);
        b.channel_mut(0).fill(1.0);
        b.clear();
        assert_eq!(b.peak(), 0.0);
    }
}
