//! The in-memory project domain model (see spec §5.1).
//!
//! This type is UI- and device-independent. Serialization lives in the
//! `daw-project` crate; this crate only defines the data and its validation
//! rules so that `daw-core` has no dependency on a serialization format.

use serde::{Deserialize, Serialize};

use crate::automation::AutomationLane;
use crate::id::{AssetId, ProjectId, TrackId};
use crate::media::MediaAssetRef;
use crate::time::{SampleRate, Tempo, TimeSignature};
use crate::track::{Track, TrackKind};
use crate::validate::{Validate, ValidationError};

/// A music project: the authoritative, UI-independent musical state.
///
/// Milestone 0 contains the minimum structure required to represent a project
/// and drive the offline mixing path. Clips, routing, automation, and media
/// references are deferred to later milestones but have reserved spots in the
/// architecture (spec §5.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// Stable identifier.
    pub id: ProjectId,
    /// User-visible display name.
    pub name: String,
    /// Project sample rate.
    pub sample_rate: SampleRate,
    /// Default tempo.
    pub tempo: Tempo,
    /// Default time signature.
    pub time_signature: TimeSignature,
    /// Ordered tracks and buses. Order is significant for display; references
    /// still use [`TrackId`]. Exactly one [`TrackKind::Master`] must exist.
    pub tracks: Vec<Track>,
    /// Media asset references used by audio clips. Clips reference assets by
    /// [`AssetId`]; the samples live in files on disk (spec §5.3, §12).
    #[serde(default)]
    pub assets: Vec<MediaAssetRef>,
    /// Automation lanes (spec §5.5). Stored at project level; each lane targets
    /// a stable id (track volume/pan or an effect parameter).
    #[serde(default)]
    pub automation: Vec<AutomationLane>,
}

impl Project {
    /// Creates a new, empty project with a single master track.
    #[must_use]
    pub fn new(name: impl Into<String>, sample_rate: SampleRate) -> Self {
        Self {
            id: ProjectId::new(),
            name: name.into(),
            sample_rate,
            tempo: Tempo::default(),
            time_signature: TimeSignature::default(),
            tracks: vec![Track::master()],
            assets: Vec::new(),
            automation: Vec::new(),
        }
    }

    /// Returns a reference to the master track, if present.
    #[must_use]
    pub fn master(&self) -> Option<&Track> {
        self.tracks.iter().find(|t| t.kind == TrackKind::Master)
    }

