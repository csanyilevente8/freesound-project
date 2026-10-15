//! The real-time-safe playback core (see ADR-0007).
//!
//! A [`Player`] lives on the audio side. It owns the transport state and the
//! [`BlockSource`], drains the inbound command ring, produces one block of
//! interleaved stereo output, and pushes meter/status events outbound. It is
//! **device-independent** — the CPAL backend simply calls
//! [`Player::process_interleaved`] from its callback — and therefore fully
//! testable without any hardware.
//!
//! Real-time discipline (enforced here and by tests): `process_interleaved`
//! and the command drain perform no allocation, no locking, and no I/O. All
//! working storage is owned up front; the inbound/outbound rings are bounded
//! (`rtrb`), and the outbound ring drops on full rather than blocking.

use daw_core::audio::Sample;
use rtrb::{Consumer, Producer};

use crate::message::{Command, Event};
use crate::source::BlockSource;

/// Transport run state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Not advancing; output is silence.
    Stopped,
    /// Advancing; output is sourced from the block source.
    Playing,
}

/// The audio-side player. Generic over the block source so tests can inject a
/// fake and production uses a [`crate::source::PreparedBuffer`].
pub struct Player<S: BlockSource> {
    source: S,
    commands: Consumer<Command>,
    events: Producer<Event>,
    transport: Transport,
    position: u64,
    master_gain: Sample,
    // Preallocated planar scratch for de-interleaving the device buffer. Sized
    // to the maximum block the device may request (set in `new`).
    scratch_l: Vec<Sample>,
    scratch_r: Vec<Sample>,
    // Throttle meter events so we don't flood the ring every tiny block.
    frames_since_meter: u64,
    meter_interval: u64,
}

impl<S: BlockSource> Player<S> {
    /// Creates a player. `max_block` is the largest frame count the device may
    /// request in one callback; scratch is preallocated to that size so the
    /// callback never allocates. `meter_interval` is how many frames between
    /// meter events (0 = every block).
    pub fn new(
        source: S,
        commands: Consumer<Command>,
        events: Producer<Event>,
        max_block: usize,
        meter_interval: u64,
    ) -> Self {
        Self {
            source,
            commands,
            events,
            transport: Transport::Stopped,
            position: 0,
            master_gain: 1.0,
            scratch_l: vec![0.0; max_block],
            scratch_r: vec![0.0; max_block],
            meter_interval,
            frames_since_meter: 0,
        }
    }

    /// Current transport state (for tests/inspection).
    #[must_use]
    pub fn transport(&self) -> Transport {
        self.transport
    }

