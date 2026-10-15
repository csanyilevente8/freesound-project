//! Integration tests for the command/undo-redo model.

use daw_commands::commands::{
    AddClip, AddEffect, AddTrack, MoveClip, MoveEffect, RemoveTrack, RenameTrack,
    SetAutomationPoint, SetClipGeometry, SetEffectBypass, SetEffectParam, SetTrackVolume,
};
use daw_commands::{CommandError, History};
use daw_core::audio::{ChannelLayout, Gain};
use daw_core::automation::{AutomationPoint, AutomationTarget, Interpolation};
use daw_core::clip::AudioClip;
use daw_core::effect::{EffectInstance, ParamValue};
use daw_core::id::{AssetId, TrackId};
use daw_core::media::MediaAssetRef;
use daw_core::project::Project;
use daw_core::time::{SampleRate, SampleTime};
use daw_core::track::{Track, TrackKind};

fn project() -> Project {
    Project::new("Cmd", SampleRate::new(48_000).unwrap())
}

fn project_with_asset() -> (Project, AssetId) {
    let mut p = project();
    let id = p.add_asset(MediaAssetRef::new(
        "a.wav",
        SampleRate::new(48_000).unwrap(),
        ChannelLayout::Stereo,
        SampleTime::new(10_000),
    ));
    (p, id)
}

#[test]
fn add_track_then_undo_redo() {
    let mut p = project();
    let mut h = History::new();
    let track = Track::new(TrackKind::Audio, "Gtr");
    let id = track.id;

    h.execute(&mut p, Box::new(AddTrack { track })).unwrap();
    assert!(p.track(id).is_some());
    assert_eq!(h.undo_depth(), 1);
    assert_eq!(h.redo_depth(), 0);

    let label = h.undo(&mut p).unwrap();
    assert_eq!(label, "Add Track \"Gtr\"");
    assert!(p.track(id).is_none());
    assert_eq!(h.undo_depth(), 0);
    assert_eq!(h.redo_depth(), 1);

    h.redo(&mut p).unwrap();
    assert!(p.track(id).is_some());
}

#[test]
fn remove_track_restores_at_original_index() {
    let mut p = project();
    let mut h = History::new();
    // master is index 0; add A (1), B (2), C (3)
    let a = Track::new(TrackKind::Audio, "A");
    let b = Track::new(TrackKind::Audio, "B");
    let c = Track::new(TrackKind::Audio, "C");
    let b_id = b.id;
    h.execute(&mut p, Box::new(AddTrack { track: a })).unwrap();
    h.execute(&mut p, Box::new(AddTrack { track: b })).unwrap();
    h.execute(&mut p, Box::new(AddTrack { track: c })).unwrap();

    h.execute(&mut p, Box::new(RemoveTrack { id: b_id }))
        .unwrap();
    assert_eq!(p.track_index(b_id), None);

    h.undo(&mut p).unwrap(); // undo remove -> B back at index 2
    assert_eq!(p.track_index(b_id), Some(2));
    assert_eq!(p.tracks[2].name, "B");
}

#[test]
fn rename_is_reversible() {
    let mut p = project();
    let mut h = History::new();
    let t = Track::new(TrackKind::Audio, "Original");
    let id = t.id;
    h.execute(&mut p, Box::new(AddTrack { track: t })).unwrap();

    h.execute(
        &mut p,
        Box::new(RenameTrack {
            id,
            name: "Renamed".into(),
        }),
    )
    .unwrap();
    assert_eq!(p.track(id).unwrap().name, "Renamed");

    h.undo(&mut p).unwrap();
    assert_eq!(p.track(id).unwrap().name, "Original");
    h.redo(&mut p).unwrap();
    assert_eq!(p.track(id).unwrap().name, "Renamed");
}

#[test]
fn set_volume_round_trips() {
    let mut p = project();
    let mut h = History::new();
    let t = Track::new(TrackKind::Audio, "A");
    let id = t.id;
    h.execute(&mut p, Box::new(AddTrack { track: t })).unwrap();

    h.execute(
        &mut p,
        Box::new(SetTrackVolume {
            id,
            volume: Gain::linear(0.25).unwrap(),
        }),
    )
    .unwrap();
    assert!((p.track(id).unwrap().volume.factor() - 0.25).abs() < 1e-6);

    h.undo(&mut p).unwrap();
    assert!((p.track(id).unwrap().volume.factor() - 1.0).abs() < 1e-6);
}

