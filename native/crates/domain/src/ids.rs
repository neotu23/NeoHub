//! Branded identifier newtypes.
//!
//! The stored values are the existing ULID strings — the migration must never
//! re-generate identifiers, so these types are transparent wrappers only.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Mint a new ULID-shaped identifier (same alphabet/length as the legacy app).
            pub fn new() -> Self {
                Self(ulid::Ulid::new().to_string())
            }

            /// Adopt an identifier that already exists (e.g. imported from the legacy DB).
            pub fn from_existing(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
    };
}

id_type!(AppId);
id_type!(TaskId);
// One PTY run of one task: log cursors are scoped to a run so a restart cannot be
// mistaken for "the same stream continuing".
id_type!(RunId);
id_type!(EnvVarId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_ids_are_26_char_ulids() {
        let id = AppId::new();
        assert_eq!(id.as_str().len(), 26, "ULID textual length");
        assert!(id.as_str().chars().all(|c| c.is_ascii_alphanumeric()));
        assert_ne!(AppId::new(), AppId::new());
    }

    #[test]
    fn existing_ids_are_preserved_verbatim() {
        let legacy = "01J8ZK9V4S7Q3M2N1P0R5T6W8X";
        let id = TaskId::from_existing(legacy);
        assert_eq!(id.as_str(), legacy);
        assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{legacy}\""));
    }

    #[test]
    fn ids_round_trip_through_serde_transparently() {
        let id: AppId = serde_json::from_str("\"abc\"").unwrap();
        assert_eq!(id.as_str(), "abc");
    }
}
