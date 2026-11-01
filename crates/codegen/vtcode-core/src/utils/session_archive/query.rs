use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use serde::Serialize;

use super::SESSION_FILE_EXTENSION;
use super::identifier::session_archive_path_for_identifier;
use super::settings::{SESSION_DIR_ENV, read_env_var_os};
use super::snapshot::{
    SessionArchiveSummary, SessionListHeader, SessionListing, SessionSnapshot, normalize_list_query_workspace,
    raw_workspace_path_matches_query,
};
use chrono::{DateTime, Utc};
use vtcode_commons::VtCodePaths;
use vtcode_commons::fs::{read_private_file_no_follow, read_private_json_file};

pub(crate) async fn resolve_sessions_dir_for_archive_writes() -> Result<PathBuf> {
    resolve_sessions_dir().await
}

pub async fn list_recent_sessions(limit: usize) -> Result<Vec<SessionListing>> {
    list_recent_sessions_matching(limit, None, |listing| listing).await
}

/// List the most recent archives for a workspace, applying the limit after workspace filtering.
pub async fn list_recent_sessions_in_workspace(limit: usize, workspace: &Path) -> Result<Vec<SessionListing>> {
    list_recent_sessions_matching(limit, Some(workspace), |listing| listing).await
}

/// List recent archives for a workspace while retaining only their navigation metadata.
pub async fn list_recent_session_summaries_in_workspace(
    limit: usize,
    workspace: &Path,
) -> Result<Vec<SessionArchiveSummary>> {
    list_recent_sessions_matching(limit, Some(workspace), SessionListing::into_summary).await
}

/// One archive's filter/sort header plus its raw bytes, retained so the
/// surviving entries can be fully parsed without a second read.
struct ScannedSession {
    path: PathBuf,
    ended_at: DateTime<Utc>,
    workspace_path: String,
    bytes: Vec<u8>,
}

/// Scan every archive file into headers, newest first.
///
/// Header parsing skips transcript/message/progress allocation; corrupt files
/// are skipped exactly as a failed full parse would skip them.
async fn scan_session_headers() -> Result<Vec<ScannedSession>> {
    let sessions_dir = match resolve_sessions_dir().await {
        Ok(dir) => dir,
        Err(_) => return Ok(Vec::new()),
    };

    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    // `std::fs::read_dir` is a blocking directory scan; run it off the async
    // executor and collect paths before spawning parallel read tasks. See `# Blocking`
    // docs in `src/agent/runloop/git.rs`.
    let session_paths: Vec<PathBuf> = tokio::task::spawn_blocking(move || -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for entry in fs::read_dir(&sessions_dir)
            .with_context(|| format!("failed to read session directory: {}", sessions_dir.display()))?
        {
            let entry = entry.with_context(|| format!("failed to read session entry in {}", sessions_dir.display()))?;
            let path = entry.path();
            if is_session_file(&path) {
                paths.push(path);
            }
        }
        Ok(paths)
    })
    .await
    .context("session directory scan task panicked")??;

    // Parse headers in parallel for better performance with large archives
    // Batch processing to avoid overwhelming the system with too many concurrent tasks
    const BATCH_SIZE: usize = 10;
    let mut scanned = Vec::new();

    for batch in session_paths.chunks(BATCH_SIZE) {
        let mut tasks = Vec::with_capacity(batch.len());

        for path in batch {
            let path = path.clone();
            let task = tokio::task::spawn(async move {
                let bytes = match read_private_file_no_follow(&path).await {
                    Ok(bytes) => bytes,
                    Err(_) => return None,
                };
                let header: SessionListHeader = match serde_json::from_slice(&bytes) {
                    Ok(header) => header,
                    Err(_) => return None,
                };
                Some(ScannedSession {
                    path,
                    ended_at: header.ended_at,
                    workspace_path: header.metadata.workspace_path,
                    bytes,
                })
            });
            tasks.push(task);
        }

        // Collect results from this batch
        for task in tasks {
            if let Ok(Some(entry)) = task.await {
                scanned.push(entry);
            }
        }
    }

    scanned.sort_by_key(|entry| std::cmp::Reverse(entry.ended_at));
    Ok(scanned)
}

async fn list_recent_sessions_matching<T, Project>(
    limit: usize,
    workspace: Option<&Path>,
    project: Project,
) -> Result<Vec<T>>
where
    T: Send + 'static,
    Project: Fn(SessionListing) -> T + Copy + Send + 'static,
{
    let total_started = Instant::now();
    let scan_started = Instant::now();
    let scanned = scan_session_headers().await?;
    let scan_elapsed = scan_started.elapsed();

    // Filter and limit on headers, then fully parse only the survivors.
    // Surviving entries that fail the full parse are skipped and backfilled
    // from the next-ranked headers so `limit` keeps its exact meaning.
    let normalized_workspace = workspace.map(normalize_list_query_workspace);
    let take = if limit > 0 { limit } else { usize::MAX };
    let mut selected = Vec::with_capacity(take.min(scanned.len()));
    let mut matching_sessions = 0;
    let materialize_started = Instant::now();

    for entry in &scanned {
        if normalized_workspace
            .as_deref()
            .is_some_and(|query| !raw_workspace_path_matches_query(&entry.workspace_path, query))
        {
            continue;
        }
        matching_sessions += 1;
        if selected.len() >= take {
            continue;
        }
        let snapshot: SessionSnapshot = match serde_json::from_slice(&entry.bytes) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::debug!(
                    path = %entry.path.display(),
                    %error,
                    "skipping archive that fails full snapshot parse"
                );
                continue;
            }
        };
        selected.push(project(SessionListing { path: entry.path.clone(), snapshot }));
    }
    let materialize_elapsed = materialize_started.elapsed();

    tracing::debug!(
        target: "vtcode::session_archive",
        scanned_files = scanned.len(),
        matching_sessions,
        returned_sessions = selected.len(),
        workspace_filtered = workspace.is_some(),
        header_scan = ?scan_elapsed,
        materialize = ?materialize_elapsed,
        total = ?total_started.elapsed(),
        "listed session archives"
    );

    Ok(selected)
}

