//! Legacy plaintext auth.json migration.
//!
//! In earlier versions API keys were stored in a plaintext `auth.json` file.
//! These functions detect, read, and migrate such entries into the current
//! encrypted storage format, then delete the legacy file.

use anyhow::{Context, Result, anyhow};
use std::fs;

use crate::storage_paths::legacy_auth_storage_path;

#[derive(Debug, serde::Deserialize)]
pub(crate) struct LegacyAuthFile {
    mode: String,
    provider: String,
    pub(crate) api_key: String,
}

/// Find and return a legacy auth.json entry for `provider`, if one exists.
pub(crate) fn load_for_provider(provider: &str) -> Result<Option<LegacyAuthFile>> {
    let path = legacy_auth_storage_path()?;
    let data = match fs::read(&path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(anyhow!("failed to read legacy auth file: {err}")),
    };

    let legacy: LegacyAuthFile = serde_json::from_slice(&data).context("failed to parse legacy auth file")?;
    let matches_provider = legacy.provider.eq_ignore_ascii_case(provider);
    let stores_api_key = legacy.mode.eq_ignore_ascii_case("api_key");
    let has_key = !legacy.api_key.trim().is_empty();

    if matches_provider && stores_api_key && has_key {
        Ok(Some(legacy))
    } else {
        Ok(None)
    }
}

/// Delete the legacy auth.json if it contains an entry for `provider`.
///
/// Reads the file once, checks for a match, and deletes — avoids the
/// double-read that a separate `load_for_provider` + delete call would do.
pub(crate) fn clear_for_provider(provider: &str) -> Result<()> {
    let path = legacy_auth_storage_path()?;
    let data = match fs::read(&path) {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(anyhow!("failed to read legacy auth file: {err}")),
    };

    let Ok(legacy) = serde_json::from_slice::<LegacyAuthFile>(&data) else {
        return Ok(());
    };

    if !legacy.mode.eq_ignore_ascii_case("api_key") || !legacy.provider.eq_ignore_ascii_case(provider) {
        return Ok(());
    }

    delete_file(&path)
}

/// Remove the legacy auth.json file if it exists (ignoring absent).
pub(crate) fn delete_file(path: &std::path::Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(anyhow!("failed to delete legacy auth file: {err}")),
    }
}