#[test]
fn new_command_clears_redo() {
    let mut p = project();
    let mut h = History::new();
    let a = Track::new(TrackKind::Audio, "A");
    h.execute(&mut p, Box::new(AddTrack { track: a })).unwrap();
    h.undo(&mut p).unwrap();
    assert_eq!(h.redo_depth(), 1);

    // Executing a new command discards the redo stack.
    let b = Track::new(TrackKind::Audio, "B");
    h.execute(&mut p, Box::new(AddTrack { track: b })).unwrap();
    assert_eq!(h.redo_depth(), 0);
}

#[test]
fn add_and_move_clip_undo_redo() {
    let (mut p, asset) = project_with_asset();
    let mut h = History::new();
    let track = Track::new(TrackKind::Audio, "A");
    let track_id = track.id;
    h.execute(&mut p, Box::new(AddTrack { track })).unwrap();

    let clip = AudioClip::new(asset, SampleTime::ZERO, SampleTime::new(500));
    let clip_id = clip.id;
    h.execute(
        &mut p,
        Box::new(AddClip {
            track: track_id,
            clip,
        }),
    )
    .unwrap();
    assert_eq!(p.track(track_id).unwrap().clips.len(), 1);

    h.execute(
        &mut p,
        Box::new(MoveClip {
            track: track_id,
            id: clip_id,
            timeline_start: SampleTime::new(1000),
        }),
    )
    .unwrap();
    assert_eq!(
        p.track(track_id).unwrap().clips[0].timeline_start,
        SampleTime::new(1000)
    );

    h.undo(&mut p).unwrap(); // undo move
    assert_eq!(
        p.track(track_id).unwrap().clips[0].timeline_start,
        SampleTime::ZERO
    );
    h.undo(&mut p).unwrap(); // undo add
    assert_eq!(p.track(track_id).unwrap().clips.len(), 0);
}

#[test]
fn set_clip_geometry_reversible() {
    let (mut p, asset) = project_with_asset();
    let mut h = History::new();
    let mut track = Track::new(TrackKind::Audio, "A");
    let clip = AudioClip::new(asset, SampleTime::new(100), SampleTime::new(400));
    let clip_id = clip.id;
    track.add_clip(clip);
    let track_id = p.add_track(track);

    h.execute(
        &mut p,
        Box::new(SetClipGeometry {
            track: track_id,
            id: clip_id,
            timeline_start: SampleTime::new(200),
            source_offset: SampleTime::new(50),
            duration: SampleTime::new(300),
        }),
    )
    .unwrap();
    let c = &p.track(track_id).unwrap().clips[0];
    assert_eq!(c.timeline_start, SampleTime::new(200));
    assert_eq!(c.source_offset, SampleTime::new(50));
    assert_eq!(c.duration, SampleTime::new(300));

    h.undo(&mut p).unwrap();
    let c = &p.track(track_id).unwrap().clips[0];
    assert_eq!(c.timeline_start, SampleTime::new(100));
    assert_eq!(c.source_offset, SampleTime::ZERO);
    assert_eq!(c.duration, SampleTime::new(400));
}

#[test]
fn invalid_command_rolls_back_and_errors() {
    // Removing the master track leaves the project invalid; the history must
    // reject it and restore the master.
    let mut p = project();
    let master_id = p.master().unwrap().id;
    let mut h = History::new();

    let err = h
        .execute(&mut p, Box::new(RemoveTrack { id: master_id }))
        .unwrap_err();
    assert!(matches!(err, CommandError::Validation(_)));
    // Rolled back: master is still present and nothing recorded.
    assert!(p.master().is_some());
    assert_eq!(h.undo_depth(), 0);
}

#[test]
fn missing_entity_errors_without_change() {
    let mut p = project();
    let mut h = History::new();
    let err = h
        .execute(
            &mut p,
            Box::new(RenameTrack {
                id: daw_core::id::TrackId::new(),
                name: "X".into(),
            }),
        )
        .unwrap_err();
    assert!(matches!(err, CommandError::NotFound(_)));
    assert_eq!(h.undo_depth(), 0);
}

#[test]
fn transaction_groups_into_single_undo_unit() {
    let (mut p, asset) = project_with_asset();
    let mut h = History::new();
    let track = Track::new(TrackKind::Audio, "A");
    let track_id = track.id;
    h.execute(&mut p, Box::new(AddTrack { track })).unwrap();

    // Add three clips as one gesture.
    h.transaction(&mut p, "Add 3 Clips", |proj, tx| {
        for i in 0..3u64 {
            let clip = AudioClip::new(asset, SampleTime::new(i * 1000), SampleTime::new(500));
            tx.execute(
                proj,
                Box::new(AddClip {
                    track: track_id,
                    clip,
                }),
            )?;
        }
        Ok(())
    })
    .unwrap();

    assert_eq!(p.track(track_id).unwrap().clips.len(), 3);
    // The AddTrack is one unit; the whole clip gesture is ONE more unit.
    assert_eq!(h.undo_depth(), 2);
    assert_eq!(h.peek_undo(), Some("Add 3 Clips"));

    h.undo(&mut p).unwrap(); // undo the grouped clip gesture
    assert_eq!(p.track(track_id).unwrap().clips.len(), 0);
    assert_eq!(h.undo_depth(), 1); // AddTrack remains

    h.redo(&mut p).unwrap();
    assert_eq!(p.track(track_id).unwrap().clips.len(), 3);
}

