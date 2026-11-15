#![expect(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::let_underscore_must_use,
    unused_results,
    reason = "Filesystem helpers validate path lengths and intentionally ignore local cleanup results."
)]

//! File utility functions for common operations

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::image::has_supported_image_extension;

pub mod bound_file;

/// Ensure a directory exists, creating it if necessary
pub async fn ensure_dir_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)
            .await
            .with_context(|| format!("Failed to create directory: {}", path.display()))?;
    }
    Ok(())
}

/// Read a file with contextual error message
pub async fn read_file_with_context(path: &Path, context: &str) -> Result<String> {
    fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read {}: {}", context, path.display()))
}

/// Write a file with contextual error message, ensuring parent directory exists
pub async fn write_file_with_context(path: &Path, content: &str, context: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir_exists(parent).await?;
    }
    fs::write(path, content)
        .await
        .with_context(|| format!("Failed to write {}: {}", context, path.display()))
}

/// Write a file atomically with a contextual error message, ensuring the
/// parent directory exists.
///
/// The content is first written to a temporary file created in the same
/// directory as `path` (so the final rename stays on the same filesystem and
/// is therefore atomic), then the temp file is renamed onto `path`. This
/// prevents concurrent readers -- e.g. another vtcode process sharing the
/// same workspace -- from ever observing a partially written file.
///
/// On rename failure the temp file is best-effort removed before returning
/// the error.
pub async fn write_file_atomic_with_context(path: &Path, content: &str, context: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir_exists(parent).await?;
    }

    let temp_path = atomic_temp_path(path);

    fs::write(&temp_path, content)
        .await
        .with_context(|| format!("Failed to write {}: {}", context, temp_path.display()))?;

    if let Err(err) = fs::rename(&temp_path, path).await {
        let _ = fs::remove_file(&temp_path).await;
        return Err(err).with_context(|| format!("Failed to write {}: {}", context, path.display()));
    }

    Ok(())
}

/// Build a unique temp file path in the same directory as `path`, suitable
/// for a write-then-rename atomic publish of `path`.
fn atomic_temp_path(path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};

    static ATOMIC_WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("vtcode-atomic-write");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let counter = ATOMIC_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);

    dir.join(format!(".{file_name}.tmp-{}-{nanos:x}-{counter:x}", std::process::id()))
}

/// Write a JSON file
pub async fn write_json_file<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(data)
        .with_context(|| format!("Failed to serialize data for {}", path.display()))?;

    write_file_with_context(path, &json, "JSON data").await
}

/// Read a category-owned private file without following symlinks.
pub async fn read_private_file_no_follow(path: &Path) -> Result<Vec<u8>> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || crate::VtCodePaths::read_file_no_follow(&path))
        .await
        .context("private file read task panicked")?
}

/// Create a category-owned private file without following a final symlink.
pub async fn create_private_file(path: &Path) -> Result<std::fs::File> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || crate::VtCodePaths::create_private_file(&path))
        .await
        .context("private file creation task panicked")?
}

/// Atomically write a category-owned private file without following symlinks.
pub async fn write_private_file_atomic(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    let path = path.to_path_buf();
    let contents = contents.as_ref().to_vec();
    tokio::task::spawn_blocking(move || crate::VtCodePaths::write_private_file_atomic(&path, &contents))
        .await
        .context("private file write task panicked")?
}

/// Atomically create a category-owned private file when the destination is absent.
///
/// Returns `true` when this call published the file and `false` when another
/// writer had already created it.
pub async fn write_private_file_atomic_if_absent(path: &Path, contents: impl AsRef<[u8]>) -> Result<bool> {
    let path = path.to_path_buf();
    let contents = contents.as_ref().to_vec();
    tokio::task::spawn_blocking(move || crate::VtCodePaths::write_private_file_atomic_if_absent(&path, &contents))
        .await
        .context("private file create task panicked")?
}

/// Run a blocking operation while holding an exclusive private file lock.
pub async fn with_private_file_lock<T, F>(path: &Path, operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || crate::VtCodePaths::with_private_file_lock(&path, operation))
        .await
        .context("private file lock task panicked")?
}