    /// Looks up a track by its stable id.
    #[must_use]
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// Looks up a track mutably by its stable id.
    #[must_use]
    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// Returns the index of a track by id, if present.
    #[must_use]
    pub fn track_index(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    /// Removes a track by id, returning `(index, track)` if it existed. The
    /// index is preserved so the removal can be undone by re-inserting at the
    /// same position.
    pub fn remove_track(&mut self, id: TrackId) -> Option<(usize, Track)> {
        let idx = self.track_index(id)?;
        Some((idx, self.tracks.remove(idx)))
    }

    /// Inserts a track at a specific index (clamped to the current length).
    pub fn insert_track(&mut self, index: usize, track: Track) {
        let index = index.min(self.tracks.len());
        self.tracks.insert(index, track);
    }

    /// Looks up a media asset by its stable id.
    #[must_use]
    pub fn asset(&self, id: AssetId) -> Option<&MediaAssetRef> {
        self.assets.iter().find(|a| a.id == id)
    }

    /// Adds a media asset and returns its id.
    pub fn add_asset(&mut self, asset: MediaAssetRef) -> AssetId {
        let id = asset.id;
        self.assets.push(asset);
        id
    }

    /// Removes a media asset by id, returning `(index, asset)` if present.
    pub fn remove_asset(&mut self, id: AssetId) -> Option<(usize, MediaAssetRef)> {
        let idx = self.assets.iter().position(|a| a.id == id)?;
        Some((idx, self.assets.remove(idx)))
    }

    /// Inserts a media asset at `index` (clamped to the current length).
    pub fn insert_asset(&mut self, index: usize, asset: MediaAssetRef) {
        let index = index.min(self.assets.len());
        self.assets.insert(index, asset);
    }

    /// Returns the index of the automation lane targeting `target`, if any.
    #[must_use]
    pub fn automation_index(&self, target: &crate::automation::AutomationTarget) -> Option<usize> {
        self.automation.iter().position(|l| &l.target == target)
    }

    /// Gets a mutable automation lane by its target.
    #[must_use]
    pub fn automation_lane_mut(
        &mut self,
        target: &crate::automation::AutomationTarget,
    ) -> Option<&mut AutomationLane> {
        self.automation.iter_mut().find(|l| &l.target == target)
    }

    /// Removes the automation lane at `index`, returning it. Index-based so the
    /// removal can be undone by re-inserting at the same position.
    #[must_use]
    pub fn remove_automation_lane(&mut self, index: usize) -> Option<AutomationLane> {
        if index < self.automation.len() {
            Some(self.automation.remove(index))
        } else {
            None
        }
    }

    /// Inserts an automation lane at `index` (clamped to the current length).
    pub fn insert_automation_lane(&mut self, index: usize, lane: AutomationLane) {
        let index = index.min(self.automation.len());
        self.automation.insert(index, lane);
    }

    /// Returns the indices of automation lanes that target `effect` on `track`
    /// (any parameter). Useful when removing an effect to clean up its lanes.
    #[must_use]
    pub fn automation_lanes_for_effect(
        &self,
        track: TrackId,
        effect: crate::id::EffectId,
    ) -> Vec<usize> {
        use crate::automation::AutomationTarget;
        self.automation
            .iter()
            .enumerate()
            .filter_map(|(i, lane)| match &lane.target {
                AutomationTarget::EffectParam {
                    track: t,
                    effect: e,
                    ..
                } if *t == track && *e == effect => Some(i),
                _ => None,
            })
            .collect()
    }

    /// Adds a track and returns its id. Does not enforce uniqueness of the
    /// master role here; call [`Validate::validate`] to check invariants.
    pub fn add_track(&mut self, track: Track) -> TrackId {
        let id = track.id;
        self.tracks.push(track);
        id
    }
}

impl Validate for Project {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.name.trim().is_empty() {
            return Err(ValidationError::new("project name must not be empty"));
        }

        let master_count = self
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Master)
            .count();
        if master_count != 1 {
            return Err(ValidationError::new(format!(
                "project must have exactly one master track, found {master_count}"
            )));
        }

        // Track ids must be unique.
        let mut seen = std::collections::HashSet::with_capacity(self.tracks.len());
        for track in &self.tracks {
            if !seen.insert(track.id) {
                return Err(ValidationError::new(format!(
                    "duplicate track id {}",
                    track.id
                )));
            }
        }

        // Asset ids must be unique and each asset must be valid.
        let mut asset_ids = std::collections::HashSet::with_capacity(self.assets.len());
        for asset in &self.assets {
            asset.validate()?;
            if !asset_ids.insert(asset.id) {
                return Err(ValidationError::new(format!(
                    "duplicate asset id {}",
                    asset.id
                )));
            }
        }

        // Every clip must be valid and reference an existing asset.
        let mut clip_ids = std::collections::HashSet::new();
        for track in &self.tracks {
            for clip in &track.clips {
                clip.validate()?;
                if !clip_ids.insert(clip.id) {
                    return Err(ValidationError::new(format!(
                        "duplicate clip id {}",
                        clip.id
                    )));
                }
                if !asset_ids.contains(&clip.asset) {
                    return Err(ValidationError::new(format!(
                        "clip {} references missing asset {}",
                        clip.id, clip.asset
                    )));
                }
            }
            for clip in &track.midi_clips {
                clip.validate()?;
                if !clip_ids.insert(clip.id) {
                    return Err(ValidationError::new(format!(
                        "duplicate clip id {}",
                        clip.id
                    )));
                }
            }
        }

