use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use super::SESSION_FILE_EXTENSION;
use super::identifier::session_archive_path_for_identifier;
use super::retention::apply_session_retention_best_effort;
use super::settings::{SESSION_DIR_ENV, read_env_var_os};
use super::snapshot::{SessionListing, SessionSnapshot};
use vtcode_commons::VtCodePaths;
use vtcode_commons::fs::read_private_json_file;

pub(crate) async fn resolve_sessions_dir_for_archive_writes() -> Result<PathBuf> {
    let sessions_dir = resolve_sessions_dir().await?;
    apply_session_retention_best_effort(&sessions_dir);
    Ok(sessions_dir)
}

pub async fn list_recent_sessions(limit: usize) -> Result<Vec<SessionListing>> {
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

    // Process session files in parallel for better performance with large archives
    // Batch processing to avoid overwhelming the system with too many concurrent tasks
    const BATCH_SIZE: usize = 10;
    let mut all_listings = Vec::new();

    for batch in session_paths.chunks(BATCH_SIZE) {
        let mut tasks = Vec::with_capacity(batch.len());

        for path in batch {
            let path = path.clone();
            let task = tokio::task::spawn(async move {
                read_private_json_file::<SessionSnapshot>(&path)
                    .await
                    .ok()
                    .map(|snapshot| SessionListing { path, snapshot })
            });
            tasks.push(task);
        }

        // Collect results from this batch
        for task in tasks {
            if let Ok(Some(listing)) = task.await {
                all_listings.push(listing);
            }
        }
    }

    // Sort and limit results
    all_listings.sort_by_key(|a| std::cmp::Reverse(a.snapshot.ended_at));
    if limit > 0 && all_listings.len() > limit {
        all_listings.truncate(limit);
    }

    Ok(all_listings)
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

    // Fall back to searching recent sessions by identifier substring
    let listings = list_recent_sessions(200).await?;
    let normalized_query = id_or_name.to_lowercase();

    for listing in &listings {
        // Match against the file stem (session identifier)
        if let Some(stem) = listing.path.file_stem()
            && let Some(stem_str) = stem.to_str()
            && stem_str.to_lowercase().contains(&normalized_query)
        {
            return Ok(listing.clone());
        }
    }

    Err(anyhow::anyhow!("No saved chat found matching '{id_or_name}'."))
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
