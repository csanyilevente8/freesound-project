//! Integration test: automation affects the offline-rendered output.

use daw_core::automation::{AutomationLane, AutomationPoint, AutomationTarget, Interpolation};
use daw_core::clip::AudioClip;
use daw_core::id::AssetId;
use daw_core::project::Project;
use daw_core::time::{SampleRate, SampleTime};
use daw_core::track::{Track, TrackKind};
use daw_engine::schedule::AssetSource;
use daw_render::{render_project, ExportOptions};

/// Constant full-scale source for every asset.
struct ConstSource;
impl AssetSource for ConstSource {
    fn read(&self, _a: AssetId, _ch: usize, _start: SampleTime, out: &mut [f32]) -> usize {
        out.fill(1.0);
        out.len()
    }
}

#[test]
fn track_volume_automation_fades_output() {
    let rate = SampleRate::new(48_000).unwrap();
    let frames = 1000u64;
    let mut p = Project::new("Fade", rate);
    let asset = p.add_asset(daw_core::media::MediaAssetRef::new(
        "<const>",
        rate,
        daw_core::audio::ChannelLayout::Stereo,
        SampleTime::new(frames),
    ));
    let mut track = Track::new(TrackKind::Audio, "A");
    let track_id = track.id;
    track.add_clip(AudioClip::new(
        asset,
        SampleTime::ZERO,
        SampleTime::new(frames),
    ));
    p.add_track(track);

    // Linear volume automation 0.0 -> 1.0 across the clip.
    let mut lane = AutomationLane::new(
        AutomationTarget::TrackVolume(track_id),
        Interpolation::Linear,
    );
    lane.add_point(AutomationPoint::new(SampleTime::new(0), 0.0));
    lane.add_point(AutomationPoint::new(SampleTime::new(frames - 1), 1.0));
    p.automation.push(lane);

    // Small blocks so per-block automation sampling tracks the ramp closely.
    let out = render_project(
        &p,
        &ConstSource,
        &ExportOptions {
            block_frames: 16,
            ..Default::default()
        },
    )
    .unwrap();

    // Early output should be quiet, late output near full scale.
    let early = out.channel(0)[0].abs();
    let late = out.channel(0)[frames as usize - 1].abs();
    assert!(early < 0.1, "early={early}");
    assert!(late > 0.9, "late={late}");
    assert!(late > early);
}

#[test]
fn step_automation_switches_at_point() {
    let rate = SampleRate::new(48_000).unwrap();
    let frames = 200u64;
    let mut p = Project::new("Step", rate);
    let asset = p.add_asset(daw_core::media::MediaAssetRef::new(
        "<const>",
        rate,
        daw_core::audio::ChannelLayout::Stereo,
        SampleTime::new(frames),
    ));
    let mut track = Track::new(TrackKind::Audio, "A");
    let track_id = track.id;
    track.add_clip(AudioClip::new(
        asset,
        SampleTime::ZERO,
        SampleTime::new(frames),
    ));
    p.add_track(track);

    let mut lane =
        AutomationLane::new(AutomationTarget::TrackVolume(track_id), Interpolation::Step);
    lane.add_point(AutomationPoint::new(SampleTime::new(0), 0.0)); // muted
    lane.add_point(AutomationPoint::new(SampleTime::new(100), 1.0)); // full
    p.automation.push(lane);

    let out = render_project(
        &p,
        &ConstSource,
        &ExportOptions {
            block_frames: 1,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(out.channel(0)[50].abs() < 1e-6); // before the step: muted
    assert!((out.channel(0)[150].abs() - 1.0).abs() < 1e-6); // after: full
}
