//! Integration test: full offline vertical slice.
//!
//! Builds a project, renders it offline through the gain + mixing pipeline,
//! saves it, reloads it, and renders again — asserting the rendered output is
//! identical. This exercises every Milestone 0 subsystem end to end without
//! any audio hardware.

use std::collections::HashMap;

use daw_core::audio::Gain;
use daw_core::project::Project;
use daw_core::time::SampleRate;
use daw_core::track::{Track, TrackKind};
use daw_dsp::buffer::AudioBuffer;
use daw_engine::render::render_block;

fn stereo_const(frames: usize, value: f32) -> AudioBuffer {
    let mut b = AudioBuffer::new(2, frames);
    b.channel_mut(0).fill(value);
    b.channel_mut(1).fill(value);
    b
}

fn build_project() -> (Project, HashMap<daw_core::id::TrackId, AudioBuffer>) {
    let mut project = Project::new("Vertical Slice", SampleRate::new(48_000).unwrap());

    let mut guitar = Track::new(TrackKind::Audio, "Guitar");
    guitar.volume = Gain::linear(0.5).unwrap();
    let guitar_id = project.add_track(guitar);

    let synth_id = project.add_track(Track::new(TrackKind::Midi, "Synth"));

    let mut sources = HashMap::new();
    sources.insert(guitar_id, stereo_const(32, 0.8)); // -> 0.4 after gain
    sources.insert(synth_id, stereo_const(32, 0.1)); // -> 0.1
    (project, sources)
}

#[test]
fn offline_slice_renders_save_reload_renders_identically() {
    let (project, sources) = build_project();

    let before = render_block(&project, 32, &sources).unwrap();
    // 0.4 (guitar after 0.5 gain) + 0.1 (synth) = 0.5
    assert!((before.channel(0)[0] - 0.5).abs() < 1e-6);

    // Persist and reload.
    let json = daw_project::to_json(&project).unwrap();
    let reloaded = daw_project::from_json(&json).unwrap();
    assert_eq!(project, reloaded);

    let after = render_block(&reloaded, 32, &sources).unwrap();
    assert_eq!(before, after);
}

#[test]
fn file_round_trip_then_render() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("slice.json");
    let (project, sources) = build_project();

    daw_project::save(&project, &path).unwrap();
    let loaded = daw_project::load(&path).unwrap();

    let out = render_block(&loaded, 32, &sources).unwrap();
    assert!((out.channel(1)[0] - 0.5).abs() < 1e-6);
}
