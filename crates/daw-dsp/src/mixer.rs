//! Simple summing mixer (see spec §10.2 item 3).
//!
//! Mixes multiple source buffers into a single destination buffer by addition.
//! No automatic clipping/normalization is applied: summing can exceed the
//! nominal [-1.0, 1.0] range, and the clipping policy is the responsibility of
//! the master stage (spec §9.4). This keeps the mixer predictable and testable.

use crate::buffer::AudioBuffer;

/// Sums `sources` into `dst`. `dst` is cleared first, then each source is
/// added via [`AudioBuffer::add_from`]. Allocation-free.
pub fn mix_into(dst: &mut AudioBuffer, sources: &[AudioBuffer]) {
    dst.clear();
    for src in sources {
        dst.add_from(src);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixing_sums_sources() {
        let mut dst = AudioBuffer::new(2, 4);
        let mut a = AudioBuffer::new(2, 4);
        a.channel_mut(0).fill(0.25);
        a.channel_mut(1).fill(0.1);
        let mut b = AudioBuffer::new(2, 4);
        b.channel_mut(0).fill(0.25);
        b.channel_mut(1).fill(0.2);
        mix_into(&mut dst, &[a, b]);
        assert_eq!(dst.channel(0), &[0.5, 0.5, 0.5, 0.5]);
        assert!(dst.channel(1).iter().all(|&s| (s - 0.3).abs() < 1e-6));
    }

    #[test]
    fn mixing_no_sources_is_silence() {
        let mut dst = AudioBuffer::new(1, 4);
        dst.channel_mut(0).fill(1.0);
        mix_into(&mut dst, &[]);
        assert_eq!(dst.peak(), 0.0);
    }

    #[test]
    fn mixing_can_exceed_unity_without_clipping() {
        let mut dst = AudioBuffer::new(1, 1);
        let mut a = AudioBuffer::new(1, 1);
        a.channel_mut(0)[0] = 0.8;
        let mut b = AudioBuffer::new(1, 1);
        b.channel_mut(0)[0] = 0.8;
        mix_into(&mut dst, &[a, b]);
        assert!((dst.channel(0)[0] - 1.6).abs() < 1e-6);
    }
}
