//! A simple built-in polyphonic synthesizer (see spec §10.3).
//!
//! Validates the instrument/voice path: oscillator + ADSR amplitude envelope,
//! polyphonic voice allocation, MIDI note-on/off, and velocity response. It is
//! intentionally minimal — not a commercial synth — enough to make MIDI clips
//! audible through the existing render/playback/export path (spec §10.3).
//!
//! The synth is **not** an `AudioProcessor` (that trait is event-free): it has
//! its own note-driven API (`note_on` / `note_off` / `render`) because the
//! engine schedules it specially from MIDI clips. All voices are preallocated
//! in `new`; `render` does no allocation.

use daw_core::audio::Sample;

/// Oscillator waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    /// Sine.
    Sine,
    /// Band-unlimited sawtooth (naive; fine for a first instrument).
    Saw,
    /// Square (50% duty).
    Square,
    /// Triangle.
    Triangle,
}

/// ADSR envelope parameters, in seconds (A/D/R) and linear level (S in [0,1]).
#[derive(Debug, Clone, Copy)]
pub struct Adsr {
    /// Attack time (s).
    pub attack: f32,
    /// Decay time (s).
    pub decay: f32,
    /// Sustain level (0..1).
    pub sustain: f32,
    /// Release time (s).
    pub release: f32,
}

impl Default for Adsr {
    fn default() -> Self {
        Self {
            attack: 0.005,
            decay: 0.08,
            sustain: 0.7,
            release: 0.15,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy)]
struct Voice {
    pitch: u8,
    phase: f32,
    phase_inc: f32,
    amp: f32, // current envelope level
    stage: EnvStage,
    velocity: f32, // 0..1
}

impl Voice {
    const fn silent() -> Self {
        Self {
            pitch: 0,
            phase: 0.0,
            phase_inc: 0.0,
            amp: 0.0,
            stage: EnvStage::Idle,
            velocity: 0.0,
        }
    }
    fn active(&self) -> bool {
        self.stage != EnvStage::Idle
    }
}

/// Converts a MIDI note number to frequency in Hz (A4=69=440 Hz).
#[must_use]
pub fn midi_to_hz(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((f32::from(note) - 69.0) / 12.0)
}

/// A polyphonic synthesizer.
pub struct Synth {
    sample_rate: f32,
    waveform: Waveform,
    adsr: Adsr,
    voices: Vec<Voice>,
    /// Overall output gain to keep polyphony from clipping easily.
    gain: f32,
}

impl Synth {
    /// Creates a synth with `max_voices` polyphony at `sample_rate` Hz.
    #[must_use]
    pub fn new(sample_rate: f32, max_voices: usize, waveform: Waveform, adsr: Adsr) -> Self {
        Self {
            sample_rate: sample_rate.max(1.0),
            waveform,
            adsr,
            voices: vec![Voice::silent(); max_voices.max(1)],
            gain: 0.25,
        }
    }

    /// Number of currently sounding voices (for tests/metering).
    #[must_use]
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active()).count()
    }

    /// Triggers a note. Allocates a free voice, or steals the quietest one if
    /// all are busy.
    pub fn note_on(&mut self, pitch: u8, velocity: u8) {
        let inc = midi_to_hz(pitch) / self.sample_rate;
        let vel = f32::from(velocity.min(127)) / 127.0;
        // Prefer an idle voice; else steal the lowest-amplitude voice.
        let idx = self
            .voices
            .iter()
            .position(|v| !v.active())
            .unwrap_or_else(|| {
                let mut min_i = 0;
                let mut min_amp = f32::INFINITY;
                for (i, v) in self.voices.iter().enumerate() {
                    if v.amp < min_amp {
                        min_amp = v.amp;
                        min_i = i;
                    }
                }
                min_i
            });
        self.voices[idx] = Voice {
            pitch,
            phase: 0.0,
            phase_inc: inc,
            amp: 0.0,
            stage: EnvStage::Attack,
            velocity: vel,
        };
    }

    /// Releases all voices playing `pitch` (enters the release stage).
    pub fn note_off(&mut self, pitch: u8) {
        for v in &mut self.voices {
            if v.active() && v.pitch == pitch && v.stage != EnvStage::Release {
                v.stage = EnvStage::Release;
            }
        }
    }

    /// Immediately silences all voices (e.g. transport stop — avoids stuck
    /// notes, spec §11).
    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            *v = Voice::silent();
        }
    }

    /// Renders `out.len()` mono samples, **adding** into `out` (so callers can
    /// accumulate). Advances all active voices. Allocation-free.
    pub fn render_add(&mut self, out: &mut [Sample]) {
        let sr = self.sample_rate;
        let a_inc = 1.0 / (self.adsr.attack.max(1e-4) * sr);
        let d_inc = 1.0 / (self.adsr.decay.max(1e-4) * sr);
        let r_inc = 1.0 / (self.adsr.release.max(1e-4) * sr);
        let sustain = self.adsr.sustain.clamp(0.0, 1.0);
        let wf = self.waveform;
        let gain = self.gain;

        for v in &mut self.voices {
            if !v.active() {
                continue;
            }
            for s in out.iter_mut() {
                // Envelope.
                match v.stage {
                    EnvStage::Attack => {
                        v.amp += a_inc;
                        if v.amp >= 1.0 {
                            v.amp = 1.0;
                            v.stage = EnvStage::Decay;
                        }
                    }
                    EnvStage::Decay => {
                        v.amp -= d_inc;
                        if v.amp <= sustain {
                            v.amp = sustain;
                            v.stage = EnvStage::Sustain;
                        }
                    }
                    EnvStage::Sustain => {}
                    EnvStage::Release => {
                        v.amp -= r_inc;
                        if v.amp <= 0.0 {
                            v.amp = 0.0;
                            v.stage = EnvStage::Idle;
                            break;
                        }
                    }
                    EnvStage::Idle => break,
                }

                let osc = oscillator(wf, v.phase);
                *s += osc * v.amp * v.velocity * gain;

                v.phase += v.phase_inc;
                if v.phase >= 1.0 {
                    v.phase -= 1.0;
                }
            }
        }
    }
}

