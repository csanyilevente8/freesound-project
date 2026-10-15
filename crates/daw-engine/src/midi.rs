//! MIDI → audio scheduling (see spec §11, §10.3).
//!
//! Renders a MIDI track's note events through a built-in [`Synth`] into a block
//! buffer, firing note-on/note-off at sample-accurate positions within the
//! block. The synth is **persistent across blocks** (envelope/phase continue),
//! so the caller owns one [`Synth`] per MIDI track and passes it in each block.
//!
//! Timing: `MidiClip.timeline_start` and each `MidiNote.start`/`duration` are in
//! sample frames (see `daw_core::midi`); note positions are absolute on the
//! timeline = `clip.timeline_start + note.start`.

use daw_core::track::Track;
use daw_dsp::buffer::AudioBuffer;
use daw_dsp::synth::Synth;

/// Renders `track`'s MIDI clips for the block `[block_start, block_start+frames)`
/// through `synth`, writing (mono synth output duplicated to all channels) into
/// `dst`. `dst` is cleared first. The synth state carries across calls.
///
/// Note events are applied sample-accurately: the block is split at each
/// note-on/off boundary so envelopes trigger at the right offset.
pub fn schedule_midi_block(
    track: &Track,
    synth: &mut Synth,
    block_start: u64,
    dst: &mut AudioBuffer,
) {
    dst.clear();
    let frames = dst.frames();
    if frames == 0 {
        return;
    }
    let block_end = block_start + frames as u64;

    // Collect (offset_in_block, event) pairs for note-ons and note-offs that
    // fall within this block. offset is relative to block_start.
    // event: (is_on, pitch, velocity)
    let mut events: Vec<(usize, bool, u8, u8)> = Vec::new();
    for clip in &track.midi_clips {
        let clip_start = clip.timeline_start.frames();
        for note in clip.notes() {
            let on_abs = clip_start + note.start.frames();
            let off_abs = on_abs + note.duration.frames();
            // Clamp note end to the clip's end (notes don't sound past the clip).
            let clip_end_abs = clip.timeline_end().frames();
            let off_abs = off_abs.min(clip_end_abs);
            if on_abs >= block_start && on_abs < block_end {
                events.push((
                    (on_abs - block_start) as usize,
                    true,
                    note.pitch,
                    note.velocity,
                ));
            }
            // Note-off fires in the block that contains off_abs (offset <
            // frames), so an off landing exactly on a block boundary is handled
            // by the next block at offset 0 — never dropped.
            if off_abs >= block_start && off_abs < block_end {
                events.push(((off_abs - block_start) as usize, false, note.pitch, 0));
            }
        }
    }
    // Sort by offset so events fire in order; offs before ons at the same offset
    // would be unusual but on-then-off ordering is fine either way here.
    events.sort_by_key(|e| e.0);

    // Render the mono synth into a scratch, then copy to all channels.
    // Walk the block in segments between event boundaries.
    let mut mono = vec![0.0f32; frames];
    let mut cursor = 0usize;
    let mut ev_i = 0usize;
    while cursor < frames {
        // Apply all events at exactly `cursor`.
        while ev_i < events.len() && events[ev_i].0 == cursor {
            let (_, is_on, pitch, vel) = events[ev_i];
            if is_on {
                synth.note_on(pitch, vel);
            } else {
                synth.note_off(pitch);
            }
            ev_i += 1;
        }
        // Next boundary: the next event offset, or end of block.
        let next = if ev_i < events.len() {
            events[ev_i].0.min(frames)
        } else {
            frames
        };
        let seg = &mut mono[cursor..next];
        synth.render_add(seg);
        cursor = next;
    }

    // Copy mono synth output to every output channel.
    let channels = dst.channels();
    for ch in 0..channels {
        dst.channel_mut(ch).copy_from_slice(&mono);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_core::midi::{MidiClip, MidiNote};
    use daw_core::time::SampleTime;
    use daw_core::track::{Track, TrackKind};
    use daw_dsp::synth::{Adsr, Waveform};

    fn synth() -> Synth {
        Synth::new(
            48_000.0,
            8,
            Waveform::Sine,
            Adsr {
                attack: 0.001,
                decay: 0.001,
                sustain: 0.9,
                release: 0.01,
            },
        )
    }

    #[test]
    fn note_in_block_produces_sound_in_its_window() {
        let mut track = Track::new(TrackKind::Midi, "Synth");
        let mut clip = MidiClip::new(SampleTime::ZERO, SampleTime::new(48_000));
        // Note at frame 100, duration 200.
        clip.add_note(MidiNote::new(
            69,
            127,
            SampleTime::new(100),
            SampleTime::new(200),
        ));
        track.add_midi_clip(clip);

        let mut s = synth();
        let mut dst = AudioBuffer::new(2, 1000);
        schedule_midi_block(&track, &mut s, 0, &mut dst);

        // Before the note (0..100): silence.
        assert!(dst.channel(0)[0..100].iter().all(|&x| x == 0.0));
        // During the note: some signal.
        let during: f32 = dst.channel(0)[100..300].iter().map(|x| x.abs()).sum();
        assert!(during > 0.0);
    }

    #[test]
    fn empty_midi_track_is_silent() {
        let track = Track::new(TrackKind::Midi, "Empty");
        let mut s = synth();
        let mut dst = AudioBuffer::new(2, 256);
        schedule_midi_block(&track, &mut s, 0, &mut dst);
        assert_eq!(dst.peak(), 0.0);
    }

    #[test]
    fn note_spanning_block_boundary_continues() {
        // Note from frame 400 for 1000 frames; render two 512-frame blocks and
        // confirm the second block (which the note spans into) has sound.
        let mut track = Track::new(TrackKind::Midi, "Synth");
        let mut clip = MidiClip::new(SampleTime::ZERO, SampleTime::new(48_000));
        clip.add_note(MidiNote::new(
            60,
            100,
            SampleTime::new(400),
            SampleTime::new(1000),
        ));
        track.add_midi_clip(clip);

        let mut s = synth();
        let mut b0 = AudioBuffer::new(2, 512);
        let mut b1 = AudioBuffer::new(2, 512);
        schedule_midi_block(&track, &mut s, 0, &mut b0);
        schedule_midi_block(&track, &mut s, 512, &mut b1);
        // The note (400..1400) spans into block 1 (512..1024) -> sound there.
        assert!(b1.channel(0).iter().map(|x| x.abs()).sum::<f32>() > 0.0);
    }

    #[test]
    fn both_channels_match() {
        let mut track = Track::new(TrackKind::Midi, "Synth");
        let mut clip = MidiClip::new(SampleTime::ZERO, SampleTime::new(4800));
        clip.add_note(MidiNote::new(
            72,
            110,
            SampleTime::new(0),
            SampleTime::new(2000),
        ));
        track.add_midi_clip(clip);
        let mut s = synth();
        let mut dst = AudioBuffer::new(2, 1000);
        schedule_midi_block(&track, &mut s, 0, &mut dst);
        assert_eq!(dst.channel(0), dst.channel(1));
    }
}
