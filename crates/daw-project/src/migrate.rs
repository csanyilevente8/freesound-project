//! Schema migration scaffolding (see spec §6.1).
//!
//! Given a document at some older schema version, [`migrate_to_current`]
//! upgrades it step-by-step to [`CURRENT_SCHEMA_VERSION`]. Each step is a small
//! function; adding a future version means adding one more step and bumping the
//! constant, keeping older files loadable.
//!
//! ## v1 → v2 (Milestone 1)
//!
//! v2 adds media asset references (`project.assets`) and per-track audio clips
//! (`track.clips`). Both new fields use `#[serde(default)]`, so a v1 document
//! (which lacks them) deserializes with empty collections. The v1→v2 step
//! therefore only needs to stamp the new version number — the data is already
//! in a valid v2 shape. This still exercises the migration *mechanism* so the
//! next, data-transforming migration has a tested path to follow.

use crate::document::{ProjectDocument, CURRENT_SCHEMA_VERSION};
use crate::error::ProjectError;

/// Upgrades a document to the current schema version, or returns an error if
/// its version is newer than this build supports.
///
/// # Errors
/// Returns [`ProjectError::UnsupportedVersion`] if the document's version is
/// greater than [`CURRENT_SCHEMA_VERSION`].
pub fn migrate_to_current(mut doc: ProjectDocument) -> Result<ProjectDocument, ProjectError> {
    if doc.schema_version > CURRENT_SCHEMA_VERSION {
        return Err(ProjectError::UnsupportedVersion {
            found: doc.schema_version,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }

    while doc.schema_version < CURRENT_SCHEMA_VERSION {
        doc = match doc.schema_version {
            1 => migrate_1_to_2(doc),
            2 => migrate_2_to_3(doc),
            3 => migrate_3_to_4(doc),
            // Unreachable: the loop condition guarantees a version below
            // CURRENT, and every such version must have a step above.
            other => {
                return Err(ProjectError::UnsupportedVersion {
                    found: other,
                    supported: CURRENT_SCHEMA_VERSION,
                })
            }
        };
    }

    Ok(doc)
}

/// Migrates a v1 document to v2.
///
/// v1 has no `assets` / `clips`; serde defaults filled them with empty
/// collections during parsing, so this step only advances the version marker.
fn migrate_1_to_2(mut doc: ProjectDocument) -> ProjectDocument {
    doc.schema_version = 2;
    doc
}

/// Migrates a v2 document to v3.
///
/// v2 has no per-track `effects`; serde defaults filled each track with an
/// empty chain during parsing, so this step only advances the version marker.
fn migrate_2_to_3(mut doc: ProjectDocument) -> ProjectDocument {
    doc.schema_version = 3;
    doc
}

/// Migrates a v3 document to v4.
///
/// v3 has no project-level `automation`; serde defaults filled it with an empty
/// list during parsing, so this step only advances the version marker.
fn migrate_3_to_4(mut doc: ProjectDocument) -> ProjectDocument {
    doc.schema_version = 4;
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use daw_core::project::Project;
    use daw_core::time::SampleRate;

    #[test]
    fn current_version_passes_through() {
        let doc = ProjectDocument::new(Project::new("P", SampleRate::default()));
        let migrated = migrate_to_current(doc.clone()).unwrap();
        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn future_version_is_rejected() {
        let mut doc = ProjectDocument::new(Project::new("P", SampleRate::default()));
        doc.schema_version = CURRENT_SCHEMA_VERSION + 1;
        assert!(matches!(
            migrate_to_current(doc),
            Err(ProjectError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn v1_is_upgraded_to_current() {
        let mut doc = ProjectDocument::new(Project::new("P", SampleRate::default()));
        doc.schema_version = 1;
        let migrated = migrate_to_current(doc).unwrap();
        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        // v1 content has no assets; migration leaves it as an empty, valid v2.
        assert!(migrated.project.assets.is_empty());
    }
}
