use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use vtcode_core::utils::dot_config::DotManager;
use vtcode_core::utils::session_archive::SESSION_DIR_ENV;

use super::debug_context::sanitize_debug_component;

pub(super) const DEFAULT_MAX_DEBUG_LOG_SIZE_MB: u64 = 50;
pub(super) const DEFAULT_MAX_DEBUG_LOG_AGE_DAYS: u32 = 7;

const DEBUG_LOG_FILE_PREFIX: &str = "debug-";
const DEBUG_BYTES_PER_MB: u64 = 1024 * 1024;
const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

fn debug_log_file_name(session_id: &str) -> String {
    let normalized = sanitize_debug_component(session_id, "session");
    format!("{DEBUG_LOG_FILE_PREFIX}{normalized}.log")
}

fn default_debug_log_dir() -> PathBuf {
    if let Some(custom) = std::env::var_os(SESSION_DIR_ENV) {
        return PathBuf::from(custom);
    }
    if let Ok(manager) = DotManager::new() {
        return manager.sessions_dir();
    }
    PathBuf::from(".vtcode/sessions")
}

fn is_debug_log_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    name.starts_with(DEBUG_LOG_FILE_PREFIX) && name.ends_with(".log")
}

fn prune_expired_debug_logs(log_dir: &Path, max_age_days: u32) -> Result<()> {
    let cutoff = if max_age_days == 0 {
        SystemTime::now()
    } else {
        SystemTime::now()
            .checked_sub(Duration::from_secs(
                u64::from(max_age_days).saturating_mul(SECONDS_PER_DAY),
            ))
            .unwrap_or(UNIX_EPOCH)
    };

    for entry in fs::read_dir(log_dir)
        .with_context(|| format!("Failed to read debug log directory {}", log_dir.display()))?
    {
        let entry = match entry {
            Ok(value) => value,
            Err(err) => {
                eprintln!(
                    "warning: failed to read a debug log entry in {}: {}",
                    log_dir.display(),
                    err
                );
                continue;
            }
        };
        let path = entry.path();
        if !is_debug_log_file(&path) {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(value) => value,
            Err(err) => {
                eprintln!(
                    "warning: failed to read debug log metadata {}: {}",
                    path.display(),
                    err
                );
                continue;
            }
        };
        if !metadata.is_file() {
            continue;
        }
        if metadata.modified().unwrap_or(UNIX_EPOCH) <= cutoff
            && let Err(err) = fs::remove_file(&path)
        {
            eprintln!(
                "warning: failed to remove expired debug log {}: {}",
                path.display(),
                err
            );
        }
    }

    Ok(())
}

fn rotate_debug_log_if_needed(log_file: &Path, session_id: &str, max_size_mb: u64) -> Result<()> {
    if max_size_mb == 0 {
        return Ok(());
    }

    let max_bytes = max_size_mb.saturating_mul(DEBUG_BYTES_PER_MB);
    let metadata = match fs::metadata(log_file) {
        Ok(value) => value,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("Failed to inspect debug log {}", log_file.display()));
        }
    };

    if metadata.len() < max_bytes {
        return Ok(());
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let normalized_session_id = sanitize_debug_component(session_id, "session");
    let rotated_name = format!(
        "{DEBUG_LOG_FILE_PREFIX}{normalized_session_id}-rotated-{}-{}.log",
        timestamp,
        std::process::id()
    );
    let rotated_path = log_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(rotated_name);

    fs::rename(log_file, &rotated_path).with_context(|| {
        format!(
            "Failed to rotate debug log {} -> {}",
            log_file.display(),
            rotated_path.display()
        )
    })?;
    Ok(())
}

pub(super) fn prepare_debug_log_file(
    configured_dir: Option<PathBuf>,
    session_id: &str,
    max_size_mb: u64,
    max_age_days: u32,
) -> Result<PathBuf> {
    let log_dir = configured_dir.unwrap_or_else(default_debug_log_dir);
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("Failed to create debug log directory {}", log_dir.display()))?;
    prune_expired_debug_logs(&log_dir, max_age_days)?;
    let log_file = log_dir.join(debug_log_file_name(session_id));
    rotate_debug_log_if_needed(&log_file, session_id, max_size_mb)?;
    Ok(log_file)
}
