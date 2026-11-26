//! Windows-shell preferences, stored next to the engine's data.

use std::path::Path;

use app_core::JsonStore;
use app_core::edge::EdgePlacement;
use serde::{Deserialize, Serialize};

use crate::destinations::DestinationFields;

const FILE: &str = "windows-preferences.json";

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub edge_placement: EdgePlacement,
    /// `None` until the user first edits a destination.
    #[serde(default)]
    pub destinations: Option<DestinationFields>,
}

/// Unreadable preferences fall back to defaults (and are set aside).
pub fn load(data_dir: Option<&Path>) -> Preferences {
    data_dir
        .and_then(|dir| JsonStore::<Preferences>::new(dir, FILE).open().ok())
        .map(|loaded| loaded.value)
        .unwrap_or_default()
}

/// # Errors
/// A user-facing message when saving fails.
pub fn save(data_dir: Option<&Path>, preferences: &Preferences) -> Result<(), String> {
    let dir = data_dir.ok_or("The app couldn't find a folder for its settings.")?;
    JsonStore::new(dir, FILE)
        .save(preferences)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(Some(dir.path())), Preferences::default());

        let saved = Preferences {
            edge_placement: EdgePlacement::Right,
            destinations: Some(DestinationFields::with_default("C:\\Sorted".into())),
        };
        save(Some(dir.path()), &saved).unwrap();

        assert_eq!(load(Some(dir.path())), saved);
        assert_eq!(load(None), Preferences::default());
    }
}
