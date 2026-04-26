//! File utility functions for common operations

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;

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
    serde_json::from_str(content).with_context(|| format!("Failed to parse JSON from {}", context))
}

/// Serialize JSON with context
pub fn serialize_json_with_context<T: Serialize>(data: &T, context: &str) -> Result<String> {
    serde_json::to_string(data).with_context(|| format!("Failed to serialize JSON for {}", context))
}

/// Serialize JSON pretty with context
pub fn serialize_json_pretty_with_context<T: Serialize>(data: &T, context: &str) -> Result<String> {
    serde_json::to_string_pretty(data)
        .with_context(|| format!("Failed to pretty-serialize JSON for {}", context))
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