/// Read and deserialize a category-owned private JSON file.
pub async fn read_private_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let contents = read_private_file_no_follow(path).await?;
    serde_json::from_slice(&contents).with_context(|| format!("Failed to parse private JSON from {}", path.display()))
}

/// Serialize and atomically write a category-owned private JSON file.
pub async fn write_private_json_file<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let json = serde_json::to_vec_pretty(data)
        .with_context(|| format!("Failed to serialize private JSON for {}", path.display()))?;
    write_private_file_atomic(path, json).await
}

/// Read and parse a JSON file
pub async fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let content = read_file_with_context(path, "JSON file").await?;

    serde_json::from_str(&content).with_context(|| format!("Failed to parse JSON from {}", path.display()))
}

/// Parse JSON with context for better error messages
pub fn parse_json_with_context<T: for<'de> Deserialize<'de>>(content: &str, context: &str) -> Result<T> {
    serde_json::from_str(content).with_context(|| format!("Failed to parse JSON from {context}"))
}

/// Serialize JSON with context
pub fn serialize_json_with_context<T: Serialize>(data: &T, context: &str) -> Result<String> {
    serde_json::to_string(data).with_context(|| format!("Failed to serialize JSON for {context}"))
}

/// Serialize JSON pretty with context
pub fn serialize_json_pretty_with_context<T: Serialize>(data: &T, context: &str) -> Result<String> {
    serde_json::to_string_pretty(data).with_context(|| format!("Failed to pretty-serialize JSON for {context}"))
}

/// Parse JSON into a typed value, returning `None` on failure.
///
/// Intended for non-critical, best-effort parsing where a missing or malformed
/// value should be silently ignored. Use `parse_json_with_context` when the
/// caller needs an actionable error.
#[must_use]
#[inline]
pub fn try_parse_json<T: for<'de> Deserialize<'de>>(input: &str) -> Option<T> {
    serde_json::from_str(input).ok()
}

/// Parse JSON into an untyped `Value`, returning `None` on failure.
///
/// Same semantics as `try_parse_json` but avoids a type annotation at the call
/// site when only dynamic inspection is needed.
#[must_use]
#[inline]
pub fn try_parse_json_value(input: &str) -> Option<serde_json::Value> {
    serde_json::from_str(input).ok()
}

/// Parse JSON into a typed value, falling back to `Default` on failure.
///
/// A parse failure is logged at `debug` level with the provided `label` so the
/// failure is visible in traces without being fatal.
#[inline]
pub fn parse_json_or_default<T: for<'de> Deserialize<'de> + Default>(input: &str, label: &str) -> T {
    serde_json::from_str(input).unwrap_or_else(|err| {
        tracing::debug!(label, %err, "JSON parse failed, using default");
        T::default()
    })
}

/// Canonicalize path with context.
///
/// Uses [`crate::paths::canonicalize`] (backed by `dunce`) to avoid Windows
/// `\\?\` verbatim prefixes from `std::fs::canonicalize`.
pub fn canonicalize_with_context(path: &Path, context: &str) -> Result<PathBuf> {
    crate::paths::canonicalize(path)
        .with_context(|| format!("Failed to canonicalize {} path: {}", context, path.display()))
}

/// Canonicalize path with context (async).
///
/// `dunce::canonicalize` is a synchronous syscall; we wrap it in
/// `spawn_blocking` to preserve the async interface without blocking the
/// runtime, matching the behaviour of `tokio::fs::canonicalize`.
pub async fn canonicalize_with_context_async(path: &Path, context: &str) -> Result<PathBuf> {
    let path = path.to_path_buf();
    let path_display = path.display().to_string();
    // `?` coerces JoinError → anyhow::Error via the blanket From impl.
    let result = tokio::task::spawn_blocking(move || crate::paths::canonicalize(&path)).await?;
    result.with_context(|| format!("Failed to canonicalize {context} path: {path_display}"))
}

/// Read a file to string with contextual error (async)
pub async fn read_to_string_async(path: &Path) -> Result<String> {
    fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read {}", path.display()))
}

/// Write a file with contextual error (async)
pub async fn write_async(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    fs::write(path, contents)
        .await
        .with_context(|| format!("Failed to write {}", path.display()))
}

