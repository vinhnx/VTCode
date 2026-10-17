use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use anyhow::Result;
use chrono::{DateTime, Utc};

use super::metadata::SessionArchiveMetadata;
use super::{SESSION_FILE_EXTENSION, SESSION_FILE_PREFIX, resolve_sessions_dir_for_archive_writes};

pub(crate) fn generate_unique_archive_path(
    sessions_dir: &Path,
    metadata: &SessionArchiveMetadata,
    started_at: DateTime<Utc>,
    custom_suffix: Option<&str>,
) -> PathBuf {
    generate_unique_archive_path_for_label(sessions_dir, &metadata.workspace_label, started_at, custom_suffix)
}

fn generate_unique_archive_path_for_label(
    sessions_dir: &Path,
    workspace_label: &str,
    started_at: DateTime<Utc>,
    custom_suffix: Option<&str>,
) -> PathBuf {
    if custom_suffix.is_some() {
        return sessions_dir.join(archive_file_name_for_label(workspace_label, started_at, custom_suffix, None));
    }

    let mut attempt = 0u32;
    loop {
        let candidate =
            sessions_dir.join(archive_file_name_for_label(workspace_label, started_at, None, Some(attempt)));
        if !candidate.exists() {
            return candidate;
        }
        attempt = attempt.wrapping_add(1);
    }
}

fn archive_file_name_for_label(
    workspace_label: &str,
    started_at: DateTime<Utc>,
    custom_suffix: Option<&str>,
    attempt: Option<u32>,
) -> String {
    let sanitized_label = sanitize_component(workspace_label);
    let timestamp = started_at.format("%Y%m%dT%H%M%SZ").to_string();

    if let Some(suffix) = custom_suffix {
        return format!(
            "{}-{}-{}-{}.{}",
            SESSION_FILE_PREFIX,
            sanitized_label,
            timestamp,
            sanitize_component(suffix),
            SESSION_FILE_EXTENSION
        );
    }

    let micros = started_at.timestamp_subsec_micros();
    let pid = process::id();
    let attempt_suffix = match attempt.unwrap_or_default() {
        0 => String::new(),
        value => format!("-{value:02}"),
    };
    format!(
        "{SESSION_FILE_PREFIX}-{sanitized_label}-{timestamp}_{micros:06}-{pid:05}{attempt_suffix}.{SESSION_FILE_EXTENSION}"
    )
}

pub fn generate_session_archive_identifier(workspace_label: &str, custom_suffix: Option<String>) -> String {
    let file_name = archive_file_name_for_label(workspace_label, Utc::now(), custom_suffix.as_deref(), Some(0));
    Path::new(&file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("session-{}-{}", sanitize_component(workspace_label), process::id()))
}

fn session_identifier_from_archive_path(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow::anyhow!("failed to derive session identifier from archive path"))
}

fn is_valid_session_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

pub(crate) fn validate_session_identifier(session_identifier: &str) -> Result<()> {
    if is_valid_session_identifier(session_identifier) {
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "Invalid session identifier '{session_identifier}': only ASCII letters, digits, '-' and '_' are allowed"
    ))
}

pub(crate) fn session_archive_path_for_identifier(sessions_dir: &Path, session_identifier: &str) -> Result<PathBuf> {
    validate_session_identifier(session_identifier)?;
    Ok(sessions_dir.join(format!("{session_identifier}.{SESSION_FILE_EXTENSION}")))
}

pub(crate) fn reserve_new_session_archive_path(sessions_dir: &Path, session_identifier: &str) -> Result<PathBuf> {
    let path = session_archive_path_for_identifier(sessions_dir, session_identifier)?;
    if fs::symlink_metadata(&path).is_ok() {
        return Err(anyhow::anyhow!("Session archive identifier '{session_identifier}' already exists"));
    }

    Ok(path)
}

/// Reserve a unique session archive identifier for the current process.
///
/// The returned identifier is the JSON file stem (without `.json`) and can be reused
/// to create an archive and pair external artifacts (for example debug logs).
pub async fn reserve_session_archive_identifier(
    workspace_label: &str,
    custom_suffix: Option<String>,
) -> Result<String> {
    let sessions_dir = resolve_sessions_dir_for_archive_writes().await?;
    let started_at = Utc::now();
    let path =
        generate_unique_archive_path_for_label(&sessions_dir, workspace_label, started_at, custom_suffix.as_deref());
    session_identifier_from_archive_path(&path)
}

fn sanitize_component(value: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if matches!(ch, '-' | '_') {
            if !last_was_separator {
                normalized.push(ch);
                last_was_separator = true;
            }
        } else if !last_was_separator {
            normalized.push('-');
            last_was_separator = true;
        }
    }

    let trimmed = normalized.trim_matches(|c| c == '-' || c == '_');
    if trimmed.is_empty() {
        "workspace".to_owned()
    } else {
        trimmed.to_owned()
    }
}
