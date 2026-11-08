use anyhow::{Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use vtcode_commons::VtCodePaths;

#[cfg(test)]
thread_local! {
    static TEST_CACHE_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct UpdateCacheSnapshot {
    pub(super) last_checked: Option<SystemTime>,
    pub(super) latest_version: Option<Version>,
    pub(super) latest_was_newer: bool,
    pub(super) last_seen_version: Option<Version>,
    pub(super) dismissed_version: Option<Version>,
    release_notes: Option<String>,
    /// Notes recorded for a version an updater just installed. Kept apart from
    /// the check fields so a later check reporting an even newer release cannot
    /// hide the notes the freshly installed version still owes the user.
    pub(super) installed_release: Option<InstalledRelease>,
}

/// Version plus body of the release an updater just installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InstalledRelease {
    pub(super) version: Version,
    pub(super) notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateCachePayload {
    last_checked_unix_secs: u64,
    #[serde(default)]
    latest_version: Option<String>,
    #[serde(default)]
    latest_was_newer: bool,
    #[serde(default)]
    last_seen_version: Option<String>,
    #[serde(default)]
    dismissed_version: Option<String>,
    #[serde(default)]
    release_notes: Option<String>,
    #[serde(default)]
    installed_release_version: Option<String>,
    #[serde(default)]
    installed_release_notes: Option<String>,
}

pub(super) fn read_snapshot() -> Result<UpdateCacheSnapshot> {
    let cache_file = cache_file_path()?;
    let legacy_paths = legacy_cache_file_paths()?;
    read_snapshot_from_paths(&cache_file, &legacy_paths)
}

fn read_snapshot_from_paths(canonical_path: &Path, legacy_paths: &[PathBuf]) -> Result<UpdateCacheSnapshot> {
    let canonical = read_snapshot_file(canonical_path)?;
    let canonical_missing = canonical.is_none();
    if let Some((snapshot, true)) = canonical.as_ref() {
        return Ok(snapshot.clone());
    }

    let mut fallback = canonical;
    for legacy_path in legacy_paths {
        if legacy_path == canonical_path {
            continue;
        }
        let Some((snapshot, valid_payload)) = read_snapshot_file(legacy_path)? else {
            continue;
        };
        if valid_payload {
            if canonical_missing {
                if let Err(error) = write_snapshot_at_if_absent(canonical_path, &snapshot) {
                    tracing::debug!(
                        path = %canonical_path.display(),
                        %error,
                        "Failed to republish legacy update cache"
                    );
                }
            }
            return Ok(snapshot);
        }
        if fallback.is_none() {
            fallback = Some((snapshot, false));
        }
    }

    Ok(fallback.map(|(snapshot, _)| snapshot).unwrap_or_default())
}

fn read_snapshot_file(path: &Path) -> Result<Option<(UpdateCacheSnapshot, bool)>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            anyhow::bail!("refusing to read symlinked update cache {}", path.display())
        }
        Ok(metadata) if !metadata.is_file() => {
            anyhow::bail!("update cache is not a regular file: {}", path.display())
        }
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to inspect update cache {}", path.display()));
        }
    };
    let modified = metadata.modified().ok();

    let content = String::from_utf8(VtCodePaths::read_file_no_follow(path)?)
        .with_context(|| format!("Failed to read update cache metadata {}", path.display()))?;

    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(Some((
            UpdateCacheSnapshot {
                last_checked: modified,
                latest_version: None,
                latest_was_newer: false,
                last_seen_version: None,
                dismissed_version: None,
                release_notes: None,
                installed_release: None,
            },
            false,
        )));
    }

    let Ok(payload) = serde_json::from_str::<UpdateCachePayload>(trimmed) else {
        return Ok(Some((
            UpdateCacheSnapshot {
                last_checked: modified,
                latest_version: None,
                latest_was_newer: false,
                last_seen_version: None,
                dismissed_version: None,
                release_notes: None,
                installed_release: None,
            },
            false,
        )));
    };

    Ok(Some((
        UpdateCacheSnapshot {
            last_checked: Some(UNIX_EPOCH + std::time::Duration::from_secs(payload.last_checked_unix_secs))
                .or(modified),
            latest_version: payload.latest_version.as_deref().and_then(|value| Version::parse(value).ok()),
            latest_was_newer: payload.latest_was_newer,
            last_seen_version: payload
                .last_seen_version
                .as_deref()
                .and_then(|value| Version::parse(value).ok()),
            dismissed_version: payload
                .dismissed_version
                .as_deref()
                .and_then(|value| Version::parse(value).ok()),
            release_notes: payload.release_notes,
            installed_release: installed_release_from(
                &payload.installed_release_version,
                &payload.installed_release_notes,
            ),
        },
        true,
    )))
}

