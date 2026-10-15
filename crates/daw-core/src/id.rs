//! Stable identifiers for domain entities.
//!
//! Identifiers are UUIDs so that references remain valid across renames,
//! reordering, and serialization round-trips. Array positions and display
//! names must never be used as references (see spec §5).
//!
//! Each entity gets a distinct newtype wrapper so the type system prevents
//! accidentally using (for example) a [`TrackId`] where a [`ClipId`] is
//! expected.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Declares a strongly-typed, UUID-backed identifier newtype.
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a new random (v4) identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wraps an existing [`Uuid`]. Useful for tests and migrations.
            #[must_use]
            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// Returns the inner [`Uuid`].
            #[must_use]
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

define_id!(
    /// Identifies a project.
    ProjectId
);
define_id!(
    /// Identifies a track within a project.
    TrackId
);
define_id!(
    /// Identifies a clip within a track.
    ClipId
);
define_id!(
    /// Identifies a media asset referenced by the project.
    AssetId
);
define_id!(
    /// Identifies an effect/processor instance.
    EffectId
);
define_id!(
    /// Identifies an automatable parameter target.
    ParameterId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        assert_ne!(TrackId::new(), TrackId::new());
    }

    #[test]
    fn id_round_trips_through_uuid() {
        let id = ClipId::new();
        assert_eq!(id, ClipId::from_uuid(id.as_uuid()));
    }

    #[test]
    fn id_serde_round_trip() {
        let id = ProjectId::new();
        let json = serde_json::to_string(&id).unwrap();
        let back: ProjectId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