    /// Current playhead position in frames (for tests/inspection).
    #[must_use]
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Drains all pending commands. Real-time safe (bounded by ring capacity).
    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.commands.pop() {
            match cmd {
                Command::Play => self.transport = Transport::Playing,
                Command::Stop => self.transport = Transport::Stopped,
                Command::Seek(frame) => self.position = frame,
                Command::SetMasterGain(g) => self.master_gain = g.max(0.0),
            }
        }
    }

    /// Processes one device block of `frames` frames into `out`, which is
    /// **interleaved** stereo of length `frames * 2` (L,R,L,R,...).
    ///
    /// This is the function the device callback calls. Real-time safe.
    pub fn process_interleaved(&mut self, out: &mut [Sample], frames: usize) {
        self.drain_commands();

        let frames = frames.min(self.scratch_l.len());
        let out = &mut out[..frames * 2];

        if self.transport == Transport::Stopped {
            out.iter_mut().for_each(|s| *s = 0.0);
            return;
        }

        let l = &mut self.scratch_l[..frames];
        let r = &mut self.scratch_r[..frames];
        let produced = self.source.fill(self.position, l, r);

        // Apply master gain and interleave.
        let g = self.master_gain;
        let mut peak_l = 0.0_f32;
        let mut peak_r = 0.0_f32;
        for i in 0..frames {
            let sl = l[i] * g;
            let sr = r[i] * g;
            peak_l = peak_l.max(sl.abs());
            peak_r = peak_r.max(sr.abs());
            out[i * 2] = sl;
            out[i * 2 + 1] = sr;
        }

        self.position += frames as u64;

        // End-of-material: if nothing was produced and we're past the end, stop.
        if produced == 0 && self.position >= self.source.len_frames() {
            self.transport = Transport::Stopped;
            let _ = self.events.push(Event::ReachedEnd);
        }

        // Throttled meter event (drop on full — never block the audio thread).
        self.frames_since_meter += frames as u64;
        if self.frames_since_meter >= self.meter_interval {
            self.frames_since_meter = 0;
            let _ = self.events.push(Event::Meter {
                frame: self.position,
                peak_l,
                peak_r,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::PreparedBuffer;
    use rtrb::RingBuffer;

    fn make(
        source: PreparedBuffer,
        max_block: usize,
    ) -> (Player<PreparedBuffer>, Producer<Command>, Consumer<Event>) {
        let (cmd_tx, cmd_rx) = RingBuffer::<Command>::new(64);
        let (evt_tx, evt_rx) = RingBuffer::<Event>::new(64);
        let player = Player::new(source, cmd_rx, evt_tx, max_block, 0);
        (player, cmd_tx, evt_rx)
    }

    #[test]
    fn stopped_outputs_silence() {
        let (mut p, _cmd, _evt) = make(PreparedBuffer::new(vec![1.0; 8], vec![1.0; 8]), 8);
        let mut out = [9.0; 8]; // 4 frames interleaved
        p.process_interleaved(&mut out, 4);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn play_advances_and_outputs_audio() {
        let (mut p, mut cmd, _evt) = make(
            PreparedBuffer::new(vec![0.5, 0.6, 0.7, 0.8], vec![-0.5, -0.6, -0.7, -0.8]),
            8,
        );
        cmd.push(Command::Play).unwrap();
        let mut out = [0.0; 8]; // 4 frames
        p.process_interleaved(&mut out, 4);
        // Interleaved L,R for frame 0 then frame 1 ...
        assert_eq!(out[0], 0.5);
        assert_eq!(out[1], -0.5);
        assert_eq!(out[2], 0.6);
        assert_eq!(p.position(), 4);
    }

    #[test]
    fn master_gain_scales_output() {
        let (mut p, mut cmd, _evt) = make(PreparedBuffer::new(vec![1.0, 1.0], vec![1.0, 1.0]), 8);
        cmd.push(Command::Play).unwrap();
        cmd.push(Command::SetMasterGain(0.5)).unwrap();
        let mut out = [0.0; 4];
        p.process_interleaved(&mut out, 2);
        assert!(out.iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn seek_sets_position() {
        let (mut p, mut cmd, _evt) = make(
            PreparedBuffer::new(vec![0.0, 1.0, 2.0, 3.0], vec![0.0; 4]),
            8,
        );
        cmd.push(Command::Play).unwrap();
        cmd.push(Command::Seek(2)).unwrap();
        let mut out = [0.0; 2];
        p.process_interleaved(&mut out, 1);
        assert_eq!(out[0], 2.0); // frame 2, left
    }

    #[test]
    fn stop_command_returns_to_silence() {
        let (mut p, mut cmd, _evt) = make(PreparedBuffer::new(vec![1.0; 8], vec![1.0; 8]), 8);
        cmd.push(Command::Play).unwrap();
        let mut out = [0.0; 4];
        p.process_interleaved(&mut out, 2);
        assert!(out.iter().any(|&s| s != 0.0));
        cmd.push(Command::Stop).unwrap();
        let mut out2 = [9.0; 4];
        p.process_interleaved(&mut out2, 2);
        assert!(out2.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn reaching_end_stops_and_emits_event() {
        let (mut p, mut cmd, mut evt) =
            make(PreparedBuffer::new(vec![1.0, 1.0], vec![1.0, 1.0]), 8);
        cmd.push(Command::Play).unwrap();
        // First block consumes all 2 frames.
        let mut out = [0.0; 8];
        p.process_interleaved(&mut out, 4); // request more than available
                                            // Next block is fully past the end -> stop + ReachedEnd.
        let mut out2 = [0.0; 8];
        p.process_interleaved(&mut out2, 4);
        assert_eq!(p.transport(), Transport::Stopped);
        // Drain events; at least one ReachedEnd present.
        let mut saw_end = false;
        while let Ok(e) = evt.pop() {
            if matches!(e, Event::ReachedEnd) {
                saw_end = true;
            }
        }
        assert!(saw_end);
    }

    #[test]
    fn meter_events_report_position_and_peak() {
        let (mut p, mut cmd, mut evt) =
            make(PreparedBuffer::new(vec![0.5, 0.5], vec![0.25, 0.25]), 8);
        cmd.push(Command::Play).unwrap();
        let mut out = [0.0; 4];
        p.process_interleaved(&mut out, 2);
        let mut peak_seen = None;
        while let Ok(e) = evt.pop() {
            if let Event::Meter {
                frame,
                peak_l,
                peak_r,
            } = e
            {
                peak_seen = Some((frame, peak_l, peak_r));
            }
        }
        let (frame, pl, pr) = peak_seen.expect("a meter event");
        assert_eq!(frame, 2);
        assert!((pl - 0.5).abs() < 1e-6);
        assert!((pr - 0.25).abs() < 1e-6);
    }
}