#[test]
fn transaction_rolls_back_all_on_failure() {
    let (mut p, asset) = project_with_asset();
    let mut h = History::new();
    let track = Track::new(TrackKind::Audio, "A");
    let track_id = track.id;
    h.execute(&mut p, Box::new(AddTrack { track })).unwrap();

    let result = h.transaction(&mut p, "Partial", |proj, tx| {
        // First clip succeeds.
        tx.execute(
            proj,
            Box::new(AddClip {
                track: track_id,
                clip: AudioClip::new(asset, SampleTime::ZERO, SampleTime::new(100)),
            }),
        )?;
        // Second references a missing track -> error.
        tx.execute(
            proj,
            Box::new(AddClip {
                track: daw_core::id::TrackId::new(),
                clip: AudioClip::new(asset, SampleTime::ZERO, SampleTime::new(100)),
            }),
        )?;
        Ok(())
    });

    assert!(result.is_err());
    // The first clip must have been rolled back; the failed transaction records
    // nothing, so only the earlier AddTrack unit remains.
    assert_eq!(p.track(track_id).unwrap().clips.len(), 0);
    assert_eq!(h.undo_depth(), 1);
}

#[test]
fn undo_with_empty_history_errors() {
    let mut p = project();
    let mut h = History::new();
    assert!(h.undo(&mut p).is_err());
    assert!(h.redo(&mut p).is_err());
}

// --- effect commands ---

fn project_with_track() -> (Project, History, TrackId) {
    let mut p = project();
    let mut h = History::new();
    let t = Track::new(TrackKind::Audio, "A");
    let id = t.id;
    h.execute(&mut p, Box::new(AddTrack { track: t })).unwrap();
    (p, h, id)
}

#[test]
fn add_effect_undo_redo() {
    let (mut p, mut h, track) = project_with_track();
    let effect = EffectInstance::new("distortion").with_param("drive", ParamValue::Float(3.0));
    h.execute(&mut p, Box::new(AddEffect { track, effect }))
        .unwrap();
    assert_eq!(p.track(track).unwrap().effects.len(), 1);

    h.undo(&mut p).unwrap();
    assert_eq!(p.track(track).unwrap().effects.len(), 0);
    h.redo(&mut p).unwrap();
    assert_eq!(p.track(track).unwrap().effects.len(), 1);
    assert_eq!(
        p.track(track).unwrap().effects.effects[0].processor_type,
        "distortion"
    );
}

#[test]
fn reorder_effect_is_reversible() {
    let (mut p, mut h, track) = project_with_track();
    for name in ["a", "b", "c"] {
        h.execute(
            &mut p,
            Box::new(AddEffect {
                track,
                effect: EffectInstance::new(name),
            }),
        )
        .unwrap();
    }
    // Move "a" (index 0) to the end.
    h.execute(
        &mut p,
        Box::new(MoveEffect {
            track,
            from: 0,
            to: 2,
        }),
    )
    .unwrap();
    let order: Vec<_> = p
        .track(track)
        .unwrap()
        .effects
        .effects
        .iter()
        .map(|e| e.processor_type.clone())
        .collect();
    assert_eq!(order, vec!["b", "c", "a"]);

    h.undo(&mut p).unwrap();
    let order: Vec<_> = p
        .track(track)
        .unwrap()
        .effects
        .effects
        .iter()
        .map(|e| e.processor_type.clone())
        .collect();
    assert_eq!(order, vec!["a", "b", "c"]);
}

#[test]
fn bypass_toggle_is_reversible() {
    let (mut p, mut h, track) = project_with_track();
    let effect = EffectInstance::new("gain");
    let effect_id = effect.id;
    h.execute(&mut p, Box::new(AddEffect { track, effect }))
        .unwrap();

    h.execute(
        &mut p,
        Box::new(SetEffectBypass {
            track,
            id: effect_id,
            bypassed: true,
        }),
    )
    .unwrap();
    assert!(
        p.track(track)
            .unwrap()
            .effects
            .get(effect_id)
            .unwrap()
            .bypassed
    );

    h.undo(&mut p).unwrap();
    assert!(
        !p.track(track)
            .unwrap()
            .effects
            .get(effect_id)
            .unwrap()
            .bypassed
    );
}