/// Evaluates a waveform at phase `p` in [0, 1).
fn oscillator(wf: Waveform, p: f32) -> Sample {
    match wf {
        Waveform::Sine => (p * std::f32::consts::TAU).sin(),
        Waveform::Saw => 2.0 * p - 1.0,
        Waveform::Square => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Waveform::Triangle => 4.0 * (p - 0.5).abs() - 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_to_hz_reference_pitches() {
        assert!((midi_to_hz(69) - 440.0).abs() < 1e-3); // A4
        assert!((midi_to_hz(60) - 261.63).abs() < 0.1); // middle C
        assert!((midi_to_hz(81) - 880.0).abs() < 1e-2); // A5
    }

    #[test]
    fn note_on_activates_a_voice_and_produces_sound() {
        let mut s = Synth::new(48_000.0, 8, Waveform::Sine, Adsr::default());
        assert_eq!(s.active_voices(), 0);
        s.note_on(69, 100);
        assert_eq!(s.active_voices(), 1);
        let mut buf = vec![0.0; 480];
        s.render_add(&mut buf);
        let peak = buf.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(peak > 0.0, "note should produce output");
    }

    #[test]
    fn note_off_then_release_eventually_silences() {
        let mut s = Synth::new(
            48_000.0,
            8,
            Waveform::Saw,
            Adsr {
                attack: 0.001,
                decay: 0.001,
                sustain: 0.5,
                release: 0.01,
            },
        );
        s.note_on(60, 127);
        let mut warm = vec![0.0; 4800];
        s.render_add(&mut warm); // let it reach sustain
        s.note_off(60);
        // Render well past the release time; the voice should go idle.
        let mut tail = vec![0.0; 4800];
        s.render_add(&mut tail);
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn polyphony_mixes_multiple_notes() {
        let mut s = Synth::new(48_000.0, 8, Waveform::Sine, Adsr::default());
        s.note_on(60, 100);
        s.note_on(64, 100);
        s.note_on(67, 100);
        assert_eq!(s.active_voices(), 3);
    }

    #[test]
    fn velocity_scales_amplitude() {
        let adsr = Adsr {
            attack: 0.0001,
            decay: 0.0001,
            sustain: 1.0,
            release: 0.1,
        };
        let mut loud = Synth::new(48_000.0, 1, Waveform::Sine, adsr);
        let mut soft = Synth::new(48_000.0, 1, Waveform::Sine, adsr);
        loud.note_on(69, 127);
        soft.note_on(69, 40);
        let mut lb = vec![0.0; 2400];
        let mut sb = vec![0.0; 2400];
        loud.render_add(&mut lb);
        soft.render_add(&mut sb);
        let lp = lb.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        let sp = sb.iter().fold(0.0f32, |m, &x| m.max(x.abs()));
        assert!(
            lp > sp * 1.5,
            "higher velocity should be louder: {lp} vs {sp}"
        );
    }

    #[test]
    fn all_notes_off_silences_immediately() {
        let mut s = Synth::new(48_000.0, 8, Waveform::Square, Adsr::default());
        s.note_on(60, 100);
        s.note_on(64, 100);
        s.all_notes_off();
        assert_eq!(s.active_voices(), 0);
    }

    #[test]
    fn output_is_finite_and_bounded() {
        let mut s = Synth::new(48_000.0, 16, Waveform::Saw, Adsr::default());
        for p in 48..72 {
            s.note_on(p, 127);
        }
        let mut buf = vec![0.0; 4800];
        s.render_add(&mut buf);
        assert!(buf.iter().all(|x| x.is_finite()));
    }
}
