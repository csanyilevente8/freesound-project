//! Versioned on-disk project document (see spec §6 and ADR-0005).
//!
//! The on-disk schema is a dedicated, versioned document type that is kept
//! separate from the in-memory [`Project`] so the two can evolve independently
//! and so we never serialize "arbitrary internal Rust structs" (spec §6.1).
//!
//! A stable `format` magic string and an integer `schema_version` are written
//! first so that readers can validate identity and route to the correct
//! migration path before attempting to parse the body.

use serde::{Deserialize, Serialize};

use daw_core::project::Project;

/// Stable format identifier written into every project document.
///
/// This is intentionally a neutral, product-name-independent string so the
/// working name ("Free Sound") is not baked into the file format.
pub const FORMAT_MAGIC: &str = "daw-project";

/// Current schema version produced by this build.
///
/// Version history:
/// * **1** — Milestone 0: project, tracks (no clips), no media assets.
/// * **2** — Milestone 1: adds media asset references and audio clips.
/// * **3** — Milestone 1: adds per-track insert-effect chains
///   (`track.effects`).
/// * **4** — Milestone 1: adds project-level automation lanes
///   (`project.automation`). Older files are forward-compatible (the field
///   defaults to empty) and upgraded by [`crate::migrate::migrate_to_current`].
pub const CURRENT_SCHEMA_VERSION: u32 = 4;

/// The root serialized document.
///
/// The header fields (`format`, `schema_version`) are declared first and are
/// plain, forward-compatible scalars so a reader can inspect them even if the
/// body shape changes in a future version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectDocument {
    /// Format magic; must equal [`FORMAT_MAGIC`].
    pub format: String,
    /// Integer schema version of the `project` body.
    pub schema_version: u32,
    /// The project payload.
    pub project: Project,
}

impl ProjectDocument {
    /// Wraps a project in a current-version document.
    #[must_use]
    pub fn new(project: Project) -> Self {
        Self {
            format: FORMAT_MAGIC.to_string(),
            schema_version: CURRENT_SCHEMA_VERSION,
            project,
        }
    }
}
