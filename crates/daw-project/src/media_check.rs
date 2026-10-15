//! Missing-media detection and relinking (see spec §6.2, §7.11, §8.7).
//!
//! When a project is reopened, referenced media files may have moved or become
//! inaccessible. These are **pure helper functions** over the project model:
//! they report which asset references do not resolve on disk and apply a
//! relink (path change) by stable [`AssetId`]. The actual relink *workflow*
//! (prompting the user, searching a folder) is an application/UI concern; this
//! layer provides the data and the mutation primitive so that logic is testable
//! without a UI.
//!
//! Detection uses an injectable existence check so tests do not touch the real
//! filesystem.

use daw_core::id::AssetId;
use daw_core::project::Project;

/// A report of a project's media references that do not resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingMedia {
    /// Asset ids whose paths did not resolve, with their recorded paths.
    pub missing: Vec<(AssetId, String)>,
}

impl MissingMedia {
    /// Whether all referenced media resolved (nothing missing).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.missing.is_empty()
    }
}

/// Reports media references whose paths do not satisfy `exists`.
///
/// `exists` is a predicate on the recorded path string (inject
/// `|p| std::path::Path::new(p).exists()` in production; a fake set in tests).
#[must_use]
pub fn find_missing_media<F>(project: &Project, exists: F) -> MissingMedia
where
    F: Fn(&str) -> bool,
{
    let missing = project
        .assets
        .iter()
        .filter(|a| !exists(&a.path))
        .map(|a| (a.id, a.path.clone()))
        .collect();
    MissingMedia { missing }
}

/// Relinks the asset with `id` to a new path, returning `true` if the asset was
/// found and updated. The asset's other metadata (rate/channels/frames) is
/// left unchanged; callers that relink to a materially different file should
/// re-import instead.
pub fn relink_asset(project: &mut Project, id: AssetId, new_path: impl Into<String>) -> bool {
    if let Some(asset) = project.assets.iter_mut().find(|a| a.id == id) {
        asset.path = new_path.into();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_core::audio::ChannelLayout;
    use daw_core::media::MediaAssetRef;
    use daw_core::time::{SampleRate, SampleTime};
    use std::collections::HashSet;

    fn project_with_assets(paths: &[&str]) -> (Project, Vec<AssetId>) {
        let mut p = Project::new("M", SampleRate::new(48_000).unwrap());
        let ids = paths
            .iter()
            .map(|path| {
                p.add_asset(MediaAssetRef::new(
                    *path,
                    SampleRate::new(48_000).unwrap(),
                    ChannelLayout::Stereo,
                    SampleTime::new(100),
                ))
            })
            .collect();
        (p, ids)
    }

    #[test]
    fn all_present_reports_empty() {
        let (p, _) = project_with_assets(&["a.wav", "b.wav"]);
        let report = find_missing_media(&p, |_| true);
        assert!(report.is_empty());
    }

    #[test]
    fn detects_missing_paths() {
        let (p, ids) = project_with_assets(&["a.wav", "b.wav"]);
        let present: HashSet<&str> = ["a.wav"].into_iter().collect();
        let report = find_missing_media(&p, |path| present.contains(path));
        assert_eq!(report.missing.len(), 1);
        assert_eq!(report.missing[0].0, ids[1]);
        assert_eq!(report.missing[0].1, "b.wav");
    }

    #[test]
    fn relink_updates_path() {
        let (mut p, ids) = project_with_assets(&["old.wav"]);
        assert!(relink_asset(&mut p, ids[0], "new/location.wav"));
        assert_eq!(p.asset(ids[0]).unwrap().path, "new/location.wav");
    }

    #[test]
    fn relink_unknown_asset_returns_false() {
        let (mut p, _) = project_with_assets(&["a.wav"]);
        assert!(!relink_asset(&mut p, AssetId::new(), "x.wav"));
    }

    #[test]
    fn relink_then_no_longer_missing() {
        let (mut p, ids) = project_with_assets(&["gone.wav"]);
        let present = |path: &str| path == "found.wav";
        assert_eq!(find_missing_media(&p, present).missing.len(), 1);
        relink_asset(&mut p, ids[0], "found.wav");
        assert!(find_missing_media(&p, present).is_empty());
    }
}
