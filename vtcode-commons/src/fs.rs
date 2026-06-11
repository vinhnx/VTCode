//! File utility functions for common operations

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::image::has_supported_image_extension;

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

/// Write a JSON file
pub async fn write_json_file<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(data)
        .with_context(|| format!("Failed to serialize data for {}", path.display()))?;

    write_file_with_context(path, &json, "JSON data").await
}

/// Read and parse a JSON file
pub async fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let content = read_file_with_context(path, "JSON file").await?;

    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse JSON from {}", path.display()))
}

/// Parse JSON with context for better error messages
pub fn parse_json_with_context<T: for<'de> Deserialize<'de>>(
    content: &str,
    context: &str,
) -> Result<T> {
    serde_json::from_str(content).with_context(|| format!("Failed to parse JSON from {context}"))
}

/// Serialize JSON with context
pub fn serialize_json_with_context<T: Serialize>(data: &T, context: &str) -> Result<String> {
    serde_json::to_string(data).with_context(|| format!("Failed to serialize JSON for {context}"))
}

/// Serialize JSON pretty with context
pub fn serialize_json_pretty_with_context<T: Serialize>(data: &T, context: &str) -> Result<String> {
    serde_json::to_string_pretty(data)
        .with_context(|| format!("Failed to pretty-serialize JSON for {context}"))
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
pub fn parse_json_or_default<T: for<'de> Deserialize<'de> + Default>(
    input: &str,
    label: &str,
) -> T {
    serde_json::from_str(input).unwrap_or_else(|err| {
        tracing::debug!(label, %err, "JSON parse failed, using default");
        T::default()
    })
}

/// Canonicalize path with context
pub fn canonicalize_with_context(path: &Path, context: &str) -> Result<PathBuf> {
    path.canonicalize().with_context(|| {
        format!(
            "Failed to canonicalize {} path: {}",
            context,
            path.display()
        )
    })
}

/// Canonicalize path with context (async)
pub async fn canonicalize_with_context_async(path: &Path, context: &str) -> Result<PathBuf> {
    fs::canonicalize(path).await.with_context(|| {
        format!(
            "Failed to canonicalize {} path: {}",
            context,
            path.display()
        )
    })
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
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create directory: {}", path.display()))?;
    }
    Ok(())
}

/// Read a file with contextual error message (sync)
pub fn read_file_with_context_sync(path: &Path, context: &str) -> Result<String> {
    std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}: {}", context, path.display()))
}

/// Write a file with contextual error message (sync)
pub fn write_file_with_context_sync(path: &Path, content: &str, context: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir_exists_sync(parent)?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write {}: {}", context, path.display()))
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

    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse JSON from {}", path.display()))
}

/// Check whether a path looks like an image file based on extension.
pub fn is_image_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        extension,
        _ if extension.eq_ignore_ascii_case("png")
            || extension.eq_ignore_ascii_case("jpg")
            || extension.eq_ignore_ascii_case("jpeg")
            || extension.eq_ignore_ascii_case("gif")
            || extension.eq_ignore_ascii_case("bmp")
            || extension.eq_ignore_ascii_case("webp")
            || extension.eq_ignore_ascii_case("tiff")
            || extension.eq_ignore_ascii_case("tif")
            || extension.eq_ignore_ascii_case("svg")
    )
}

/// Check whether a string is a Windows absolute path (e.g., `C:\...` or `C:/...`).
pub fn is_windows_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
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