/// Create directories recursively with contextual error (async)
pub async fn create_dir_all_async(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
        .await
        .with_context(|| format!("Failed to create {}", path.display()))
}

/// Remove a file with contextual error (async)
pub async fn remove_file_async(path: &Path) -> Result<()> {
    fs::remove_file(path)
        .await
        .with_context(|| format!("Failed to remove {}", path.display()))
}

/// Rename a file with contextual error (async)
pub async fn rename_async(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to)
        .await
        .with_context(|| format!("Failed to rename {} to {}", from.display(), to.display()))
}

// --- Sync Versions ---

/// Ensure a directory exists (sync)
pub fn ensure_dir_exists_sync(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path).with_context(|| format!("Failed to create directory: {}", path.display()))?;
    }
    Ok(())
}

/// Read a file with contextual error message (sync)
pub fn read_file_with_context_sync(path: &Path, context: &str) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("Failed to read {}: {}", context, path.display()))
}

/// Write a file with contextual error message (sync)
pub fn write_file_with_context_sync(path: &Path, content: &str, context: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir_exists_sync(parent)?;
    }
    std::fs::write(path, content).with_context(|| format!("Failed to write {}: {}", context, path.display()))
}

/// Write a JSON file (sync)
pub fn write_json_file_sync<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(data)
        .with_context(|| format!("Failed to serialize data for {}", path.display()))?;

    write_file_with_context_sync(path, &json, "JSON data")
}

/// Read and parse a JSON file (sync)
pub fn read_json_file_sync<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let content = read_file_with_context_sync(path, "JSON file")?;

    serde_json::from_str(&content).with_context(|| format!("Failed to parse JSON from {}", path.display()))
}

/// Check whether a path looks like an image file based on extension.
///
/// Matching is case-insensitive because macOS and Windows commonly use
/// case-insensitive filesystems, so `IMAGE.PNG` is still an image.
pub fn is_image_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "bmp" | "gif" | "jpeg" | "jpg" | "png" | "svg" | "tif" | "tiff" | "webp"
    )
}

/// Check whether a string is a Windows absolute path (e.g., `C:\...` or `C:/...`).
pub fn is_windows_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/')
}

/// Remove backslash-escaped whitespace from a token.
///
/// A backslash followed by an ASCII whitespace character is replaced by the
/// whitespace character itself.  All other characters are passed through.
pub fn unescape_whitespace(token: &str) -> String {
    let mut result = String::with_capacity(token.len());
    let mut chars = token.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\'
            && let Some(next) = chars.peek()
            && next.is_ascii_whitespace()
        {
            result.push(*next);
            chars.next();
            continue;
        }
        result.push(ch);
    }
    result
}

/// Trim trailing text from a raw image path match.
///
/// When a regex greedily matches an image path that contains spaces, it may
/// also consume trailing prose (e.g., "/path/to/image.png can you see").
/// This function walks backwards through whitespace-delimited tokens to find
/// the longest prefix that looks like a valid image path.
///
/// The `candidate_check` closure receives a trimmed candidate string and
/// returns `true` if it should be accepted as a valid image path.
pub fn trim_trailing_image_path<F>(raw: &str, candidate_check: F) -> &str
where
    F: Fn(&str) -> bool,
{
    if candidate_check(raw) {
        return raw;
    }
    let mut candidate = raw.trim_end();
    while let Some(last_space) = candidate.rfind(' ') {
        candidate = &candidate[..last_space];
        if candidate_check(candidate) {
            return candidate;
        }
    }
    raw
}

/// Convenience wrapper for [`trim_trailing_image_path`] that checks
/// image file extensions via [`has_supported_image_extension`].
///
/// Handles `file://` scheme and `~/` home expansion before checking.
pub fn trim_trailing_image_path_str(raw: &str) -> &str {
    trim_trailing_image_path(raw, |candidate| {
        let unescaped = unescape_whitespace(candidate);
        let mut path_str = unescaped.as_str();
        if let Some(rest) = path_str.strip_prefix("file://") {
            path_str = rest;
        }
        if let Some(rest) = path_str.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                return has_supported_image_extension(&home.join(rest));
            }
            return false;
        }
        has_supported_image_extension(Path::new(path_str))
    })
}

