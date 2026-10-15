//! Save and load a project to/from a single versioned JSON file.
//!
//! Milestone 0 uses a single `project.json` file rather than the full
//! directory-based layout in spec §6.1; the directory layout (media/, peaks/,
//! autosave/, recovery/) is deferred until there is media to put in it. The
//! document format and versioning are already in place so the move to a
//! directory bundle later is additive.
//!
//! Saves are **atomic**: the document is written to a temporary file in the
//! same directory and then renamed over the target, so an interrupted save
//! cannot corrupt the previous good file (spec §6.2).

use std::fs;
use std::io::Write;
use std::path::Path;

use daw_core::project::Project;
use daw_core::validate::Validate;

use crate::document::{ProjectDocument, FORMAT_MAGIC};
use crate::error::ProjectError;
use crate::migrate::migrate_to_current;

/// Serializes a project to a pretty-printed JSON string (current schema).
///
/// # Errors
/// Returns [`ProjectError::Serde`] on serialization failure.
pub fn to_json(project: &Project) -> Result<String, ProjectError> {
    let doc = ProjectDocument::new(project.clone());
    Ok(serde_json::to_string_pretty(&doc)?)
}

/// Parses a project from a JSON string, validating magic, version, migrating,
/// and checking domain invariants.
///
/// # Errors
/// Returns [`ProjectError`] on malformed JSON, bad magic, unsupported version,
/// or invariant violation.
pub fn from_json(json: &str) -> Result<Project, ProjectError> {
    let doc: ProjectDocument = serde_json::from_str(json)?;

    if doc.format != FORMAT_MAGIC {
        return Err(ProjectError::BadMagic { found: doc.format });
    }

    let doc = migrate_to_current(doc)?;
    doc.project.validate()?;
    Ok(doc.project)
}

/// Saves a project to `path` atomically (write-temp-then-rename).
///
/// The project is validated before writing so an invalid project is never
/// persisted.
///
/// # Errors
/// Returns [`ProjectError`] on validation failure, serialization failure, or
/// any filesystem error.
pub fn save(project: &Project, path: impl AsRef<Path>) -> Result<(), ProjectError> {
    project.validate()?;
    let path = path.as_ref();
    let json = to_json(project)?;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;

    // Unique-ish temp name in the same directory to keep the rename atomic on
    // the same filesystem.
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project.json");
    let tmp_path = dir.join(format!(".{file_name}.tmp"));

    {
        let mut tmp = fs::File::create(&tmp_path)?;
        tmp.write_all(json.as_bytes())?;
        tmp.flush()?;
        tmp.sync_all()?;
    }

    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Loads a project from `path`.
///
/// # Errors
/// Returns [`ProjectError`] on filesystem error or any parse/validation error
/// from [`from_json`].
pub fn load(path: impl AsRef<Path>) -> Result<Project, ProjectError> {
    let json = fs::read_to_string(path)?;
    from_json(&json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_core::time::SampleRate;
    use daw_core::track::{Track, TrackKind};

    fn sample_project() -> Project {
        let mut p = Project::new("Round Trip", SampleRate::new(44_100).unwrap());
        p.add_track(Track::new(TrackKind::Audio, "Guitar"));
        p.add_track(Track::new(TrackKind::Midi, "Synth"));
        p
    }

    #[test]
    fn json_round_trip_preserves_project() {
        let p = sample_project();
        let json = to_json(&p).unwrap();
        let back = from_json(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn bad_magic_is_rejected() {
        let p = sample_project();
        let json = to_json(&p).unwrap();
        let tampered = json.replace("daw-project", "something-else");
        assert!(matches!(
            from_json(&tampered),
            Err(ProjectError::BadMagic { .. })
        ));
    }

    #[test]
    fn future_version_is_rejected() {
        let p = sample_project();
        let json = to_json(&p).unwrap();
        let tampered = json.replace("\"schema_version\": 5", "\"schema_version\": 9999");
        assert!(matches!(
            from_json(&tampered),
            Err(ProjectError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn legacy_v1_document_loads_and_upgrades() {
        // A hand-written v1 document: no `assets`, tracks without `clips`.
        // Exercises forward-compat defaults + the v1->v2 migration.
        let v1 = r#"{
            "format": "daw-project",
            "schema_version": 1,
            "project": {
                "id": "11111111-1111-4111-8111-111111111111",
                "name": "Legacy",
                "sample_rate": 44100,
                "tempo": 120.0,
                "time_signature": { "numerator": 4, "denominator": 4 },
                "tracks": [
                    { "id": "22222222-2222-4222-8222-222222222222",
                      "kind": "master", "name": "Master",
                      "volume": 1.0, "mute": false, "solo": false }
                ]
            }
        }"#;
        let p = from_json(v1).unwrap();
        assert_eq!(p.name, "Legacy");
        assert!(p.assets.is_empty());
        assert_eq!(p.tracks.len(), 1);
        assert!(p.tracks[0].clips.is_empty());
    }

    #[test]
    fn malformed_json_is_rejected() {
        assert!(matches!(
            from_json("{ not json "),
            Err(ProjectError::Serde(_))
        ));
    }

    #[test]
    fn invalid_project_fails_validation_on_load() {
        // A document whose project has zero master tracks must fail on load.
        let mut empty = sample_project();
        empty.tracks.clear();
        let json = to_json(&empty).unwrap();
        assert!(matches!(from_json(&json), Err(ProjectError::Validation(_))));
    }

    #[test]
    fn save_load_file_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.json");
        let p = sample_project();
        save(&p, &path).unwrap();
        assert!(path.exists());
        let back = load(&path).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn effect_chain_survives_round_trip() {
        use daw_core::effect::{EffectInstance, ParamValue};
        let mut p = sample_project();
        // Attach an effect chain to the first audio track.
        if let Some(track) = p.tracks.iter_mut().find(|t| t.name == "Guitar") {
            track.effects.push(
                EffectInstance::new("distortion")
                    .with_param("drive", ParamValue::Float(4.0))
                    .with_param("output", ParamValue::Float(0.8)),
            );
            let mut bypassed = EffectInstance::new("filter")
                .with_param("cutoff", ParamValue::Float(1200.0))
                .with_param("mode", ParamValue::Int(2));
            bypassed.bypassed = true;
            track.effects.push(bypassed);
        }
        let json = to_json(&p).unwrap();
        let back = from_json(&json).unwrap();
        assert_eq!(p, back);
        let g = back.tracks.iter().find(|t| t.name == "Guitar").unwrap();
        assert_eq!(g.effects.len(), 2);
        assert!(g.effects.effects[1].bypassed);
        assert_eq!(
            g.effects.effects[0].param("drive"),
            Some(ParamValue::Float(4.0))
        );
    }

    #[test]
    fn save_is_atomic_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.json");
        save(&sample_project(), &path).unwrap();
        let temp = dir.path().join(".song.json.tmp");
        assert!(!temp.exists(), "temp file should be renamed away");
    }

    #[test]
    fn save_rejects_invalid_project() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.json");
        let mut p = sample_project();
        p.tracks.clear(); // no master -> invalid
        assert!(save(&p, &path).is_err());
        assert!(!path.exists(), "invalid project must not be persisted");
    }
}
