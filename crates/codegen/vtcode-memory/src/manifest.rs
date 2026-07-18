use std::fs::{self, File};
use std::io::BufReader;

use crate::SessionManifest;
use crate::TurnIndex;
use crate::error::SessionStoreError;

/// Manifest persistence helpers.
///
/// Separated from `event_log` so the hot append path does not carry
/// serialization concerns, and so `open` can cheaply probe the manifest
/// before deciding whether to run the O(n) scan.
pub struct ManifestStore {
    session_dir: std::path::PathBuf,
}

impl ManifestStore {
    /// Create a new manifest store for the given session directory.
    pub fn new(session_dir: std::path::PathBuf) -> Self {
        Self { session_dir }
    }

    /// Path to `manifest.json` inside the session directory.
    pub fn manifest_path(&self) -> std::path::PathBuf {
        self.session_dir.join("manifest.json")
    }

    /// Path to `index/turns.json` inside the session directory.
    pub fn turns_path(&self) -> std::path::PathBuf {
        self.session_dir.join("index").join("turns.json")
    }

    /// Load the manifest if it exists and is parseable.
    ///
    /// Returns `Ok(None)` when the file is missing (fresh session) or
    /// unreadable, rather than erroring — the caller can fall back to
    /// scanning the event log.
    pub fn load_manifest(&self) -> Result<Option<SessionManifest>, SessionStoreError> {
        let path = self.manifest_path();
        if !path.exists() {
            return Ok(None);
        }
        let file = File::open(&path).map_err(|e| SessionStoreError::io(path.clone(), e))?;
        let reader = BufReader::new(file);
        let manifest: SessionManifest =
            serde_json::from_reader(reader).map_err(|e| SessionStoreError::io(path.clone(), e.into()))?;
        Ok(Some(manifest))
    }

    /// Load the turn index if it exists and is parseable.
    ///
    /// Returns `Ok(None)` when the file is missing or unreadable.
    pub fn load_turn_index(&self) -> Result<Option<TurnIndex>, SessionStoreError> {
        let path = self.turns_path();
        if !path.exists() {
            return Ok(None);
        }
        let file = File::open(&path).map_err(|e| SessionStoreError::io(path.clone(), e))?;
        let reader = BufReader::new(file);
        let index: TurnIndex =
            serde_json::from_reader(reader).map_err(|e| SessionStoreError::io(path.clone(), e.into()))?;
        Ok(Some(index))
    }
    /// Atomically write the manifest. Parent directories must already exist.
    pub fn write_manifest(&self, manifest: &SessionManifest) -> Result<(), SessionStoreError> {
        let path = self.manifest_path();
        let bytes = serde_json::to_string_pretty(manifest)?;
        fs::write(&path, bytes).map_err(|e| SessionStoreError::io(path.clone(), e))
    }

    /// Atomically write the turn index. Parent directories must already exist.
    pub fn write_turn_index(&self, index: &TurnIndex) -> Result<(), SessionStoreError> {
        let path = self.turns_path();
        let bytes = serde_json::to_string_pretty(index)?;
        fs::write(&path, bytes).map_err(|e| SessionStoreError::io(path.clone(), e))
    }
}
