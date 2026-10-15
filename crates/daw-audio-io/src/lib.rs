//! `daw-audio-io`: real-time audio device layer and playback engine.
//!
//! Architecture (ADR-0003, ADR-0007):
//! * [`source::BlockSource`] — real-time-safe audio producer; the first slice
//!   uses [`source::PreparedBuffer`] (whole project rendered offline up front).
//! * [`player::Player`] — audio-side core: transport + command drain + meter
//!   events. Device-independent and fully unit-tested without hardware.
//! * [`cpal_backend`] — the only module that touches CPAL; drives a `Player`
//!   from the device callback.
//! * [`handle::PlaybackHandle`] — control-side API to send commands / poll
//!   events over bounded lock-free rings.
//!
//! CPAL and `rtrb` types do not leak into `daw-core`/`daw-engine`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cpal_backend;
pub mod handle;
pub mod message;
pub mod player;
pub mod source;

pub use cpal_backend::{AudioOutput, CpalOutput, DeviceError};
pub use handle::PlaybackHandle;
pub use message::{Command, Event};
pub use player::{Player, Transport};
pub use source::{BlockSource, PreparedBuffer};

use rtrb::RingBuffer;

/// Capacity of the command and event rings. Small, fixed, and bounded.
const RING_CAPACITY: usize = 256;

/// Builds a [`Player`] and its control-side [`PlaybackHandle`] wired together by
/// fresh command/event rings.
///
/// `max_block` is the largest device callback block to preallocate scratch for;
/// `meter_interval` is frames between meter events (e.g. ~1 per 50 ms).
#[must_use]
pub fn playback_channel<S: BlockSource>(
    source: S,
    max_block: usize,
    meter_interval: u64,
) -> (Player<S>, PlaybackHandle) {
    let (cmd_tx, cmd_rx) = RingBuffer::<Command>::new(RING_CAPACITY);
    let (evt_tx, evt_rx) = RingBuffer::<Event>::new(RING_CAPACITY);
    let player = Player::new(source, cmd_rx, evt_tx, max_block, meter_interval);
    let handle = PlaybackHandle::new(cmd_tx, evt_rx);
    (player, handle)
}