/// Find a session archive by its identifier (file stem) without needing to list all sessions.
pub async fn find_session_by_identifier(identifier: &str) -> Result<Option<SessionListing>> {
    let sessions_dir = match resolve_sessions_dir().await {
        Ok(dir) => dir,
        Err(_) => return Ok(None),
    };

    if !sessions_dir.exists() {
        return Ok(None);
    }

    let path = match session_archive_path_for_identifier(&sessions_dir, identifier) {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => return Ok(None),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to inspect session archive {}", path.display()));
        }
    }

    let snapshot: SessionSnapshot = read_private_json_file(&path).await?;
    Ok(Some(SessionListing { path, snapshot }))
}

pub(crate) fn write_private_json_file_sync<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let contents =
        serde_json::to_vec_pretty(value).with_context(|| format!("failed to serialize {}", path.display()))?;
    VtCodePaths::write_private_file_atomic(path, &contents)
        .with_context(|| format!("failed to write private session archive {}", path.display()))
}

/// Find a saved session by ID or name for TUI `/resume` command.
///
/// Searches both the session identifier (UUID or file stem) and matches
/// against recent sessions. Returns a descriptive error message when no
/// matching session is found.
pub async fn find_session_by_id_or_name(id_or_name: &str) -> Result<SessionListing> {
    // First try exact identifier match (UUID or file stem)
    if let Some(listing) = find_session_by_identifier(id_or_name).await? {
        return Ok(listing);
    }

    // Fall back to searching recent sessions by identifier substring.
    // The header scan is newest-first like the previous list-then-scan order
    // with the same 200-entry recency window (counted over headers now, so a
    // match ranked past 200 headers by corrupt newer archives is missed), and
    // fully parses only the winner.
    let normalized_query = id_or_name.to_lowercase();

    for entry in scan_session_headers().await?.into_iter().take(200) {
        // Match against the file stem (session identifier)
        let stem_matches = entry
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.to_lowercase().contains(&normalized_query));
        if !stem_matches {
            continue;
        }
        let snapshot: SessionSnapshot = match serde_json::from_slice(&entry.bytes) {
            Ok(snapshot) => snapshot,
            Err(_) => continue,
        };
        return Ok(SessionListing { path: entry.path, snapshot });
    }

    Err(anyhow::anyhow!("No saved chat found matching '{id_or_name}'."))
}

/// Delete one session archive by identifier. Returns false when no archive
/// file exists. Invalid identifiers are rejected before any filesystem access.
pub async fn delete_session_archive(identifier: &str) -> Result<bool> {
    let sessions_dir = match resolve_sessions_dir().await {
        Ok(dir) => dir,
        Err(_) => return Ok(false),
    };

    if !sessions_dir.exists() {
        return Ok(false);
    }

    let path = session_archive_path_for_identifier(&sessions_dir, identifier)?;
    match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => return Ok(false),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to inspect session archive {}", path.display()));
        }
    }

    tokio::fs::remove_file(&path)
        .await
        .with_context(|| format!("failed to delete session archive {}", path.display()))?;
    Ok(true)
}

/// Resolve the session archive directory for storage accounting.
pub async fn sessions_dir() -> Result<PathBuf> {
    resolve_sessions_dir().await
}

async fn resolve_sessions_dir() -> Result<PathBuf> {
    if let Some(custom) = read_env_var_os(SESSION_DIR_ENV) {
        let path = PathBuf::from(custom);
        VtCodePaths::ensure_user_dir(&path)
            .with_context(|| format!("failed to initialize session directory {}", path.display()))?;
        return Ok(path);
    }

    let paths = VtCodePaths::resolve().context("failed to resolve VT Code paths")?;
    paths
        .ensure_state_dir()
        .context("failed to initialize VT Code state directory")?;
    let dir = paths.state_path("sessions")?;
    VtCodePaths::ensure_user_dir(&dir)
        .with_context(|| format!("failed to initialize session directory {}", dir.display()))?;
    Ok(dir)
}

pub(crate) fn is_session_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some(ext)
            if ext.eq_ignore_ascii_case(SESSION_FILE_EXTENSION)
                || ext.eq_ignore_ascii_case("jsonl")
                || ext.eq_ignore_ascii_case("log")
    )
}

#[cfg(test)]
mod tests;