/// Pair the recorded installed version with its notes; either half missing
/// means there is no installed-release record to show.
fn installed_release_from(version: &Option<String>, notes: &Option<String>) -> Option<InstalledRelease> {
    let version = Version::parse(version.as_deref()?).ok()?;
    Some(InstalledRelease { version, notes: notes.clone()? })
}

pub(super) fn record_successful_check_with_notes(
    latest_version: Option<&Version>,
    latest_was_newer: bool,
    release_notes: Option<&str>,
) -> Result<()> {
    update_snapshot(|snapshot| {
        snapshot.last_checked = Some(SystemTime::now());
        snapshot.latest_version = latest_version.cloned();
        snapshot.latest_was_newer = latest_was_newer;
        if let Some(release_notes) = release_notes {
            snapshot.release_notes = Some(release_notes.to_owned());
        }
        Ok(())
    })
}

#[cfg(test)]
pub(super) fn record_successful_check(latest_version: Option<&Version>, latest_was_newer: bool) -> Result<()> {
    record_successful_check_with_notes(latest_version, latest_was_newer, None)
}

pub(super) fn record_failed_check() -> Result<()> {
    update_snapshot(|snapshot| {
        snapshot.last_checked = Some(SystemTime::now());
        Ok(())
    })
}

pub(super) fn record_seen_version(version: &Version) -> Result<()> {
    update_snapshot(|snapshot| {
        snapshot.last_seen_version = Some(version.clone());
        // The install record exists only to bridge the gap between a fresh
        // install and its first announcement, so once that version is seen the
        // body is dropped: leaving it would keep a second full copy of the
        // changelog in the cache for every later startup to re-read.
        if snapshot
            .installed_release
            .as_ref()
            .is_some_and(|installed| &installed.version == version)
        {
            snapshot.installed_release = None;
        }
        Ok(())
    })
}

pub(super) fn record_dismissed_version(version: &Version) -> Result<()> {
    update_snapshot(|snapshot| {
        snapshot.dismissed_version = Some(version.clone());
        Ok(())
    })
}

/// Return the release metadata cached by the background update check when it
/// describes the running version, falling back to the notes of the release an
/// updater installed. This deliberately never performs network I/O; session
/// setup must not wait for GitHub.
pub(super) fn current_release_info(current: &Version) -> Option<(Version, String)> {
    let snapshot = read_snapshot().ok()?;
    if snapshot.latest_version.as_ref() == Some(current)
        && let Some(notes) = snapshot.release_notes.clone()
    {
        return Some((current.clone(), notes));
    }
    installed_release_info_in(&snapshot, current)
}

/// Record the release notes of a version an updater just installed.
pub(super) fn record_installed_release(version: &Version, notes: &str) -> Result<()> {
    update_snapshot(|snapshot| {
        snapshot.installed_release = Some(InstalledRelease { version: version.clone(), notes: notes.to_owned() });
        Ok(())
    })
}

fn installed_release_info_in(snapshot: &UpdateCacheSnapshot, current: &Version) -> Option<(Version, String)> {
    let installed = snapshot.installed_release.as_ref()?;
    (&installed.version == current).then(|| (installed.version.clone(), installed.notes.clone()))
}

/// Return the cached release metadata for the latest known release,
/// regardless of the running version. Like [`current_release_info`], this
/// never performs network I/O; the desktop `update/status` handler serves it
/// straight from disk.
pub(super) fn latest_release_info() -> Option<(Version, String)> {
    let snapshot = read_snapshot().ok()?;
    let latest = snapshot.latest_version?;
    Some((latest, snapshot.release_notes?))
}

pub(super) fn clear_dismissed_version() -> Result<()> {
    update_snapshot(|snapshot| {
        snapshot.dismissed_version = None;
        Ok(())
    })
}

