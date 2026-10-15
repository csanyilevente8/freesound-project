//! Integration test: a MIDI clip renders to audio through the built-in synth.

use daw_core::midi::{MidiClip, MidiNote};
use daw_core::project::Project;
use daw_core::time::{SampleRate, SampleTime};
use daw_core::track::{Track, TrackKind};
use daw_engine::schedule::AssetSource;
use daw_render::{export_wav, render_project, ExportOptions, WavFormat};

/// MIDI tracks need no asset source; this stub supplies none.
struct NoSource;
impl AssetSource for NoSource {
    fn read(
        &self,
        _a: daw_core::id::AssetId,
        _ch: usize,
        _start: SampleTime,
        _out: &mut [f32],
    ) -> usize {
        0
    }
}

fn midi_project() -> Project {
    let rate = SampleRate::new(48_000).unwrap();
    let mut p = Project::new("MIDI", rate);
    let mut track = Track::new(TrackKind::Midi, "Synth");
    let mut clip = MidiClip::new(SampleTime::ZERO, SampleTime::new(48_000)); // 1s
                                                                             // A short C-major chord near the start.
    clip.add_note(MidiNote::new(
        60,
        110,
        SampleTime::new(0),
        SampleTime::new(24_000),
    ));
    clip.add_note(MidiNote::new(
        64,
        100,
        SampleTime::new(0),
        SampleTime::new(24_000),
    ));
    clip.add_note(MidiNote::new(
        67,
        100,
        SampleTime::new(0),
        SampleTime::new(24_000),
    ));
    track.add_midi_clip(clip);
    p.add_track(track);
    p
}

#[test]
fn midi_clip_renders_non_silent_audio() {
    let p = midi_project();
    let out = render_project(&p, &NoSource, &ExportOptions::default()).unwrap();
    assert_eq!(out.frames(), 48_000);
    // The chord region should be audible.
    let energy: f32 = out.channel(0)[0..24_000].iter().map(|x| x.abs()).sum();
    assert!(energy > 0.0, "MIDI chord should produce sound");
    assert!(out.channel(0).iter().all(|x| x.is_finite()));
}

#[test]
fn midi_renders_identically_across_block_sizes() {
    // Envelope/phase continuity: the result must be independent of block size.
    let p = midi_project();
    let a = render_project(
        &p,
        &NoSource,
        &ExportOptions {
            block_frames: 64,
            ..Default::default()
        },
    )
    .unwrap();
    let b = render_project(
        &p,
        &NoSource,
        &ExportOptions {
            block_frames: 1024,
            ..Default::default()
        },
    )
    .unwrap();
    // Compare with a small tolerance (float accumulation order can differ
    // slightly at block boundaries).
    let mut max_diff = 0.0f32;
    for (x, y) in a.channel(0).iter().zip(b.channel(0).iter()) {
        max_diff = max_diff.max((x - y).abs());
    }
    assert!(
        max_diff < 1e-4,
        "block size should not change output: {max_diff}"
    );
}

#[test]
fn midi_project_exports_a_wav() {
    let p = midi_project();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("midi.wav");
    export_wav(
        &p,
        &NoSource,
        &path,
        &ExportOptions {
            format: WavFormat::Float32,
            ..Default::default()
        },
    )
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[0..4], b"RIFF");
    // 1s stereo float32 = 44 + 48000*2*4.
    assert_eq!(bytes.len(), 44 + 48_000 * 2 * 4);
}
