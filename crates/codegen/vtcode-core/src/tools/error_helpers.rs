//! Shared error helpers to reduce repetitive .with_context() patterns

use anyhow::{Context, Result};
use std::fmt::Display;
use std::path::Path;

// ---------------------------------------------------------------------------
// Shared guidance constants — single source of truth for tool-facing messages
// ---------------------------------------------------------------------------

/// Standard hint when the model sends a patch payload to the wrong tool.
pub const PATCH_PARAMETER_HINT: &str = "(use 'input' or 'patch' parameter)";

/// Canonical guidance for routing large edits away from edit_file.
pub const USE_PATCH_FOR_LARGE_EDITS: &str = "For larger edits, use apply_patch instead.";

/// Canonical guidance for routing large-file edits away from edit_file.
pub const USE_PATCH_OR_WRITE_FOR_LARGE_FILES: &str =
    "For large files, use apply_patch or a dedicated write operation.";

/// Canonical guidance to read a file before attempting a precise edit or patch.
pub const READ_FILE_FIRST_HINT: &str =
    "Use read_file first to get the exact text, then copy it precisely.";

/// Wrap a file operation result with standardized path context.
/// Works with any `Result<T, E>` where `E` implements `std::error::Error`.
pub fn with_file_context<T, E>(
    result: std::result::Result<T, E>,
    operation: impl Display,
    path: &Path,
) -> Result<T>
where
    E: std::error::Error + Send + Sync + 'static,
{
    result.with_context(|| format!("Failed to {operation} '{}'", path.display()))
}

/// Like [`with_file_context`] but accepts a string path instead of `&Path`.
pub fn with_path_context<T, E>(
    result: std::result::Result<T, E>,
    operation: impl Display,
    path: impl Display,
) -> Result<T>
where
    E: std::error::Error + Send + Sync + 'static,
{
    result.with_context(|| format!("Failed to {operation} {path}"))
}

/// Extract a required string field from JSON args, with a consistent error message
#[cold]
pub fn require_string_field(
    args: &serde_json::Value,
    field: &str,
    tool_name: &str,
) -> Result<String> {
    args.get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("{field} is required for {tool_name}"))
}

/// Extract an optional string field from JSON args
pub fn optional_string_field(args: &serde_json::Value, field: &str) -> Option<String> {
    args.get(field).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Extract a required integer field from JSON args
#[cold]
pub fn require_int_field(args: &serde_json::Value, field: &str, tool_name: &str) -> Result<i64> {
    args.get(field)
        .and_then(|v| v.as_i64())
        .ok_or_else(|| anyhow::anyhow!("{field} is required for {tool_name}"))
}

/// Extract a required value from an `Option<T>`, with a consistent error message
/// referencing the field name and tool/operation name.
#[cold]
pub fn require_field<T>(value: Option<T>, field: &str, tool_name: &str) -> Result<T> {
    value.ok_or_else(|| anyhow::anyhow!("{field} is required for {tool_name}"))
}

/// Deserialize JSON tool arguments into a typed struct with a consistent error message.
///
/// Replaces the repeated pattern of `serde_json::from_value(args).context("Error: Invalid 'X' arguments...")`
/// scattered across tool implementations.
pub fn deserialize_tool_args<T: serde::de::DeserializeOwned>(
    args: &serde_json::Value,
    tool_name: &str,
) -> Result<T> {
    serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("Error: Invalid '{tool_name}' arguments: {e}"))
}
