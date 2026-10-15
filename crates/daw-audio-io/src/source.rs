//! The real-time block source abstraction (see ADR-0007, ADR-0003).
//!
//! A [`BlockSource`] produces audio one block at a time at an absolute frame
//! position. The audio callback calls [`BlockSource::fill`] with whatever frame
//! count the device requests (variable block size, ADR-0003). Implementations
//! must be real-time safe: no allocation, locks, or I/O inside `fill`.
//!
//! For the first live-playback slice the source is a **prepared, in-memory
//! buffer** ([`PreparedBuffer`]) — the whole project is rendered offline up
//! front (reusing `daw-render`) and simply played back. This keeps the
//! real-time path trivially safe while proving device output end to end. A
//! streaming, block-rendering source can implement the same trait later without
//! changing the player or device layer.

use daw_core::audio::Sample;

/// Produces interleaved-free (planar) stereo audio at a frame position.
pub trait BlockSource: Send {
    /// Total length in frames (used to detect end-of-material).
    fn len_frames(&self) -> u64;

    /// Fills `left`/`right` (equal length = the block size) starting at absolute
    /// `start_frame`. Frames beyond the end must be written as silence. Returns
    /// the number of non-silent frames written (0 once fully past the end).
    ///
    /// Must be real-time safe (no alloc/lock/IO).
    fn fill(&self, start_frame: u64, left: &mut [Sample], right: &mut [Sample]) -> usize;
}

/// A prepared, in-memory stereo buffer played back as-is.
pub struct PreparedBuffer {
    left: Vec<Sample>,
    right: Vec<Sample>,
}

impl PreparedBuffer {
    /// Creates a prepared buffer from two equal-length planar channels.
    ///
    /// # Panics
    /// Panics if the channel lengths differ.
    #[must_use]
    pub fn new(left: Vec<Sample>, right: Vec<Sample>) -> Self {
        assert_eq!(left.len(), right.len(), "channels must be equal length");
        Self { left, right }
    }

    /// Builds a prepared buffer from a 2-channel planar slice set. Mono input is
    /// duplicated to both sides; empty input yields an empty buffer.
    #[must_use]
    pub fn from_channels(channels: &[Vec<Sample>]) -> Self {
        match channels.len() {
            0 => Self::new(Vec::new(), Vec::new()),
            1 => Self::new(channels[0].clone(), channels[0].clone()),
            _ => Self::new(channels[0].clone(), channels[1].clone()),
        }
    }
}

impl BlockSource for PreparedBuffer {
    fn len_frames(&self) -> u64 {
        self.left.len() as u64
    }

    fn fill(&self, start_frame: u64, left: &mut [Sample], right: &mut [Sample]) -> usize {
        let n = left.len().min(right.len());
        let start = start_frame as usize;
        let avail = self.left.len().saturating_sub(start);
        let copy = avail.min(n);
        if copy > 0 {
            left[..copy].copy_from_slice(&self.left[start..start + copy]);
            right[..copy].copy_from_slice(&self.right[start..start + copy]);
        }
        // Silence the remainder of the requested block.
        for s in &mut left[copy..n] {
            *s = 0.0;
        }
        for s in &mut right[copy..n] {
            *s = 0.0;
        }
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared() -> PreparedBuffer {
        PreparedBuffer::new(vec![0.1, 0.2, 0.3, 0.4], vec![-0.1, -0.2, -0.3, -0.4])
    }

    #[test]
    fn fills_from_position() {
        let p = prepared();
        let mut l = [0.0; 2];
        let mut r = [0.0; 2];
        let n = p.fill(1, &mut l, &mut r);
        assert_eq!(n, 2);
        assert_eq!(l, [0.2, 0.3]);
        assert_eq!(r, [-0.2, -0.3]);
    }

    #[test]
    fn silences_past_end() {
        let p = prepared();
        let mut l = [9.0; 4];
        let mut r = [9.0; 4];
        let n = p.fill(2, &mut l, &mut r); // only frames 2,3 remain
        assert_eq!(n, 2);
        assert_eq!(l, [0.3, 0.4, 0.0, 0.0]);
        assert_eq!(r, [-0.3, -0.4, 0.0, 0.0]);
    }

    #[test]
    fn fully_past_end_is_silent_and_zero() {
        let p = prepared();
        let mut l = [9.0; 3];
        let mut r = [9.0; 3];
        let n = p.fill(100, &mut l, &mut r);
        assert_eq!(n, 0);
        assert_eq!(l, [0.0; 3]);
        assert_eq!(r, [0.0; 3]);
    }

    #[test]
    fn mono_duplicates_to_both_sides() {
        let p = PreparedBuffer::from_channels(&[vec![0.5, 0.6]]);
        let mut l = [0.0; 2];
        let mut r = [0.0; 2];
        p.fill(0, &mut l, &mut r);
        assert_eq!(l, [0.5, 0.6]);
        assert_eq!(r, [0.5, 0.6]);
    }
}