#[test]
fn set_param_reverts_to_previous_or_removes() {
    let (mut p, mut h, track) = project_with_track();
    let effect = EffectInstance::new("gain").with_param("gain", ParamValue::Float(1.0));
    let effect_id = effect.id;
    h.execute(&mut p, Box::new(AddEffect { track, effect }))
        .unwrap();

    // Change an existing param -> undo restores old value.
    h.execute(
        &mut p,
        Box::new(SetEffectParam {
            track,
            id: effect_id,
            name: "gain".into(),
            value: ParamValue::Float(0.5),
        }),
    )
    .unwrap();
    assert_eq!(
        p.track(track)
            .unwrap()
            .effects
            .get(effect_id)
            .unwrap()
            .param("gain"),
        Some(ParamValue::Float(0.5))
    );
    h.undo(&mut p).unwrap();
    assert_eq!(
        p.track(track)
            .unwrap()
            .effects
            .get(effect_id)
            .unwrap()
            .param("gain"),
        Some(ParamValue::Float(1.0))
    );

    // Add a brand-new param -> undo removes it entirely.
    h.execute(
        &mut p,
        Box::new(SetEffectParam {
            track,
            id: effect_id,
            name: "newparam".into(),
            value: ParamValue::Bool(true),
        }),
    )
    .unwrap();
    assert!(p
        .track(track)
        .unwrap()
        .effects
        .get(effect_id)
        .unwrap()
        .param("newparam")
        .is_some());
    h.undo(&mut p).unwrap();
    assert!(p
        .track(track)
        .unwrap()
        .effects
        .get(effect_id)
        .unwrap()
        .param("newparam")
        .is_none());
}

// --- automation commands ---

#[test]
fn automation_point_add_creates_lane_and_undo_removes_it() {
    let (mut p, mut h, track) = project_with_track();
    let target = AutomationTarget::TrackVolume(track);

    h.execute(
        &mut p,
        Box::new(SetAutomationPoint {
            target: target.clone(),
            interpolation: Interpolation::Linear,
            point: AutomationPoint::new(SampleTime::new(100), 0.5),
        }),
    )
    .unwrap();
    assert_eq!(p.automation.len(), 1);
    assert_eq!(p.automation[0].value_at(SampleTime::new(100)), Some(0.5));

    // Undo removes the point (the lane remains but empty).
    h.undo(&mut p).unwrap();
    assert!(p.automation[0].points().is_empty());

    h.redo(&mut p).unwrap();
    assert_eq!(p.automation[0].value_at(SampleTime::new(100)), Some(0.5));
}

#[test]
fn automation_point_replace_restores_previous_on_undo() {
    let (mut p, mut h, track) = project_with_track();
    let target = AutomationTarget::TrackVolume(track);

    h.execute(
        &mut p,
        Box::new(SetAutomationPoint {
            target: target.clone(),
            interpolation: Interpolation::Linear,
            point: AutomationPoint::new(SampleTime::new(100), 0.5),
        }),
    )
    .unwrap();
    // Replace the value at the same time.
    h.execute(
        &mut p,
        Box::new(SetAutomationPoint {
            target: target.clone(),
            interpolation: Interpolation::Linear,
            point: AutomationPoint::new(SampleTime::new(100), 0.9),
        }),
    )
    .unwrap();
    assert_eq!(p.automation[0].value_at(SampleTime::new(100)), Some(0.9));

    // Undo restores the previous value 0.5 (not removal).
    h.undo(&mut p).unwrap();
    assert_eq!(p.automation[0].value_at(SampleTime::new(100)), Some(0.5));
}

#[test]
fn automation_draw_gesture_is_one_undo_unit() {
    let (mut p, mut h, track) = project_with_track();
    let target = AutomationTarget::TrackVolume(track);

    h.transaction(&mut p, "Draw Automation", |proj, tx| {
        for i in 0..10u64 {
            tx.execute(
                proj,
                Box::new(SetAutomationPoint {
                    target: target.clone(),
                    interpolation: Interpolation::Linear,
                    point: AutomationPoint::new(SampleTime::new(i * 100), i as f32 / 10.0),
                }),
            )?;
        }
        Ok(())
    })
    .unwrap();

    assert_eq!(p.automation[0].points().len(), 10);
    // AddTrack (1 unit) + the whole draw gesture (1 unit) = 2.
    assert_eq!(h.undo_depth(), 2);

    h.undo(&mut p).unwrap(); // undo the entire gesture at once
    assert!(p.automation[0].points().is_empty());
}