fn update_snapshot<F>(update: F) -> Result<()>
where
    F: FnOnce(&mut UpdateCacheSnapshot) -> Result<()>,
{
    let cache_file = cache_file_path()?;
    let legacy_paths = legacy_cache_file_paths()?;
    VtCodePaths::with_private_file_lock(&cache_file, || {
        let mut snapshot = read_snapshot_from_paths(&cache_file, &legacy_paths)?;
        update(&mut snapshot)?;
        write_snapshot_at(&cache_file, &snapshot)
    })
}

fn snapshot_write_payload(cache_file: &Path, snapshot: &UpdateCacheSnapshot) -> Result<Vec<u8>> {
    let last_checked = snapshot.last_checked.unwrap_or_else(SystemTime::now);
    let last_checked_unix_secs = last_checked.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let payload = UpdateCachePayload {
        last_checked_unix_secs,
        latest_version: snapshot.latest_version.as_ref().map(ToString::to_string),
        latest_was_newer: snapshot.latest_was_newer,
        last_seen_version: snapshot.last_seen_version.as_ref().map(ToString::to_string),
        dismissed_version: snapshot.dismissed_version.as_ref().map(ToString::to_string),
        release_notes: snapshot.release_notes.clone(),
        installed_release_version: snapshot
            .installed_release
            .as_ref()
            .map(|installed| installed.version.to_string()),
        installed_release_notes: snapshot.installed_release.as_ref().map(|installed| installed.notes.clone()),
    };
    let serialized = serde_json::to_vec(&payload).context("Failed to serialize update cache payload")?;
    if let Some(parent) = cache_file.parent() {
        VtCodePaths::ensure_user_dir(parent).context("Failed to create update cache directory")?;
    }
    Ok(serialized)
}

fn write_snapshot_at(cache_file: &Path, snapshot: &UpdateCacheSnapshot) -> Result<()> {
    let serialized = snapshot_write_payload(cache_file, snapshot)?;
    VtCodePaths::write_private_file_atomic(cache_file, &serialized)
        .with_context(|| format!("Failed to write update cache {}", cache_file.display()))?;
    Ok(())
}

fn write_snapshot_at_if_absent(cache_file: &Path, snapshot: &UpdateCacheSnapshot) -> Result<bool> {
    let serialized = snapshot_write_payload(cache_file, snapshot)?;
    VtCodePaths::write_private_file_atomic_if_absent(cache_file, &serialized)
        .with_context(|| format!("Failed to write update cache {}", cache_file.display()))
}

