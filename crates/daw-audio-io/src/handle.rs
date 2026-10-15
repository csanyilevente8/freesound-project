//! Control-side handle to a running playback stream (see ADR-0007).
//!
//! Lives on the UI/CLI thread. Sends [`Command`]s to the audio thread and polls
//! [`Event`]s back. Sending never blocks the audio thread; if the command ring
//! is full the send is reported as failed so the caller can retry/coalesce.

use rtrb::{Consumer, Producer};

use crate::message::{Command, Event};

/// Control-side endpoint: a command producer and an event consumer.
pub struct PlaybackHandle {
    commands: Producer<Command>,
    events: Consumer<Event>,
}

impl PlaybackHandle {
    /// Wraps the control-side ring endpoints.
    #[must_use]
    pub fn new(commands: Producer<Command>, events: Consumer<Event>) -> Self {
        Self { commands, events }
    }

    /// Sends a command. Returns `false` if the command ring is full (caller may
    /// retry); never blocks the audio thread.
    pub fn send(&mut self, command: Command) -> bool {
        self.commands.push(command).is_ok()
    }

    /// Convenience wrappers.
    pub fn play(&mut self) -> bool {
        self.send(Command::Play)
    }
    /// Stop playback.
    pub fn stop(&mut self) -> bool {
        self.send(Command::Stop)
    }
    /// Seek to an absolute frame.
    pub fn seek(&mut self, frame: u64) -> bool {
        self.send(Command::Seek(frame))
    }
    /// Set master gain (linear).
    pub fn set_master_gain(&mut self, gain: f32) -> bool {
        self.send(Command::SetMasterGain(gain))
    }

    /// Pops the next pending event, if any (non-blocking).
    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop().ok()
    }
}
