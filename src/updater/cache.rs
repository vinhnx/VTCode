use anyhow::{Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use vtcode_core::utils::file_utils::{ensure_dir_exists_sync, write_file_with_context_sync};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct UpdateCacheSnapshot {
    pub(super) last_checked: Option<SystemTime>,
    pub(super) latest_version: Option<Version>,
    pub(super) latest_was_newer: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateCachePayload {
    last_checked_unix_secs: u64,
    #[serde(default)]
    latest_version: Option<String>,
    #[serde(default)]
    latest_was_newer: bool,
}

pub(super) fn read_snapshot() -> Result<UpdateCacheSnapshot> {
    let cache_file = cache_file_path()?;
    if !cache_file.exists() {
        return Ok(UpdateCacheSnapshot::default());
    }

    let metadata = std::fs::metadata(&cache_file).with_context(|| {
        format!(
            "Failed to read update cache metadata {}",
            cache_file.display()
        )
    })?;
    let modified = metadata.modified().ok();

    let Ok(content) = std::fs::read_to_string(&cache_file) else {
        return Ok(UpdateCacheSnapshot {
            last_checked: modified,
            latest_version: None,
            latest_was_newer: false,
        });
    };

    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(UpdateCacheSnapshot {
            last_checked: modified,
            latest_version: None,
            latest_was_newer: false,
        });
    }

    let Ok(payload) = serde_json::from_str::<UpdateCachePayload>(trimmed) else {
        return Ok(UpdateCacheSnapshot {
            last_checked: modified,
            latest_version: None,
            latest_was_newer: false,
        });
    };

    Ok(UpdateCacheSnapshot {
        last_checked: payload
            .last_checked_unix_secs
            .checked_add(0)
            .map(|secs| UNIX_EPOCH + std::time::Duration::from_secs(secs))
            .or(modified),
        latest_version: payload
            .latest_version
            .as_deref()
            .and_then(|value| Version::parse(value).ok()),
        latest_was_newer: payload.latest_was_newer,
    })
}

pub(super) fn record_successful_check(
    latest_version: Option<&Version>,
    latest_was_newer: bool,
) -> Result<()> {
    write_snapshot(UpdateCacheSnapshot {
        last_checked: Some(SystemTime::now()),
        latest_version: latest_version.cloned(),
        latest_was_newer,
    })
}

pub(super) fn record_failed_check() -> Result<()> {
    let mut snapshot = read_snapshot()?;
    snapshot.last_checked = Some(SystemTime::now());
    write_snapshot(snapshot)
}

fn write_snapshot(snapshot: UpdateCacheSnapshot) -> Result<()> {
    let last_checked = snapshot.last_checked.unwrap_or_else(SystemTime::now);
    let last_checked_unix_secs = last_checked
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let payload = UpdateCachePayload {
        last_checked_unix_secs,
        latest_version: snapshot.latest_version.map(|version| version.to_string()),
        latest_was_newer: snapshot.latest_was_newer,
    };
    let serialized =
        serde_json::to_string(&payload).context("Failed to serialize update cache payload")?;
    write_file_with_context_sync(&cache_file_path()?, &serialized, "update cache")
        .context("Failed to write update cache")?;
    Ok(())
}

fn get_cache_dir() -> Result<PathBuf> {
    let dir = if let Ok(xdg_cache) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(xdg_cache).join("vtcode")
    } else {
        let home = dirs::home_dir().context("Cannot determine home directory")?;
        home.join(".cache/vtcode")
    };

    ensure_dir_exists_sync(&dir).context("Failed to create cache directory")?;
    Ok(dir)
}

fn cache_file_path() -> Result<PathBuf> {
    Ok(get_cache_dir()?.join("last_update_check"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock")
    }

    #[test]
    fn empty_legacy_cache_file_uses_file_metadata() {
        let _guard = env_guard();
        let temp_dir = TempDir::new().expect("temp dir");
        let previous = env::var_os("XDG_CACHE_HOME");
        unsafe {
            env::set_var("XDG_CACHE_HOME", temp_dir.path());
        }

        let cache_file = cache_file_path().expect("cache path");
        std::fs::write(&cache_file, "").expect("write legacy cache");

        let snapshot = read_snapshot().expect("read snapshot");
        assert!(snapshot.last_checked.is_some());
        assert!(snapshot.latest_version.is_none());
        assert!(!snapshot.latest_was_newer);

        unsafe {
            if let Some(value) = previous {
                env::set_var("XDG_CACHE_HOME", value);
            } else {
                env::remove_var("XDG_CACHE_HOME");
            }
        }
    }

    #[test]
    fn json_cache_round_trips_latest_version() {
        let _guard = env_guard();
        let temp_dir = TempDir::new().expect("temp dir");
        let previous = env::var_os("XDG_CACHE_HOME");
        unsafe {
            env::set_var("XDG_CACHE_HOME", temp_dir.path());
        }

        let version = Version::parse("0.113.0").expect("version");
        record_successful_check(Some(&version), true).expect("write cache");

        let snapshot = read_snapshot().expect("read snapshot");
        assert_eq!(snapshot.latest_version, Some(version));
        assert!(snapshot.latest_was_newer);
        assert!(snapshot.last_checked.is_some());

        unsafe {
            if let Some(value) = previous {
                env::set_var("XDG_CACHE_HOME", value);
            } else {
                env::remove_var("XDG_CACHE_HOME");
            }
        }
    }
}
