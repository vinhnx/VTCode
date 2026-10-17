use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use super::query::is_session_file;
use super::settings::{SESSION_MAX_AGE_DAYS_ENV, SESSION_MAX_FILES_ENV, SESSION_MAX_SIZE_MB_ENV, read_env_var};
use super::{BYTES_PER_MB, DEFAULT_SESSION_MAX_AGE_DAYS, DEFAULT_SESSION_MAX_FILES, DEFAULT_SESSION_MAX_SIZE_MB};
use crate::core::SECONDS_PER_DAY;

#[derive(Debug, Clone)]
struct SessionFileEntry {
    path: PathBuf,
    modified: SystemTime,
    size: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionRetentionLimits {
    pub(crate) max_files: usize,
    pub(crate) max_age_days: u64,
    pub(crate) max_total_size_bytes: u64,
}

impl Default for SessionRetentionLimits {
    fn default() -> Self {
        Self {
            max_files: DEFAULT_SESSION_MAX_FILES,
            max_age_days: DEFAULT_SESSION_MAX_AGE_DAYS,
            max_total_size_bytes: DEFAULT_SESSION_MAX_SIZE_MB.saturating_mul(BYTES_PER_MB),
        }
    }
}

impl SessionRetentionLimits {
    fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            max_files: parse_env_value(SESSION_MAX_FILES_ENV).unwrap_or(defaults.max_files),
            max_age_days: parse_env_value(SESSION_MAX_AGE_DAYS_ENV).unwrap_or(defaults.max_age_days),
            max_total_size_bytes: parse_env_value::<u64>(SESSION_MAX_SIZE_MB_ENV)
                .map(|value| value.saturating_mul(BYTES_PER_MB))
                .unwrap_or(defaults.max_total_size_bytes),
        }
    }
}

fn session_retention_limits() -> SessionRetentionLimits {
    SessionRetentionLimits::from_env()
}

fn parse_env_value<T>(key: &str) -> Option<T>
where
    T: FromStr,
{
    read_env_var(key)?.trim().parse().ok()
}

pub(crate) fn apply_session_retention_best_effort(sessions_dir: &Path) {
    if let Err(err) = apply_session_retention(sessions_dir) {
        tracing::warn!(
            sessions_dir = %sessions_dir.display(),
            error = %err,
            "Failed to prune session archives"
        );
    }
}

fn apply_session_retention(sessions_dir: &Path) -> Result<()> {
    apply_session_retention_with_limits(sessions_dir, session_retention_limits())
}

pub(crate) fn apply_session_retention_with_limits(sessions_dir: &Path, limits: SessionRetentionLimits) -> Result<()> {
    let mut entries = collect_session_entries(sessions_dir)?;

    if entries.is_empty() {
        return Ok(());
    }

    let now = SystemTime::now();
    let age_cutoff = if limits.max_age_days == 0 {
        now
    } else {
        now.checked_sub(Duration::from_secs(limits.max_age_days.saturating_mul(SECONDS_PER_DAY)))
            .unwrap_or(UNIX_EPOCH)
    };

    let (expired, retained): (Vec<_>, Vec<_>) = entries.into_iter().partition(|entry| entry.modified <= age_cutoff);
    remove_session_files(expired);
    entries = retained;

    entries.sort_by_key(|a| std::cmp::Reverse(a.modified));

    if limits.max_files > 0 && entries.len() > limits.max_files {
        let overflow = entries.split_off(limits.max_files);
        remove_session_files(overflow);
    }

    if limits.max_total_size_bytes == 0 || entries.is_empty() {
        return Ok(());
    }

    let mut total_size = 0u64;
    let mut keep_entries = Vec::with_capacity(entries.len());
    let mut size_overflow = Vec::new();

    for entry in entries {
        let projected = total_size.saturating_add(entry.size);
        if keep_entries.is_empty() || projected <= limits.max_total_size_bytes {
            total_size = projected;
            keep_entries.push(entry);
        } else {
            size_overflow.push(entry);
        }
    }

    remove_session_files(size_overflow);
    Ok(())
}

fn collect_session_entries(sessions_dir: &Path) -> Result<Vec<SessionFileEntry>> {
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(sessions_dir)
        .with_context(|| format!("failed to read session directory for retention: {}", sessions_dir.display()))?
    {
        let entry = match entry {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!(
                    sessions_dir = %sessions_dir.display(),
                    error = %err,
                    "Failed to read a session archive entry"
                );
                continue;
            }
        };
        let path = entry.path();
        if !is_session_file(&path) {
            continue;
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "Failed to read session archive metadata"
                );
                continue;
            }
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        entries.push(SessionFileEntry { path, modified, size: metadata.len() });
    }

    Ok(entries)
}

fn remove_session_files(entries: Vec<SessionFileEntry>) {
    for entry in entries {
        if let Err(err) = fs::remove_file(&entry.path) {
            tracing::warn!(
                path = %entry.path.display(),
                error = %err,
                "Failed to remove session archive"
            );
        }
    }
}
