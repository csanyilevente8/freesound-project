//! Concrete editing commands (see spec §13).
//!
//! Each command implements [`Command`]: it validates its precondition, mutates
//! the [`Project`], and returns an inverse command. Commands are small and
//! composable; grouped gestures use [`crate::History::transaction`].
//!
//! Milestone 1 covers the core arrangement actions needed before an editor UI:
//! add/remove/rename track, set track volume, mute/solo, and
//! add/remove/move/trim clips. More commands are added as features arrive.

use daw_core::audio::Gain;
use daw_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use daw_core::clip::AudioClip;
use daw_core::effect::{EffectInstance, ParamValue};
use daw_core::id::{AssetId, ClipId, EffectId, TrackId};
use daw_core::media::MediaAssetRef;
use daw_core::project::Project;
use daw_core::time::SampleTime;
use daw_core::track::Track;

use crate::command::Command;
use crate::error::CommandError;

fn not_found(what: &str, id: impl std::fmt::Display) -> CommandError {
    CommandError::NotFound(format!("{what} {id}"))
}

// ---------------------------------------------------------------------------
// Track commands
// ---------------------------------------------------------------------------

/// Adds a track to the project.
pub struct AddTrack {
    /// The track to add. Cloned on apply so the command can be re-applied (redo).
    pub track: Track,
}

impl Command for AddTrack {
    fn label(&self) -> String {
        format!("Add Track \"{}\"", self.track.name)
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let id = self.track.id;
        project.add_track(self.track.clone());
        Ok(Box::new(RemoveTrack { id }))
    }
}

/// Removes a track by id.
pub struct RemoveTrack {
    /// Id of the track to remove.
    pub id: TrackId,
}

impl Command for RemoveTrack {
    fn label(&self) -> String {
        "Remove Track".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let (index, track) = project
            .remove_track(self.id)
            .ok_or_else(|| not_found("track", self.id))?;
        Ok(Box::new(InsertTrackAt { index, track }))
    }
}

/// Re-inserts a previously removed track at its original index (undo of remove).
pub struct InsertTrackAt {
    /// Original index.
    pub index: usize,
    /// The track to re-insert.
    pub track: Track,
}

impl Command for InsertTrackAt {
    fn label(&self) -> String {
        format!("Insert Track \"{}\"", self.track.name)
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let id = self.track.id;
        project.insert_track(self.index, self.track.clone());
        Ok(Box::new(RemoveTrack { id }))
    }
}

/// Renames a track.
pub struct RenameTrack {
    /// Target track.
    pub id: TrackId,
    /// New name.
    pub name: String,
}

impl Command for RenameTrack {
    fn label(&self) -> String {
        "Rename Track".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let track = project
            .track_mut(self.id)
            .ok_or_else(|| not_found("track", self.id))?;
        let previous = std::mem::replace(&mut track.name, self.name.clone());
        Ok(Box::new(RenameTrack {
            id: self.id,
            name: previous,
        }))
    }
}

/// Sets a track's linear volume.
pub struct SetTrackVolume {
    /// Target track.
    pub id: TrackId,
    /// New linear gain.
    pub volume: Gain,
}

impl Command for SetTrackVolume {
    fn label(&self) -> String {
        "Set Track Volume".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let track = project
            .track_mut(self.id)
            .ok_or_else(|| not_found("track", self.id))?;
        let previous = std::mem::replace(&mut track.volume, self.volume);
        Ok(Box::new(SetTrackVolume {
            id: self.id,
            volume: previous,
        }))
    }
}

/// Sets a track's mute state.
pub struct SetTrackMute {
    /// Target track.
    pub id: TrackId,
    /// New mute state.
    pub mute: bool,
}

impl Command for SetTrackMute {
    fn label(&self) -> String {
        "Set Mute".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let track = project
            .track_mut(self.id)
            .ok_or_else(|| not_found("track", self.id))?;
        let previous = std::mem::replace(&mut track.mute, self.mute);
        Ok(Box::new(SetTrackMute {
            id: self.id,
            mute: previous,
        }))
    }
}

