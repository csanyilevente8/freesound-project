//! `daw-project`: versioned project persistence.
//!
//! Provides a versioned [`document::ProjectDocument`] wrapper, atomic
//! save/load ([`io::save`] / [`io::load`]), string round-trip helpers, and
//! migration scaffolding. The on-disk schema is decoupled from the in-memory
//! [`daw_core::project::Project`] so the two can evolve independently.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod document;
pub mod error;
pub mod io;
pub mod media_check;
pub mod migrate;

pub use document::{ProjectDocument, CURRENT_SCHEMA_VERSION, FORMAT_MAGIC};
pub use error::ProjectError;
pub use io::{from_json, load, save, to_json};
pub use media_check::{find_missing_media, relink_asset, MissingMedia};
pub use migrate::migrate_to_current;
