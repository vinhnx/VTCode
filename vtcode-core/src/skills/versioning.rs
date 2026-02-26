//! Skill version resolution
//!
//! Implements deterministic version resolution with support for:
//! - `default_version` (stable, recommended for production)
//! - `latest_version` (opt-in, newest available)
//! - Lockfile-based pinning for reproducibility
//! - Fallback to manifest `version` field

use crate::utils::file_utils::{
    ensure_dir_exists_sync, read_file_with_context_sync, write_file_with_context_sync,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::skills::container::SkillVersion;
use crate::skills::types::SkillManifest;

/// A fully resolved skill reference with concrete version
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkillRef {
    /// Skill name
    pub name: String,
    /// What was originally requested
    pub requested: SkillVersion,
    /// Concrete resolved version string
    pub resolved: String,
    /// Where the skill was found
    pub source: SkillSource,
}

/// Where a resolved skill comes from
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    /// On-disk skill directory
    LocalDir(PathBuf),
    /// Imported bundle from skill store
    ImportedBundle(PathBuf),
    /// Inline bundle (temporary)
    InlineBundle(PathBuf),
}

/// Lockfile for reproducible skill version resolution
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillLockfile {
    /// Locked skill versions: name -> version
    pub locked: HashMap<String, String>,
    /// When the lockfile was last updated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

const LOCKFILE_NAME: &str = "skills.lock";

impl SkillLockfile {
    /// Load lockfile from a directory (repo or user level)
    pub fn load(dir: &Path) -> Result<Self> {
        let path = dir.join(LOCKFILE_NAME);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = read_file_with_context_sync(&path, "skills lockfile")
            .with_context(|| format!("Failed to read lockfile at {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse lockfile at {}", path.display()))
    }

    /// Save lockfile to a directory
    pub fn save(&self, dir: &Path) -> Result<()> {
        let path = dir.join(LOCKFILE_NAME);
        ensure_dir_exists_sync(dir)?;
        let content = serde_json::to_string_pretty(self)?;
        write_file_with_context_sync(&path, &content, "skills lockfile")
            .with_context(|| format!("Failed to write lockfile at {}", path.display()))?;
        info!("Saved skill lockfile to {}", path.display());
        Ok(())
    }

    /// Get locked version for a skill
    pub fn get_locked(&self, name: &str) -> Option<&str> {
        self.locked.get(name).map(|s| s.as_str())
    }

    /// Lock a skill to a specific version
    pub fn lock(&mut self, name: String, version: String) {
        self.locked.insert(name, version);
        self.updated_at = Some(chrono::Utc::now().to_rfc3339());
    }

    /// Check if any skills are locked
    pub fn is_empty(&self) -> bool {
        self.locked.is_empty()
    }
}

/// Resolve a skill version based on manifest, lockfile, and request
///
/// Resolution order:
/// 1. If `Specific(v)` requested: use that exact version
/// 2. If lockfile has an entry: use locked version
/// 3. If `Latest` requested: use `manifest.latest_version` or `manifest.version`
/// 4. Otherwise: use `manifest.default_version` or `manifest.version`
pub fn resolve_version(
    manifest: &SkillManifest,
    requested: &SkillVersion,
    lockfile: Option<&SkillLockfile>,
) -> Result<String> {
    match requested {
        SkillVersion::Specific(v) => {
            debug!(
                "Using specifically requested version '{}' for '{}'",
                v, manifest.name
            );
            Ok(v.clone())
        }
        SkillVersion::Latest => {
            if let Some(lock) = lockfile
                && let Some(locked) = lock.get_locked(&manifest.name)
            {
                debug!(
                    "Using locked version '{}' for '{}' (Latest requested)",
                    locked, manifest.name
                );
                return Ok(locked.to_string());
            }

            if let Some(ref latest) = manifest.latest_version {
                debug!("Resolved Latest to '{}' for '{}'", latest, manifest.name);
                return Ok(latest.clone());
            }

            if let Some(ref version) = manifest.version {
                debug!(
                    "Falling back to manifest version '{}' for '{}'",
                    version, manifest.name
                );
                return Ok(version.clone());
            }

            warn!(
                "No version info available for '{}', using '0.0.0'",
                manifest.name
            );
            Ok("0.0.0".to_string())
        }
    }
}

/// Resolve version using default_version semantics (no version specified by user)
pub fn resolve_default_version(
    manifest: &SkillManifest,
    lockfile: Option<&SkillLockfile>,
) -> String {
    if let Some(lock) = lockfile
        && let Some(locked) = lock.get_locked(&manifest.name)
    {
        return locked.to_string();
    }

    if let Some(ref default) = manifest.default_version {
        return default.clone();
    }

    manifest
        .version
        .clone()
        .unwrap_or_else(|| "0.0.0".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manifest(name: &str) -> SkillManifest {
        SkillManifest {
            name: name.to_string(),
            description: "Test skill".to_string(),
            version: Some("1.0.0".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_resolve_specific() {
        let manifest = test_manifest("test");
        let result = resolve_version(
            &manifest,
            &SkillVersion::Specific("2.0.0".to_string()),
            None,
        );
        assert_eq!(result.unwrap(), "2.0.0");
    }

    #[test]
    fn test_resolve_latest_with_latest_version() {
        let mut manifest = test_manifest("test");
        manifest.latest_version = Some("1.2.0".to_string());
        let result = resolve_version(&manifest, &SkillVersion::Latest, None);
        assert_eq!(result.unwrap(), "1.2.0");
    }

    #[test]
    fn test_resolve_latest_fallback_to_version() {
        let manifest = test_manifest("test");
        let result = resolve_version(&manifest, &SkillVersion::Latest, None);
        assert_eq!(result.unwrap(), "1.0.0");
    }

    #[test]
    fn test_resolve_latest_with_lockfile() {
        let manifest = test_manifest("test");
        let mut lock = SkillLockfile::default();
        lock.locked.insert("test".to_string(), "0.9.0".to_string());
        let result = resolve_version(&manifest, &SkillVersion::Latest, Some(&lock));
        assert_eq!(result.unwrap(), "0.9.0");
    }

    #[test]
    fn test_resolve_default_version() {
        let mut manifest = test_manifest("test");
        manifest.default_version = Some("1.0.0".to_string());
        manifest.latest_version = Some("1.2.0".to_string());
        let result = resolve_default_version(&manifest, None);
        assert_eq!(result, "1.0.0");
    }

    #[test]
    fn test_lockfile_roundtrip() {
        let mut lock = SkillLockfile::default();
        lock.locked
            .insert("skill-a".to_string(), "1.0.0".to_string());
        lock.locked
            .insert("skill-b".to_string(), "2.0.0".to_string());
        let json = serde_json::to_string(&lock).unwrap();
        let parsed: SkillLockfile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.locked.len(), 2);
        assert_eq!(parsed.get_locked("skill-a"), Some("1.0.0"));
    }

    #[test]
    fn test_lockfile_empty() {
        let lock = SkillLockfile::default();
        assert!(lock.is_empty());
    }
}