/// Sets a track's solo state.
pub struct SetTrackSolo {
    /// Target track.
    pub id: TrackId,
    /// New solo state.
    pub solo: bool,
}

impl Command for SetTrackSolo {
    fn label(&self) -> String {
        "Set Solo".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let track = project
            .track_mut(self.id)
            .ok_or_else(|| not_found("track", self.id))?;
        let previous = std::mem::replace(&mut track.solo, self.solo);
        Ok(Box::new(SetTrackSolo {
            id: self.id,
            solo: previous,
        }))
    }
}

/// Sets the project tempo.
pub struct SetTempo {
    /// New tempo.
    pub tempo: daw_core::time::Tempo,
}

impl Command for SetTempo {
    fn label(&self) -> String {
        "Set Tempo".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let previous = std::mem::replace(&mut project.tempo, self.tempo);
        Ok(Box::new(SetTempo { tempo: previous }))
    }
}

/// Sets the project time signature.
pub struct SetTimeSignature {
    /// New time signature.
    pub time_signature: daw_core::time::TimeSignature,
}

impl Command for SetTimeSignature {
    fn label(&self) -> String {
        "Set Time Signature".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let previous = std::mem::replace(&mut project.time_signature, self.time_signature);
        Ok(Box::new(SetTimeSignature {
            time_signature: previous,
        }))
    }
}

// ---------------------------------------------------------------------------
// Asset commands
// ---------------------------------------------------------------------------

/// Adds a media-asset reference to the project (e.g. after recording/import).
pub struct AddAsset {
    /// The asset reference to add.
    pub asset: MediaAssetRef,
}

impl Command for AddAsset {
    fn label(&self) -> String {
        "Add Asset".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let id = self.asset.id;
        project.add_asset(self.asset.clone());
        Ok(Box::new(RemoveAsset { id }))
    }
}

/// Removes a media-asset reference by id. Note: this removes the *reference*,
/// never the underlying media file (recording-safe undo, spec §13).
pub struct RemoveAsset {
    /// Asset id to remove.
    pub id: AssetId,
}

impl Command for RemoveAsset {
    fn label(&self) -> String {
        "Remove Asset".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let (index, asset) = project
            .remove_asset(self.id)
            .ok_or_else(|| not_found("asset", self.id))?;
        Ok(Box::new(InsertAssetAt { index, asset }))
    }
}

/// Re-inserts a removed asset at its original index (inverse of remove).
pub struct InsertAssetAt {
    /// Index to insert at.
    pub index: usize,
    /// The asset reference.
    pub asset: MediaAssetRef,
}

impl Command for InsertAssetAt {
    fn label(&self) -> String {
        "Insert Asset".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let id = self.asset.id;
        project.insert_asset(self.index, self.asset.clone());
        Ok(Box::new(RemoveAsset { id }))
    }
}

// ---------------------------------------------------------------------------
// Clip commands
// ---------------------------------------------------------------------------

/// Adds a clip to a track.
pub struct AddClip {
    /// Track to add the clip to.
    pub track: TrackId,
    /// The clip to add.
    pub clip: AudioClip,
}

impl Command for AddClip {
    fn label(&self) -> String {
        "Add Clip".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let clip_id = self.clip.id;
        let track = project
            .track_mut(self.track)
            .ok_or_else(|| not_found("track", self.track))?;
        track.add_clip(self.clip.clone());
        Ok(Box::new(RemoveClip {
            track: self.track,
            id: clip_id,
        }))
    }
}

/// Removes a clip from a track by id.
pub struct RemoveClip {
    /// Track containing the clip.
    pub track: TrackId,
    /// Clip id.
    pub id: ClipId,
}

impl Command for RemoveClip {
    fn label(&self) -> String {
        "Remove Clip".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let track = project
            .track_mut(self.track)
            .ok_or_else(|| not_found("track", self.track))?;
        let (index, clip) = track
            .remove_clip(self.id)
            .ok_or_else(|| not_found("clip", self.id))?;
        Ok(Box::new(InsertClipAt {
            track: self.track,
            index,
            clip,
        }))
    }
}