        // Automation lanes must be valid and target existing entities.
        for lane in &self.automation {
            lane.validate()?;
            match &lane.target {
                crate::automation::AutomationTarget::TrackVolume(id)
                | crate::automation::AutomationTarget::TrackPan(id) => {
                    if self.track(*id).is_none() {
                        return Err(ValidationError::new(format!(
                            "automation targets missing track {id}"
                        )));
                    }
                }
                crate::automation::AutomationTarget::EffectParam { track, effect, .. } => {
                    let t = self.track(*track).ok_or_else(|| {
                        ValidationError::new(format!("automation targets missing track {track}"))
                    })?;
                    if t.effects.get(*effect).is_none() {
                        return Err(ValidationError::new(format!(
                            "automation targets missing effect {effect}"
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate() -> SampleRate {
        SampleRate::new(48_000).unwrap()
    }

    #[test]
    fn new_project_has_single_master_and_validates() {
        let p = Project::new("Song", rate());
        assert_eq!(p.tracks.len(), 1);
        assert!(p.master().is_some());
        assert!(p.validate().is_ok());
    }

    #[test]
    fn empty_name_is_invalid() {
        let mut p = Project::new("Song", rate());
        p.name = "  ".into();
        assert!(p.validate().is_err());
    }

    #[test]
    fn two_masters_is_invalid() {
        let mut p = Project::new("Song", rate());
        p.add_track(Track::master());
        assert!(p.validate().is_err());
    }

    #[test]
    fn zero_masters_is_invalid() {
        let mut p = Project::new("Song", rate());
        p.tracks.clear();
        assert!(p.validate().is_err());
    }

    #[test]
    fn duplicate_track_id_is_invalid() {
        let mut p = Project::new("Song", rate());
        let dup = Track::new(TrackKind::Audio, "A");
        let id = p.add_track(dup.clone());
        let mut dup2 = Track::new(TrackKind::Audio, "B");
        dup2.id = id;
        p.add_track(dup2);
        assert!(p.validate().is_err());
    }

    #[test]
    fn track_lookup_by_id() {
        let mut p = Project::new("Song", rate());
        let id = p.add_track(Track::new(TrackKind::Audio, "Gtr"));
        assert_eq!(p.track(id).unwrap().name, "Gtr");
    }

    #[test]
    fn clip_referencing_existing_asset_validates() {
        use crate::audio::ChannelLayout;
        use crate::clip::AudioClip;
        use crate::media::MediaAssetRef;
        use crate::time::SampleTime;

        let mut p = Project::new("Song", rate());
        let asset_id = p.add_asset(MediaAssetRef::new(
            "a.wav",
            rate(),
            ChannelLayout::Stereo,
            SampleTime::new(1000),
        ));
        let mut track = Track::new(TrackKind::Audio, "A");
        track.add_clip(AudioClip::new(
            asset_id,
            SampleTime::ZERO,
            SampleTime::new(500),
        ));
        p.add_track(track);
        assert!(p.validate().is_ok());
    }

    #[test]
    fn clip_referencing_missing_asset_is_invalid() {
        use crate::clip::AudioClip;
        use crate::time::SampleTime;

        let mut p = Project::new("Song", rate());
        let mut track = Track::new(TrackKind::Audio, "A");
        track.add_clip(AudioClip::new(
            AssetId::new(), // never added to project
            SampleTime::ZERO,
            SampleTime::new(500),
        ));
        p.add_track(track);
        assert!(p.validate().is_err());
    }

    #[test]
    fn duplicate_asset_id_is_invalid() {
        use crate::audio::ChannelLayout;
        use crate::media::MediaAssetRef;
        use crate::time::SampleTime;

        let mut p = Project::new("Song", rate());
        let a = MediaAssetRef::new("a.wav", rate(), ChannelLayout::Mono, SampleTime::new(10));
        let id = p.add_asset(a.clone());
        let mut b = MediaAssetRef::new("b.wav", rate(), ChannelLayout::Mono, SampleTime::new(10));
        b.id = id;
        p.add_asset(b);
        assert!(p.validate().is_err());
    }
}