/// Durably sync a directory so recently created, renamed, or removed entries
/// survive a crash.
///
/// Syncing a file is not enough: the directory entry pointing at it must be
/// synced too. Pair with synced file writes in leaf-up order: sync each file,
/// rename it into place, then sync the parent directory. Errors propagate to
/// the caller; a failed sync must never be silently ignored, or durability is
/// lost without notice.
///
/// # Platform notes
///
/// Unix opens directories with a plain read-only handle. Windows requires
/// `FILE_FLAG_BACKUP_SEMANTICS` to open a directory at all, set here via
/// [`OpenOptionsExt::custom_flags`](std::os::windows::fs::OpenOptionsExt).
///
/// # Errors
///
/// Returns an error when the directory cannot be opened or synced.
pub fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(windows)]
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    #[cfg(not(windows))]
    let file = std::fs::File::open(path).with_context(|| format!("open directory {}", path.display()))?;
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .with_context(|| format!("open directory {}", path.display()))?
    };
    file.sync_all().with_context(|| format!("sync directory {}", path.display()))?;
    Ok(())
}

/// Rename a file or directory and durably sync the affected parents.
///
/// A bare rename can vanish on power loss even when the file contents were
/// synced. This syncs the source parent and the destination parent (once when
/// they are the same directory) so the namespace change survives a crash.
/// Cross-filesystem renames fail, like [`std::fs::rename`].
///
/// # Errors
///
/// Returns an error when the rename fails or either parent cannot be synced.
/// Callers must treat a failed rename as "unknown state" and re-inspect both
/// paths rather than assuming either outcome.
pub fn rename_synced(source: &Path, destination: &Path) -> Result<()> {
    std::fs::rename(source, destination)
        .with_context(|| format!("rename {} to {}", source.display(), destination.display()))?;
    let source_parent = source
        .parent()
        .ok_or_else(|| anyhow::anyhow!("rename source {} has no parent", source.display()))?;
    let destination_parent = destination
        .parent()
        .ok_or_else(|| anyhow::anyhow!("rename destination {} has no parent", destination.display()))?;
    sync_dir(source_parent).with_context(|| format!("sync rename source parent {}", source_parent.display()))?;
    if source_parent != destination_parent {
        sync_dir(destination_parent)
            .with_context(|| format!("sync rename destination parent {}", destination_parent.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_dir_succeeds_on_existing_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        sync_dir(dir.path()).expect("sync tempdir");
    }

    #[test]
    fn sync_dir_fails_on_missing_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("no-such-dir");
        let err = sync_dir(&missing).expect_err("missing dir must fail");
        assert!(err.to_string().contains("no-such-dir"), "unexpected error: {err:#}");
    }

    #[test]
    fn rename_synced_moves_within_one_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("before.txt");
        let destination = dir.path().join("after.txt");
        std::fs::write(&source, "payload").expect("write source");
        rename_synced(&source, &destination).expect("rename within dir");
        assert!(!source.exists());
        assert_eq!(std::fs::read_to_string(&destination).expect("read"), "payload");
    }

    #[test]
    fn rename_synced_moves_across_directories() {
        let root = tempfile::tempdir().expect("tempdir");
        let from_dir = root.path().join("from");
        let to_dir = root.path().join("to");
        std::fs::create_dir_all(&from_dir).expect("create from");
        std::fs::create_dir_all(&to_dir).expect("create to");
        let source = from_dir.join("file.txt");
        let destination = to_dir.join("file.txt");
        std::fs::write(&source, "payload").expect("write source");
        rename_synced(&source, &destination).expect("rename across dirs");
        assert!(!source.exists());
        assert_eq!(std::fs::read_to_string(&destination).expect("read"), "payload");
    }

    #[test]
    fn rename_synced_fails_on_missing_source() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = rename_synced(&dir.path().join("absent.txt"), &dir.path().join("dest.txt"))
            .expect_err("missing source must fail");
        assert!(err.to_string().contains("absent.txt"), "unexpected error: {err:#}");
        assert!(!dir.path().join("dest.txt").exists());
    }
}