/// Re-inserts a previously removed clip at its original index (undo of remove).
pub struct InsertClipAt {
    /// Track to insert into.
    pub track: TrackId,
    /// Original index.
    pub index: usize,
    /// The clip.
    pub clip: AudioClip,
}

impl Command for InsertClipAt {
    fn label(&self) -> String {
        "Insert Clip".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let clip_id = self.clip.id;
        let track = project
            .track_mut(self.track)
            .ok_or_else(|| not_found("track", self.track))?;
        track.insert_clip(self.index, self.clip.clone());
        Ok(Box::new(RemoveClip {
            track: self.track,
            id: clip_id,
        }))
    }
}

/// Moves a clip to a new timeline start position.
pub struct MoveClip {
    /// Track containing the clip.
    pub track: TrackId,
    /// Clip to move.
    pub id: ClipId,
    /// New timeline start.
    pub timeline_start: SampleTime,
}

impl Command for MoveClip {
    fn label(&self) -> String {
        "Move Clip".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let track = project
            .track_mut(self.track)
            .ok_or_else(|| not_found("track", self.track))?;
        let clip = track
            .clip_mut(self.id)
            .ok_or_else(|| not_found("clip", self.id))?;
        let previous = std::mem::replace(&mut clip.timeline_start, self.timeline_start);
        Ok(Box::new(MoveClip {
            track: self.track,
            id: self.id,
            timeline_start: previous,
        }))
    }
}

/// Sets a clip's full geometry (position/offset/duration). Used for trim/split
/// results and any precise edit; its inverse restores the previous geometry.
pub struct SetClipGeometry {
    /// Track containing the clip.
    pub track: TrackId,
    /// Clip to edit.
    pub id: ClipId,
    /// New timeline start.
    pub timeline_start: SampleTime,
    /// New source offset.
    pub source_offset: SampleTime,
    /// New duration.
    pub duration: SampleTime,
}

impl Command for SetClipGeometry {
    fn label(&self) -> String {
        "Edit Clip".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let track = project
            .track_mut(self.track)
            .ok_or_else(|| not_found("track", self.track))?;
        let clip = track
            .clip_mut(self.id)
            .ok_or_else(|| not_found("clip", self.id))?;
        let inverse = SetClipGeometry {
            track: self.track,
            id: self.id,
            timeline_start: clip.timeline_start,
            source_offset: clip.source_offset,
            duration: clip.duration,
        };
        clip.timeline_start = self.timeline_start;
        clip.source_offset = self.source_offset;
        clip.duration = self.duration;
        Ok(Box::new(inverse))
    }
}

// ---------------------------------------------------------------------------
// Effect commands
// ---------------------------------------------------------------------------

/// Adds an effect to a track's insert chain (appended at the end).
pub struct AddEffect {
    /// Track to add the effect to.
    pub track: TrackId,
    /// The effect descriptor.
    pub effect: EffectInstance,
}

impl Command for AddEffect {
    fn label(&self) -> String {
        format!("Add Effect \"{}\"", self.effect.processor_type)
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let effect_id = self.effect.id;
        let track = project
            .track_mut(self.track)
            .ok_or_else(|| not_found("track", self.track))?;
        track.effects.push(self.effect.clone());
        Ok(Box::new(RemoveEffect {
            track: self.track,
            id: effect_id,
        }))
    }
}

/// Removes an effect from a track's insert chain by id.
pub struct RemoveEffect {
    /// Track containing the effect.
    pub track: TrackId,
    /// Effect id.
    pub id: EffectId,
}

impl Command for RemoveEffect {
    fn label(&self) -> String {
        "Remove Effect".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let track = project
            .track_mut(self.track)
            .ok_or_else(|| not_found("track", self.track))?;
        let (index, effect) = track
            .effects
            .remove(self.id)
            .ok_or_else(|| not_found("effect", self.id))?;
        Ok(Box::new(InsertEffectAt {
            track: self.track,
            index,
            effect,
        }))
    }
}