fn legacy_cache_file_paths() -> Result<Vec<PathBuf>> {
    #[cfg(test)]
    if TEST_CACHE_DIR.with(|path| path.borrow().is_some()) {
        return Ok(Vec::new());
    }

    let paths = VtCodePaths::resolve()?;
    let mut candidates = vec![paths.legacy_dir().join("cache/last_update_check")];

    // Before the XDG/native path migration, the updater used ~/.cache/vtcode
    // (or XDG_CACHE_HOME/vtcode) independently of the rest of VT Code's
    // storage. Keep that location readable for installations that have not
    // been migrated yet or whose migration marker predates the cache file.
    let old_cache_dir = std::env::var_os("XDG_CACHE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| dirs::home_dir().map(|home| home.join(".cache")))
        .map(|path| path.join("vtcode/last_update_check"));
    if let Some(path) = old_cache_dir {
        candidates.push(path);
    }

    // DotManager's historical cache root was the configuration directory.
    // This is also where some early cache writers placed their metadata.
    candidates.push(paths.config_dir().join("cache/last_update_check"));
    candidates.dedup();
    Ok(candidates)
}

fn get_cache_dir() -> Result<PathBuf> {
    #[cfg(test)]
    if let Some(path) = TEST_CACHE_DIR.with(|path| path.borrow().clone()) {
        return VtCodePaths::ensure_user_dir(&path).map(|_| path);
    }

    Ok(VtCodePaths::resolve()?.ensure_cache_dir()?.to_path_buf())
}

#[cfg(test)]
pub(crate) fn set_cache_dir_override_for_tests(path: Option<PathBuf>) -> Option<PathBuf> {
    TEST_CACHE_DIR.with(|current| std::mem::replace(&mut *current.borrow_mut(), path))
}

fn cache_file_path() -> Result<PathBuf> {
    Ok(get_cache_dir()?.join("last_update_check"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_payload(path: &Path, latest_version: &str, last_checked_unix_secs: u64) {
        let payload = UpdateCachePayload {
            last_checked_unix_secs,
            latest_version: Some(latest_version.to_string()),
            latest_was_newer: true,
            last_seen_version: None,
            dismissed_version: None,
            release_notes: None,
            installed_release_version: None,
            installed_release_notes: None,
        };
        std::fs::write(path, serde_json::to_string(&payload).expect("serialize update cache payload"))
            .expect("write update cache payload");
    }

    #[test]
    fn empty_legacy_cache_file_uses_file_metadata() {
        let temp_dir = TempDir::new().expect("temp dir");
        let previous = set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        let cache_file = cache_file_path().expect("cache path");
        std::fs::write(&cache_file, "").expect("write legacy cache");

        let snapshot = read_snapshot().expect("read snapshot");
        assert!(snapshot.last_checked.is_some());
        assert!(snapshot.latest_version.is_none());
        assert!(!snapshot.latest_was_newer);

        set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn json_cache_round_trips_latest_version() {
        let temp_dir = TempDir::new().expect("temp dir");
        let previous = set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        let version = Version::parse("0.113.0").expect("version");
        record_successful_check(Some(&version), true).expect("write cache");

        let snapshot = read_snapshot().expect("read snapshot");
        assert_eq!(snapshot.latest_version, Some(version));
        assert!(snapshot.latest_was_newer);
        assert!(snapshot.last_checked.is_some());
        assert!(snapshot.dismissed_version.is_none());

        set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn record_and_clear_dismissed_version() {
        let temp_dir = TempDir::new().expect("temp dir");
        let previous = set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        let version = Version::parse("0.113.0").expect("version");
        record_successful_check(Some(&version), true).expect("write cache");
        assert!(read_snapshot().expect("snapshot").dismissed_version.is_none());

        record_dismissed_version(&version).expect("record dismissal");
        let snapshot = read_snapshot().expect("read snapshot");
        assert_eq!(snapshot.dismissed_version, Some(version));

        clear_dismissed_version().expect("clear dismissal");
        assert!(read_snapshot().expect("snapshot").dismissed_version.is_none());

        set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn legacy_cache_is_loaded_and_republished_to_canonical_path() {
        let temp_dir = TempDir::new().expect("temp dir");
        let canonical_path = temp_dir.path().join("canonical/last_update_check");
        let legacy_path = temp_dir.path().join("legacy/last_update_check");
        std::fs::create_dir_all(legacy_path.parent().expect("legacy parent")).expect("create legacy directory");
        write_payload(&legacy_path, "0.114.0", 123);

        let snapshot = read_snapshot_from_paths(&canonical_path, std::slice::from_ref(&legacy_path))
            .expect("read legacy update cache");

        assert_eq!(snapshot.latest_version, Version::parse("0.114.0").ok());
        assert_eq!(snapshot.last_checked, Some(UNIX_EPOCH + std::time::Duration::from_secs(123)));
        assert!(canonical_path.is_file());
        let canonical_content = std::fs::read_to_string(&canonical_path).expect("read canonical cache");
        assert!(canonical_content.contains("0.114.0"));
    }

    #[test]
    fn canonical_cache_takes_precedence_over_legacy_cache() {
        let temp_dir = TempDir::new().expect("temp dir");
        let canonical_path = temp_dir.path().join("canonical/last_update_check");
        let legacy_path = temp_dir.path().join("legacy/last_update_check");
        std::fs::create_dir_all(canonical_path.parent().expect("canonical parent"))
            .expect("create canonical directory");
        std::fs::create_dir_all(legacy_path.parent().expect("legacy parent")).expect("create legacy directory");
        write_payload(&canonical_path, "0.115.0", 456);
        write_payload(&legacy_path, "0.114.0", 123);

        let snapshot = read_snapshot_from_paths(&canonical_path, std::slice::from_ref(&legacy_path))
            .expect("read canonical update cache");

        assert_eq!(snapshot.latest_version, Version::parse("0.115.0").ok());
        assert_eq!(snapshot.last_checked, Some(UNIX_EPOCH + std::time::Duration::from_secs(456)));
        let canonical_content = std::fs::read_to_string(&canonical_path).expect("read canonical cache");
        assert!(canonical_content.contains("0.115.0"));
        assert!(!canonical_content.contains("0.114.0"));
    }
}
