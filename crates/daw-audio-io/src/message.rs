//! Real-time command and event messages (see ADR-0007).
//!
//! Small, `Copy` messages cross between the control (UI/CLI) thread and the
//! audio thread over bounded lock-free SPSC rings. Keeping them `Copy` and
//! allocation-free is what makes the audio-side drain real-time safe.

/// Control -> audio commands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// Start playback from the current position.
    Play,
    /// Stop playback (position retained).
    Stop,
    /// Seek to an absolute frame position.
    Seek(u64),
    /// Set the master output gain (linear, clamped to >= 0 on apply).
    SetMasterGain(f32),
}

/// Audio -> control events (meters, position, health).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// A periodic snapshot: current playhead frame and per-side peak levels.
    Meter {
        /// Current playhead position in frames.
        frame: u64,
        /// Peak absolute level of the left channel in the last block.
        peak_l: f32,
        /// Peak absolute level of the right channel in the last block.
        peak_r: f32,
    },
    /// Playback reached the end of the available material and stopped.
    ReachedEnd,
    /// An output underflow/xrun was detected (count since last report).
    Xrun(u32),
}