/// Re-inserts an effect at a specific index (undo of remove / reorder).
pub struct InsertEffectAt {
    /// Track to insert into.
    pub track: TrackId,
    /// Index to insert at.
    pub index: usize,
    /// The effect descriptor.
    pub effect: EffectInstance,
}

impl Command for InsertEffectAt {
    fn label(&self) -> String {
        "Insert Effect".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let effect_id = self.effect.id;
        let track = project
            .track_mut(self.track)
            .ok_or_else(|| not_found("track", self.track))?;
        track.effects.insert(self.index, self.effect.clone());
        Ok(Box::new(RemoveEffect {
            track: self.track,
            id: effect_id,
        }))
    }
}

/// Moves an effect within a chain from one index to another (reorder).
pub struct MoveEffect {
    /// Track containing the chain.
    pub track: TrackId,
    /// Current index.
    pub from: usize,
    /// Destination index.
    pub to: usize,
}

impl Command for MoveEffect {
    fn label(&self) -> String {
        "Reorder Effect".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let track = project
            .track_mut(self.track)
            .ok_or_else(|| not_found("track", self.track))?;
        if self.from >= track.effects.len() {
            return Err(CommandError::Invalid(format!(
                "effect index {} out of range",
                self.from
            )));
        }
        track.effects.move_effect(self.from, self.to);
        // The inverse moves it back from `to` to `from`.
        Ok(Box::new(MoveEffect {
            track: self.track,
            from: self.to,
            to: self.from,
        }))
    }
}

/// Sets an effect's bypass state.
pub struct SetEffectBypass {
    /// Track containing the effect.
    pub track: TrackId,
    /// Effect id.
    pub id: EffectId,
    /// New bypass state.
    pub bypassed: bool,
}

impl Command for SetEffectBypass {
    fn label(&self) -> String {
        "Set Effect Bypass".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let track = project
            .track_mut(self.track)
            .ok_or_else(|| not_found("track", self.track))?;
        let effect = track
            .effects
            .get_mut(self.id)
            .ok_or_else(|| not_found("effect", self.id))?;
        let previous = std::mem::replace(&mut effect.bypassed, self.bypassed);
        Ok(Box::new(SetEffectBypass {
            track: self.track,
            id: self.id,
            bypassed: previous,
        }))
    }
}

/// Sets a single effect parameter value.
pub struct SetEffectParam {
    /// Track containing the effect.
    pub track: TrackId,
    /// Effect id.
    pub id: EffectId,
    /// Parameter name.
    pub name: String,
    /// New value.
    pub value: ParamValue,
}

impl Command for SetEffectParam {
    fn label(&self) -> String {
        "Set Effect Parameter".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let track = project
            .track_mut(self.track)
            .ok_or_else(|| not_found("track", self.track))?;
        let effect = track
            .effects
            .get_mut(self.id)
            .ok_or_else(|| not_found("effect", self.id))?;
        let previous = effect.set_param(self.name.clone(), self.value);
        // Inverse restores the previous value, or removes the key if it was new.
        match previous {
            Some(prev) => Ok(Box::new(SetEffectParam {
                track: self.track,
                id: self.id,
                name: self.name.clone(),
                value: prev,
            })),
            None => Ok(Box::new(RemoveEffectParam {
                track: self.track,
                id: self.id,
                name: self.name.clone(),
            })),
        }
    }
}

/// Removes an effect parameter (inverse of adding a brand-new parameter).
pub struct RemoveEffectParam {
    /// Track containing the effect.
    pub track: TrackId,
    /// Effect id.
    pub id: EffectId,
    /// Parameter name to remove.
    pub name: String,
}

impl Command for RemoveEffectParam {
    fn label(&self) -> String {
        "Remove Effect Parameter".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let track = project
            .track_mut(self.track)
            .ok_or_else(|| not_found("track", self.track))?;
        let effect = track
            .effects
            .get_mut(self.id)
            .ok_or_else(|| not_found("effect", self.id))?;
        let previous = effect.params.remove(&self.name);
        match previous {
            Some(prev) => Ok(Box::new(SetEffectParam {
                track: self.track,
                id: self.id,
                name: self.name.clone(),
                value: prev,
            })),
            None => Ok(Box::new(RemoveEffectParam {
                track: self.track,
                id: self.id,
                name: self.name.clone(),
            })),
        }
    }
}

