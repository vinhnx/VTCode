use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs;
use std::path::Path;

use super::time::unix_timestamp_now;

pub(crate) fn load_json_cache<T: DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {} at {}", label, path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {} at {}", label, path.display()))
}

pub(crate) fn save_json_cache<T: Serialize>(
    state_dir: &Path,
    path: &Path,
    value: &T,
    label: &str,
) -> Result<()> {
    fs::create_dir_all(state_dir)
        .with_context(|| format!("Failed to create {}", state_dir.display()))?;
    let content =
        serde_json::to_string(value).with_context(|| format!("Failed to serialize {}", label))?;
    fs::write(path, content)
        .with_context(|| format!("Failed to write {} at {}", label, path.display()))?;
    Ok(())
}

pub(crate) fn cache_is_stale(last_attempt: u64, stale_after_secs: u64) -> bool {
    unix_timestamp_now().saturating_sub(last_attempt) > stale_after_secs
}