// ---------------------------------------------------------------------------
// Automation commands
// ---------------------------------------------------------------------------

/// Sets (adds or replaces) an automation point for a target, creating the lane
/// if it does not exist. This is the command an automation-draw gesture issues
/// repeatedly inside a [`crate::History::transaction`].
pub struct SetAutomationPoint {
    /// The automation target (identifies/creates the lane).
    pub target: AutomationTarget,
    /// Interpolation to use if the lane is newly created.
    pub interpolation: daw_core::automation::Interpolation,
    /// The point to set.
    pub point: AutomationPoint,
}

impl Command for SetAutomationPoint {
    fn label(&self) -> String {
        "Edit Automation".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        // Find or create the lane for this target.
        if project.automation_index(&self.target).is_none() {
            project
                .automation
                .push(AutomationLane::new(self.target.clone(), self.interpolation));
        }
        let lane = project
            .automation_lane_mut(&self.target)
            .expect("lane exists after ensure");

        // Capture the previous point at this time (if any) for the inverse.
        let previous = lane
            .points()
            .iter()
            .find(|p| p.time == self.point.time)
            .copied();
        lane.add_point(self.point);

        match previous {
            Some(prev) => Ok(Box::new(SetAutomationPoint {
                target: self.target.clone(),
                interpolation: self.interpolation,
                point: prev,
            })),
            None => Ok(Box::new(RemoveAutomationPoint {
                target: self.target.clone(),
                time: self.point.time,
            })),
        }
    }
}

/// Removes an automation point at a specific time (inverse of adding a new
/// point). If removing the last point leaves the lane empty, the lane is kept
/// (empty lanes are harmless and avoid churn); callers can remove lanes
/// explicitly if desired.
pub struct RemoveAutomationPoint {
    /// The automation target identifying the lane.
    pub target: AutomationTarget,
    /// Time of the point to remove.
    pub time: SampleTime,
}

impl Command for RemoveAutomationPoint {
    fn label(&self) -> String {
        "Remove Automation Point".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let interpolation = project
            .automation_lane_mut(&self.target)
            .map(|l| l.interpolation);
        let removed = project
            .automation_lane_mut(&self.target)
            .and_then(|l| l.remove_point_at(self.time));

        match (removed, interpolation) {
            (Some(point), Some(interpolation)) => Ok(Box::new(SetAutomationPoint {
                target: self.target.clone(),
                interpolation,
                point,
            })),
            _ => Err(not_found("automation point at time", self.time.frames())),
        }
    }
}

/// Removes an entire automation lane (by target), e.g. when deleting the effect
/// it controls. The inverse re-inserts the exact lane at its original index.
pub struct RemoveAutomationLane {
    /// Target identifying the lane to remove.
    pub target: AutomationTarget,
}

impl Command for RemoveAutomationLane {
    fn label(&self) -> String {
        "Remove Automation Lane".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let index = project
            .automation_index(&self.target)
            .ok_or_else(|| not_found("automation lane", "for target"))?;
        let lane = project
            .remove_automation_lane(index)
            .ok_or_else(|| not_found("automation lane", index))?;
        Ok(Box::new(InsertAutomationLaneAt { index, lane }))
    }
}

/// Re-inserts a previously removed automation lane at a specific index
/// (inverse of [`RemoveAutomationLane`]).
pub struct InsertAutomationLaneAt {
    /// Index to insert at.
    pub index: usize,
    /// The lane to restore (preserves its points and interpolation).
    pub lane: AutomationLane,
}

impl Command for InsertAutomationLaneAt {
    fn label(&self) -> String {
        "Insert Automation Lane".to_string()
    }

    fn apply(&self, project: &mut Project) -> Result<Box<dyn Command>, CommandError> {
        let target = self.lane.target.clone();
        project.insert_automation_lane(self.index, self.lane.clone());
        Ok(Box::new(RemoveAutomationLane { target }))
    }
}
