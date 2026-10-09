//! Tool Output Spooler for Dynamic Context Discovery
//!
//! Implements Cursor-style dynamic context discovery by writing large tool outputs
//! to files instead of truncating them. This allows agents to retrieve the full
//! output with bounded shell inspection or a focused source query when needed.
//!
//! ## Design Philosophy
//!
//! Instead of truncating large tool responses (which loses data), we:
//! 1. Write the full output to `.vtcode/context/tool_outputs/{tool}_{timestamp}.txt`
//! 2. Return a file reference to the agent
//! 3. The agent can inspect the file with bounded `exec_command` reads
//!
//! This is more token-efficient as only necessary data is pulled into context.

use crate::config::constants::tools;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, info};
use vtcode_commons::formatting::truncate_byte_budget;
use vtcode_commons::preview::{
    condense_text_bytes, excerpt_text_lines_with_limit, format_hidden_lines_summary, tail_preview_text,
};
use vtcode_commons::serde_helpers::json_to_string_pretty;
use vtcode_commons::utils::calculate_sha256;

const TOOL_OUTPUT_DIR: &str = ".vtcode/context/tool_outputs";
const SPOOL_CREATE_ATTEMPTS: u32 = 16;

/// Default threshold for spooling tool output to files (8KB).
/// Keep this aligned with `DynamicContextConfig::default().tool_output_threshold`
/// so invalid workspace config falls back to the same runtime behavior.
pub const DEFAULT_SPOOL_THRESHOLD_BYTES: usize = 8_192;

const CONDENSE_HEAD_BYTES: usize = 8_000;
const CONDENSE_TAIL_BYTES: usize = 4_000;
const INSPECTION_PREVIEW_HEAD_BYTES: usize = 4 * 1024;
const INSPECTION_PREVIEW_TAIL_BYTES: usize = 2 * 1024;
const COMMAND_PREVIEW_MAX_BYTES: usize = 6 * 1024;
const COMMAND_PREVIEW_MAX_LINES: usize = 80;
const PREVIEW_NOTICE_RESERVE_BYTES: usize = 64;

fn condense_content(content: &str) -> String {
    condense_text_bytes(content, CONDENSE_HEAD_BYTES, CONDENSE_TAIL_BYTES)
}

fn render_line_bounded_preview(content: &str, max_lines: usize) -> String {
    let preview = excerpt_text_lines_with_limit(content, max_lines, max_lines / 2);
    if preview.hidden_count == 0 {
        return content.to_string();
    }

    let mut lines = Vec::with_capacity(preview.head.len() + preview.tail.len() + 1);
    lines.extend(preview.head.into_iter().map(str::to_string));
    lines.push(format_hidden_lines_summary(preview.hidden_count));
    lines.extend(preview.tail.into_iter().map(str::to_string));
    lines.join("\n")
}

fn command_preview_byte_limit(max_preview_bytes: Option<usize>) -> usize {
    max_preview_bytes
        .unwrap_or(COMMAND_PREVIEW_MAX_BYTES)
        .min(COMMAND_PREVIEW_MAX_BYTES)
}

fn inspection_preview_content(content: &str, max_preview_bytes: Option<usize>) -> String {
    let byte_limit = command_preview_byte_limit(max_preview_bytes);
    if byte_limit == 0 {
        return String::new();
    }

    let content_budget = byte_limit.saturating_sub(PREVIEW_NOTICE_RESERVE_BYTES).max(1);
    let head_bytes = content_budget.saturating_mul(2).div_ceil(3).min(INSPECTION_PREVIEW_HEAD_BYTES);
    let tail_bytes = content_budget.saturating_sub(head_bytes).min(INSPECTION_PREVIEW_TAIL_BYTES);
    let byte_bounded = condense_text_bytes(content, head_bytes, tail_bytes);
    let line_bounded = render_line_bounded_preview(&byte_bounded, COMMAND_PREVIEW_MAX_LINES);
    truncate_byte_budget(&line_bounded, byte_limit, "")
}

fn verification_preview_content(content: &str, max_preview_bytes: Option<usize>) -> String {
    let byte_limit = command_preview_byte_limit(max_preview_bytes);
    if byte_limit == 0 {
        return String::new();
    }

    let content_budget = byte_limit.saturating_sub(PREVIEW_NOTICE_RESERVE_BYTES).max(1);
    let preview = tail_preview_text(content, content_budget, COMMAND_PREVIEW_MAX_LINES.saturating_sub(1));
    truncate_byte_budget(&preview, byte_limit, "")
}

/// Render the bounded model-facing preview for command-session output already
/// held in memory by a response producer.
///
/// This deliberately accepts content rather than a spool path: consumers must
/// not reopen spool files while shaping the model response.
pub(crate) fn command_preview_content(
    tool_name: &str,
    response: &Value,
    content: &str,
    max_preview_bytes: Option<usize>,
) -> String {
    let args = json!({
        "action": "run",
        "command": response.get("command").cloned().unwrap_or(Value::Null),
    });
    match crate::tools::tool_intent::classify_shell_activity(tool_name, &args) {
        crate::tools::tool_intent::ShellActivity::Inspection => inspection_preview_content(content, max_preview_bytes),
        crate::tools::tool_intent::ShellActivity::Verification | crate::tools::tool_intent::ShellActivity::Mutation => {
            verification_preview_content(content, max_preview_bytes)
        }
    }
}

/// Which retention rules a cleanup pass applies.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum SpoolCleanupScope {
    /// Drop only files older than `max_age_secs`.
    AgeOnly,
    /// Age expiry plus the `max_files` count budget.
    AgeAndCount,
}

impl SpoolCleanupScope {
    fn enforces_count_budget(self) -> bool {
        matches!(self, SpoolCleanupScope::AgeAndCount)
    }
}

/// Configuration for the output spooler
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpoolerConfig {
    /// Enable spooling large outputs to files
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Threshold in bytes above which outputs are spooled to files
    #[serde(default = "default_threshold")]
    pub threshold_bytes: usize,

    /// Maximum number of spooled files to keep
    #[serde(default = "default_max_files")]
    pub max_files: usize,

    /// Maximum age in seconds before cleanup removes a spooled file
    #[serde(default = "default_max_age_secs")]
    pub max_age_secs: u64,

    /// Whether to include file reference in truncated output
    #[serde(default = "default_include_reference")]
    pub include_file_reference: bool,
}

fn default_enabled() -> bool {
    true
}

fn default_threshold() -> usize {
    DEFAULT_SPOOL_THRESHOLD_BYTES
}

fn default_max_files() -> usize {
    100
}

fn default_max_age_secs() -> u64 {
    3600
}

fn default_include_reference() -> bool {
    true
}

impl Default for SpoolerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_bytes: DEFAULT_SPOOL_THRESHOLD_BYTES,
            max_files: 100,
            max_age_secs: default_max_age_secs(),
            include_file_reference: true,
        }
    }
}

/// Result of spooling a tool output
#[derive(Debug, Clone)]
pub struct SpoolResult {
    /// Path to the spooled file (relative to workspace)
    pub file_path: PathBuf,
    /// Original size in bytes
    pub original_bytes: usize,
    /// Exact number of bytes committed to the immutable spool.
    pub byte_count: u64,
    /// SHA-256 digest of the committed bytes.
    pub sha256: String,
    /// Lifecycle state of this spool reference.
    pub state: SpoolState,
    /// Full content written to the spool file
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpoolState {
    Pending,
    Completed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpoolIntegrity {
    pub byte_count: u64,
    pub line_count: u64,
    pub sha256: String,
}

/// Count physical lines across committed chunks, including an unterminated last line.
#[derive(Default)]
pub(crate) struct SpoolLineCounter {
    newline_count: u64,
    last_byte: Option<u8>,
}

impl SpoolLineCounter {
    pub(crate) fn append(&mut self, bytes: &[u8]) {
        self.newline_count = self
            .newline_count
            .saturating_add(bytes.iter().filter(|byte| **byte == b'\n').count() as u64);
        if let Some(byte) = bytes.last() {
            self.last_byte = Some(*byte);
        }
    }

    pub(crate) fn line_count(&self) -> u64 {
        self.newline_count
            .saturating_add(u64::from(self.last_byte.is_some_and(|byte| byte != b'\n')))
    }
}

pub(crate) fn encode_digest_hex(digest: impl AsRef<[u8]>) -> String {
    use std::fmt::Write as _;

    let bytes = digest.as_ref();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

/// Validated view of the bounded model-facing portion of a spooled result.
///
/// Consumers should use this interface instead of reopening `spool_path` just
/// to reconstruct data that the spooler already produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpooledOutputReference<'a> {
    pub spool_path: &'a str,
    pub preview: Option<&'a str>,
    pub original_bytes: Option<u64>,
    pub sha256: Option<&'a str>,
    pub state: SpoolState,
}

impl<'a> SpooledOutputReference<'a> {
    #[must_use]
    pub fn recognizes_path(path: &Path) -> bool {
        is_workspace_tool_output_path(path, true)
    }

    /// Return whether `path` names a direct child spool file rather than the
    /// spool directory itself. Read-file and replay consumers require this
    /// stricter shape; shell search commands may intentionally target the
    /// directory through [`Self::recognizes_path`].
    #[must_use]
    pub fn recognizes_file_path(path: &Path) -> bool {
        is_workspace_tool_output_path(path, false)
    }

    #[must_use]
    pub fn from_value(value: &'a Value) -> Option<Self> {
        let spool_path = value
            .get("spool_path")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|path| !path.is_empty())?;
        let preview = value
            .get("preview")
            .and_then(Value::as_str)
            .filter(|preview| !preview.trim().is_empty());
        let original_bytes = value.get("spooled_bytes").and_then(Value::as_u64);
        let sha256 = value.get("spool_sha256").and_then(Value::as_str);
        let state = match value.get("spool_state").and_then(Value::as_str) {
            Some("pending") => SpoolState::Pending,
            Some("completed") => SpoolState::Completed,
            _ if value.get("spool_pending").and_then(Value::as_bool) == Some(true) => SpoolState::Pending,
            _ if value.get("spool_complete").and_then(Value::as_bool) == Some(true) => SpoolState::Completed,
            _ => SpoolState::Unknown,
        };
        Some(Self { spool_path, preview, original_bytes, sha256, state })
    }

    /// Read an immutable completed spool after validating its workspace
    /// location, regular-file identity, byte count, and digest.
    pub fn read_verified_completed(&self, workspace_root: &Path) -> Result<String> {
        anyhow::ensure!(self.state == SpoolState::Completed, "spool is not marked completed");
        let expected_bytes = self.original_bytes.context("completed spool is missing byte count")?;
        let expected_sha256 = self.sha256.context("completed spool is missing SHA-256 digest")?;
        anyhow::ensure!(
            is_workspace_tool_output_path(Path::new(self.spool_path), false),
            "unrecognized tool output spool path"
        );

        let canonical_root = vtcode_commons::canonicalize(workspace_root)
            .with_context(|| format!("canonicalize workspace root {}", workspace_root.display()))?;
        let path = Path::new(self.spool_path);
        let relative = if path.is_absolute() {
            path.strip_prefix(&canonical_root)
                .context("absolute spool path is outside workspace")?
        } else {
            path
        };
        anyhow::ensure!(
            is_workspace_tool_output_path(relative, false),
            "spool path is outside the tool output directory"
        );

        let mut file = vtcode_commons::fs::bound_file::open_file_beneath(&canonical_root, relative)
            .with_context(|| format!("open completed spool {}", relative.display()))?;
        let metadata = file.metadata().context("read completed spool metadata")?;
        anyhow::ensure!(metadata.is_file(), "completed spool is not a regular file");
        anyhow::ensure!(metadata.len() == expected_bytes, "completed spool length mismatch");
        let capacity = usize::try_from(expected_bytes).context("completed spool is too large to read")?;
        let mut bytes = Vec::with_capacity(capacity);
        file.read_to_end(&mut bytes).context("read completed spool")?;
        anyhow::ensure!(bytes.len() as u64 == expected_bytes, "completed spool changed while reading");
        anyhow::ensure!(calculate_sha256(&bytes) == expected_sha256, "completed spool digest mismatch");
        String::from_utf8(bytes).context("completed spool is not valid UTF-8")
    }

    /// Read a bounded snapshot of a live append-only spool. Pending output is
    /// explicitly unverified because the producer may append after this read.
    pub fn read_pending_bounded(&self, workspace_root: &Path, max_bytes: usize) -> Result<String> {
        anyhow::ensure!(self.state == SpoolState::Pending, "spool is not marked pending");
        anyhow::ensure!(max_bytes > 0, "pending spool read limit must be positive");
        anyhow::ensure!(
            is_workspace_tool_output_path(Path::new(self.spool_path), false),
            "unrecognized tool output spool path"
        );
        let canonical_root = vtcode_commons::canonicalize(workspace_root)
            .with_context(|| format!("canonicalize workspace root {}", workspace_root.display()))?;
        let path = Path::new(self.spool_path);
        let relative = if path.is_absolute() {
            path.strip_prefix(&canonical_root)
                .context("absolute spool path is outside workspace")?
        } else {
            path
        };
        let file = vtcode_commons::fs::bound_file::open_file_beneath(&canonical_root, relative)
            .with_context(|| format!("open pending spool {}", relative.display()))?;
        let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
        file.take(max_bytes as u64)
            .read_to_end(&mut bytes)
            .context("read bounded pending spool")?;
        Ok(decode_pending_snapshot(bytes))
    }
}

/// Decode a bounded pending snapshot without turning a byte-boundary split
/// into a completely missing preview. Pending spools are normally valid UTF-8
/// because their producers write strings, but a byte cap can end inside the
/// final code point. Drop only that incomplete trailing sequence; if the
/// snapshot contains an invalid sequence elsewhere, preserve the readable
/// portions with the standard replacement character.
fn decode_pending_snapshot(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            let utf8_error = error.utf8_error();
            let bytes = error.into_bytes();
            if utf8_error.error_len().is_none() {
                String::from_utf8_lossy(&bytes[..utf8_error.valid_up_to()]).into_owned()
            } else {
                String::from_utf8_lossy(&bytes).into_owned()
            }
        }
    }
}

fn is_workspace_tool_output_path(path: &Path, allow_directory: bool) -> bool {
    // Tool arguments can replay a Windows path on Unix (or the reverse), so
    // recognize both separators while still comparing exact path components.
    let normalized = path.to_string_lossy().replace('\\', "/");
    let absolute = normalized.starts_with('/')
        || (normalized.as_bytes().get(1) == Some(&b':') && normalized.as_bytes().get(2) == Some(&b'/'));
    let mut components = Vec::new();
    for component in normalized.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return false;
        }
        components.push(component);
    }

    let needle = [".vtcode", "context", "tool_outputs"];
    if let Some(directory_start) = components.len().checked_sub(needle.len() + 1)
        && (absolute || directory_start == 0)
        && components[directory_start..directory_start + needle.len()] == needle
    {
        return true;
    }
    if !allow_directory {
        return false;
    }
    let Some(directory_start) = components.len().checked_sub(needle.len()) else {
        return false;
    };
    (absolute || directory_start == 0) && components[directory_start..] == needle
}

/// Fill in the bounded metadata required by a model-facing spool reference.
///
/// Command-session spools are written by the session manager rather than this
/// spooler, so they arrive with the storage marker but without the regular
/// spooler envelope. Keep that normalization at the spooler boundary so all
/// reference producers share the same preview and recovery metadata.
pub(crate) fn ensure_spooled_reference_metadata(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let Some(spool_path) = object
        .get("spool_path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
    else {
        return;
    };

    let preview_source = ["preview", "output", "raw_output", "stdout", "content"]
        .into_iter()
        .find_map(|field| {
            object
                .get(field)
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
        });
    if object
        .get("preview")
        .and_then(Value::as_str)
        .is_none_or(|preview| preview.is_empty())
        && let Some(preview) = preview_source.as_deref()
    {
        object.insert("preview".to_string(), Value::String(preview.to_string()));
    }

    let spooled_bytes = object
        .get("spooled_bytes")
        .and_then(Value::as_u64)
        .or_else(|| object.get("total_output_bytes").and_then(Value::as_u64))
        .or_else(|| preview_source.as_ref().map(|preview| preview.len() as u64));
    if let Some(bytes) = spooled_bytes {
        object.entry("spooled_bytes").or_insert_with(|| json!(bytes));
        let extent = object
            .get("spool_line_count")
            .and_then(Value::as_u64)
            .map(|lines| format!(", {lines} lines; EOF at line {lines}"))
            .unwrap_or_default();
        object.entry("spool_note").or_insert_with(|| {
            json!(format!(
                "Large output ({bytes} bytes{extent}) spooled to `{spool_path}`. Do not re-inline the full file. Use exec_command with sed or rg to inspect only missing sections; avoid overlapping reads."
            ))
        });
    } else {
        object.entry("spool_note").or_insert_with(|| {
            json!(format!(
                "Large output spooled to `{spool_path}`. Use exec_command with cat, sed, or rg to inspect only the sections you need."
            ))
        });
    }
}

/// How often (in number of spool operations) to run age-based cleanup.
const CLEANUP_EVERY_N_SPOOLS: u64 = 50;

/// Tool Output Spooler for writing large outputs to files
pub struct ToolOutputSpooler {
    /// Workspace root directory
    workspace_root: PathBuf,
    /// Output directory for spooled files
    output_dir: PathBuf,
    /// Configuration
    config: SpoolerConfig,
    /// Track spooled files for cleanup
    spooled_files: Arc<RwLock<Vec<PathBuf>>>,
    /// Pinned spool files that must survive cleanup/eviction because a
    /// blocked turn handoff references them. Bounded: pins are cleared when
    /// the owning session resolves its blocker or on explicit release.
    pinned_files: Arc<RwLock<std::collections::HashSet<PathBuf>>>,
    /// Counter for throttling periodic cleanup
    spool_count: std::sync::atomic::AtomicU64,
}

impl ToolOutputSpooler {
    /// Create a new spooler for the given workspace
    pub fn new(workspace_root: &Path) -> Self {
        Self::with_config(workspace_root, SpoolerConfig::default())
    }

    /// Create a new spooler with custom configuration
    pub fn with_config(workspace_root: &Path, config: SpoolerConfig) -> Self {
        let output_dir = workspace_root.join(TOOL_OUTPUT_DIR);

        let max_files = config.max_files;
        Self {
            workspace_root: workspace_root.to_path_buf(),
            output_dir,
            config,
            spooled_files: Arc::new(RwLock::new(Vec::with_capacity(max_files))),
            pinned_files: Arc::new(RwLock::new(std::collections::HashSet::new())),
            spool_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Check if a value should be spooled based on size
    pub fn should_spool(&self, value: &Value) -> bool {
        if !self.config.enabled {
            return false;
        }
        if value.get("no_spool").and_then(|v| v.as_bool()).unwrap_or(false) {
            return false;
        }
        self.estimate_size(value) > self.config.threshold_bytes
    }

    fn estimate_size(&self, value: &Value) -> usize {
        if let Some(s) = value.get("raw_output").and_then(|v| v.as_str()) {
            return s.len();
        }
        if let Some(s) = value.get("content").and_then(|v| v.as_str()) {
            return s.len();
        }
        if let Some(s) = value.get("output").and_then(|v| v.as_str()) {
            return s.len();
        }
        if let Some(s) = value.as_str() {
            return s.len();
        }
        value.to_string().len()
    }

    /// Spool a tool output to a file and return a reference
    pub async fn spool_output(&self, tool_name: &str, value: &Value, is_mcp: bool) -> Result<SpoolResult> {
        // Generate a collision-resistant filename. Creation below is
        // exclusive, so an existing immutable spool can never be truncated.
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros();

        // For file and command-session tools, extract raw content so the spooled file is directly usable.
        // This allows grep_file to work on the spooled output and makes reading more intuitive
        let content = if (tool_name == tools::READ_FILE || tool_name == tools::UNIFIED_FILE) && !is_mcp {
            if let Some(raw_content) = value.get("content").and_then(|v| v.as_str()) {
                raw_content.to_string()
            } else if let Some(json_str) = value.as_str() {
                // Edge case: value might be a JSON string that needs parsing
                if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
                    if let Some(raw_content) = parsed.get("content").and_then(|v| v.as_str()) {
                        debug!(
                            tool = tool_name,
                            "read_file spool: recovered content from double-serialized JSON string"
                        );
                        raw_content.to_string()
                    } else {
                        json_str.to_string()
                    }
                } else {
                    json_str.to_string()
                }
            } else {
                // Fallback to JSON serialization if no content field
                debug!(
                    tool = tool_name,
                    has_content = value.get("content").is_some(),
                    "read_file spool: could not extract content as string; falling back to JSON"
                );
                json_to_string_pretty(value)
            }
        } else if crate::tools::tool_intent::is_command_session_tool(tool_name) && !is_mcp {
            // For command-session tools and legacy PTY helpers,
            // extract the actual command output from the "output" field.
            // This ensures the spooled file contains the raw command output, not the JSON wrapper.
            //
            // Handle two cases:
            // 1. value is an object with "output" field (normal case)
            // 2. value is a string containing JSON (edge case: double-serialized)
            if let Some(output_content) = value.get("raw_output").and_then(|v| v.as_str()) {
                output_content.to_string()
            } else if let Some(output_content) = value.get("output").and_then(|v| v.as_str()) {
                output_content.to_string()
            } else if let Some(json_str) = value.as_str() {
                // Edge case: value might be a JSON string that needs parsing
                // This can happen if the value was serialized somewhere in the pipeline
                if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
                    if let Some(output_content) = parsed.get("raw_output").and_then(|v| v.as_str()) {
                        debug!(tool = tool_name, "PTY spool: recovered raw_output from double-serialized JSON string");
                        output_content.to_string()
                    } else if let Some(output_content) = parsed.get("output").and_then(|v| v.as_str()) {
                        debug!(tool = tool_name, "PTY spool: recovered output from double-serialized JSON string");
                        output_content.to_string()
                    } else {
                        // Parsed but no output field - use the parsed value's stdout if available
                        if let Some(stdout) = parsed.get("stdout").and_then(|v| v.as_str()) {
                            stdout.to_string()
                        } else {
                            json_str.to_string()
                        }
                    }
                } else {
                    // Not valid JSON - use the string as-is
                    json_str.to_string()
                }
            } else {
                // Fallback to JSON serialization if no output field
                debug!(
                    tool = tool_name,
                    has_output = value.get("output").is_some(),
                    output_type = ?value.get("output").map(|v| match v {
                        serde_json::Value::Null => "null",
                        serde_json::Value::Bool(_) => "bool",
                        serde_json::Value::Number(_) => "number",
                        serde_json::Value::String(_) => "string",
                        serde_json::Value::Array(_) => "array",
                        serde_json::Value::Object(_) => "object",
                    }),
                    "PTY spool: could not extract output as string; falling back to JSON"
                );
                json_to_string_pretty(value)
            }
        } else if let Some(s) = value.as_str() {
            s.to_string()
        } else {
            json_to_string_pretty(value)
        };

        // Sanitize content to redact any secrets before writing to disk
        let sanitized_content = vtcode_commons::sanitizer::redact_secrets(content);
        let original_bytes = sanitized_content.len();
        let byte_count = original_bytes as u64;
        let sha256 = calculate_sha256(sanitized_content.as_bytes());
        let canonical_root = vtcode_commons::canonicalize(&self.workspace_root)
            .with_context(|| format!("canonicalize spool workspace {}", self.workspace_root.display()))?;
        let sanitized_name = sanitize_tool_name(tool_name);
        let bytes = sanitized_content.as_bytes().to_vec();
        let (file_path, relative_path) = tokio::task::spawn_blocking(move || -> Result<(PathBuf, PathBuf)> {
            let output_dir = Path::new(TOOL_OUTPUT_DIR);
            vtcode_commons::fs::bound_file::ensure_directory_beneath(&canonical_root, output_dir)
                .context("create bound tool output directory")?;
            for collision in 0..SPOOL_CREATE_ATTEMPTS {
                let suffix = if collision == 0 {
                    String::new()
                } else {
                    format!("_{collision}")
                };
                let filename = format!("{sanitized_name}_{timestamp}{suffix}.txt");
                let relative = output_dir.join(filename);
                match vtcode_commons::fs::bound_file::write_file_beneath(&canonical_root, &relative, &bytes) {
                    Ok(()) => return Ok((canonical_root.join(&relative), relative)),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error).context("write immutable tool output spool"),
                }
            }
            anyhow::bail!("tool output spool filename collisions exceeded {SPOOL_CREATE_ATTEMPTS} attempts")
        })
        .await
        .context("join tool output spool writer")??;

        {
            let mut files = self.spooled_files.write().await;
            files.push(file_path.clone());

            if files.len() > self.config.max_files {
                let pinned = self.pinned_files.read().await;
                if let Some(evict_idx) = files.iter().position(|candidate| !pinned.contains(candidate)) {
                    let old_file = files.remove(evict_idx);
                    drop(pinned);
                    let _ = fs::remove_file(&old_file).await;
                }
            }
        }

        info!(
            tool = tool_name,
            bytes = original_bytes,
            path = %relative_path.display(),
            is_mcp = is_mcp,
            "Spooled large tool output to file"
        );

        Ok(SpoolResult {
            file_path: relative_path,
            original_bytes,
            byte_count,
            sha256,
            state: SpoolState::Completed,
            content: sanitized_content,
        })
    }

    /// Process a tool output, spooling if necessary.
    ///
    /// Returns the original value if below threshold, or a condensed
    /// head+tail payload with a `spool_path` reference if spooled.
    /// Triggers periodic age-based cleanup of old spooled files.
    pub async fn process_output(&self, tool_name: &str, value: Value, is_mcp: bool) -> Result<Value> {
        self.process_output_with_options(tool_name, value, is_mcp, false, None).await
    }

    /// Process a tool output, optionally forcing spool behavior.
    ///
    /// `force_spool=true` bypasses the size threshold but still respects explicit
    /// `no_spool=true` in the payload.
    pub async fn process_output_with_force(
        &self,
        tool_name: &str,
        value: Value,
        is_mcp: bool,
        force_spool: bool,
    ) -> Result<Value> {
        self.process_output_with_options(tool_name, value, is_mcp, force_spool, None)
            .await
    }

    pub(crate) async fn process_output_with_preview_limit(
        &self,
        tool_name: &str,
        value: Value,
        is_mcp: bool,
        force_spool: bool,
        max_preview_bytes: usize,
    ) -> Result<Value> {
        self.process_output_with_options(tool_name, value, is_mcp, force_spool, Some(max_preview_bytes))
            .await
    }

    async fn process_output_with_options(
        &self,
        tool_name: &str,
        value: Value,
        is_mcp: bool,
        force_spool: bool,
        max_preview_bytes: Option<usize>,
    ) -> Result<Value> {
        let no_spool = value.get("no_spool").and_then(|v| v.as_bool()).unwrap_or(false);
        if no_spool {
            return Ok(value);
        }
        if !self.config.enabled {
            return Ok(value);
        }
        if !force_spool && !self.should_spool(&value) {
            return Ok(value);
        }

        let spool_result = self.spool_output(tool_name, &value, is_mcp).await?;

        // Periodic age-based cleanup: run every CLEANUP_EVERY_N_SPOOLS spool operations.
        // This ensures stale files are removed without adding overhead on every call.
        let count = self.spool_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if count.is_multiple_of(CLEANUP_EVERY_N_SPOOLS) {
            if let Err(e) = self.cleanup_old_files().await {
                debug!(error = %e, "Periodic spool cleanup failed");
            }
        }
        let condensed = if crate::tools::tool_intent::is_command_session_tool(tool_name) {
            command_preview_content(tool_name, &value, &spool_result.content, max_preview_bytes)
        } else {
            let condensed = condense_content(&spool_result.content);
            max_preview_bytes
                .map(|limit| truncate_byte_budget(&condensed, limit, ""))
                .unwrap_or(condensed)
        };
        let spool_path = spool_result.file_path.to_string_lossy().to_string();

        let mut response = match value {
            Value::Object(map) => Value::Object(map),
            _ => json!({}),
        };
        let is_pty_tool = crate::tools::tool_intent::is_command_session_tool(tool_name);
        let use_output_field =
            is_pty_tool || response.get("output").and_then(|v| v.as_str()).is_some_and(|s| !s.is_empty());
        let source_path = if tool_name == tools::READ_FILE || tool_name == tools::UNIFIED_FILE {
            response.get("path").and_then(|v| v.as_str()).map(String::from)
        } else {
            None
        };
        let stderr_preview = response
            .get("stderr")
            .and_then(|v| v.as_str())
            .map(|s| truncate_byte_budget(s, 500, "... (truncated)"));

        if let Some(obj) = response.as_object_mut() {
            obj.remove("stdout");
            obj.remove("follow_up_prompt");
            obj.remove("spooled_to_file");
            obj.remove("raw_output");
            obj.insert("preview".to_string(), json!(condensed.clone()));

            // Replace only the heavy stream field with condensed preview.
            if use_output_field {
                obj.insert("output".to_string(), json!(condensed));
            } else {
                obj.insert("content".to_string(), json!(condensed));
            }

            obj.insert("spool_path".to_string(), json!(spool_path));
            obj.insert("spooled_bytes".to_string(), json!(spool_result.original_bytes));
            obj.insert("spool_sha256".to_string(), json!(spool_result.sha256));
            obj.insert("spool_state".to_string(), json!("completed"));
            obj.insert("spool_complete".to_string(), json!(true));
            let mut line_counter = SpoolLineCounter::default();
            line_counter.append(spool_result.content.as_bytes());
            obj.insert("spool_line_count".to_string(), json!(line_counter.line_count()));

            if let Some(src) = source_path {
                obj.entry("source_path".to_string()).or_insert_with(|| json!(src));
            }
            if let Some(stderr) = stderr_preview {
                obj.insert("stderr_preview".to_string(), json!(stderr));
            }
        }

        ensure_spooled_reference_metadata(&mut response);
        Ok(response)
    }

    /// Pin spool files referenced by a blocked-turn handoff so age/eviction
    /// cleanup cannot delete them before the user resumes. Pins are bounded:
    /// callers must release them when the blocker resolves.
    pub async fn pin_spool_files(&self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        let mut pinned = self.pinned_files.write().await;
        pinned.extend(paths.iter().cloned());
    }

    /// Release pins for spool files that are no longer needed (e.g. blocker
    /// resolved). Missing entries are ignored.
    pub async fn unpin_spool_files(&self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        let mut pinned = self.pinned_files.write().await;
        for path in paths {
            pinned.remove(path);
        }
    }

    /// Extract workspace-relative spool paths from model-facing values so a
    /// blocked-turn handoff can pin exactly the files the next turn needs.
    pub fn spool_paths_from_value(value: &Value) -> Option<String> {
        SpooledOutputReference::from_value(value).map(|reference| reference.spool_path.to_string())
    }

    /// Clean up old spooled files and sync the in-memory tracking list.
    /// Pinned blocked-turn outputs are never deleted here.
    ///
    /// Also enforces [`SpoolerConfig::max_files`] against the on-disk
    /// directory (not just this instance's tracking vec) so leftovers from
    /// prior sessions cannot accumulate past the budget.
    pub async fn cleanup_old_files(&self) -> Result<usize> {
        self.cleanup_files(SpoolCleanupScope::AgeAndCount).await
    }

    /// Age-based cleanup only, for startup paths.
    ///
    /// The count budget relies on [`Self::pinned_files`], which is per-process:
    /// a second session starting in the same workspace cannot see a concurrent
    /// session's pinned or active spools, so enforcing the count budget at
    /// startup can delete young files the other session still reads from its
    /// conversation history. Age-expired files are safe to drop anywhere: no
    /// live session produced them within `max_age_secs`. The count budget
    /// stays on the in-session periodic path ([`Self::cleanup_old_files`]).
    pub async fn cleanup_expired_files(&self) -> Result<usize> {
        self.cleanup_files(SpoolCleanupScope::AgeOnly).await
    }

    async fn cleanup_files(&self, scope: SpoolCleanupScope) -> Result<usize> {
        if !fs::try_exists(&self.output_dir).await.unwrap_or(false) {
            return Ok(0);
        }

        let now = std::time::SystemTime::now();
        let mut removed = 0;

        // Collect paths to remove (can't modify vec during filesystem iteration)
        let mut paths_to_remove = Vec::new();
        // Age of each surviving candidate so max_files can drop the oldest.
        let mut survivors: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();

        let pinned = self.pinned_files.read().await;
        let mut entries = fs::read_dir(&self.output_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if pinned.contains(&path) {
                continue;
            }
            if let Ok(metadata) = entry.metadata().await
                && let Ok(modified) = metadata.modified()
            {
                let age = now.duration_since(modified).unwrap_or_default();
                if age.as_secs() > self.config.max_age_secs {
                    paths_to_remove.push(path);
                } else {
                    survivors.push((path, modified));
                }
            }
        }
        drop(pinned);

        // Enforce the file-count budget across sessions: drop the oldest
        // unpinned survivors until at most max_files remain on disk.
        if scope.enforces_count_budget() && survivors.len() > self.config.max_files {
            survivors.sort_by_key(|(_, modified)| *modified);
            let overflow = survivors.len() - self.config.max_files;
            for (path, _) in survivors.into_iter().take(overflow) {
                paths_to_remove.push(path);
            }
        }

        // Remove files from disk
        for path in &paths_to_remove {
            if fs::remove_file(path).await.is_ok() {
                removed += 1;
                debug!(path = %path.display(), "Removed old spooled file");
            }
        }

        // Sync in-memory tracking: remove entries pointing to deleted files
        if removed > 0 {
            let mut files = self.spooled_files.write().await;
            files.retain(|p| !paths_to_remove.contains(p));
        }

        if removed > 0 {
            info!(count = removed, "Cleaned up old spooled tool output files");
        }

        Ok(removed)
    }

    /// Get the output directory path
    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    /// Get current configuration
    pub fn config(&self) -> &SpoolerConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: SpoolerConfig) {
        self.config = config;
    }

    /// List currently spooled files
    pub async fn list_spooled_files(&self) -> Vec<PathBuf> {
        self.spooled_files.read().await.clone()
    }
}

/// Sanitize tool name for use in filename
fn sanitize_tool_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// Extension trait for integrating spooler with tool results
pub trait SpoolableOutput {
    /// Check if this output should be spooled
    fn should_spool(&self, threshold_bytes: usize) -> bool;

    /// Get the byte size of this output
    fn byte_size(&self) -> usize;
}

impl SpoolableOutput for Value {
    fn should_spool(&self, threshold_bytes: usize) -> bool {
        self.to_string().len() > threshold_bytes
    }

    fn byte_size(&self) -> usize {
        self.to_string().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_spooler_creation() {
        let temp = tempdir().unwrap();
        let spooler = ToolOutputSpooler::new(temp.path());

        assert!(spooler.config.enabled);
        assert_eq!(spooler.config.threshold_bytes, DEFAULT_SPOOL_THRESHOLD_BYTES);
        assert_eq!(spooler.config.max_age_secs, default_max_age_secs());
    }

    #[tokio::test]
    async fn test_should_spool_small_value() {
        let temp = tempdir().unwrap();
        let spooler = ToolOutputSpooler::new(temp.path());

        let small_value = json!({"result": "ok"});
        assert!(!spooler.should_spool(&small_value));
    }

    #[tokio::test]
    async fn test_should_spool_large_value() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 100, ..Default::default() }; // Low threshold for testing
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let large_content = "x".repeat(200);
        let large_value = json!({"content": large_content});
        assert!(spooler.should_spool(&large_value));
    }

    #[tokio::test]
    async fn test_should_not_spool_when_disabled() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 100, ..Default::default() }; // Low threshold for testing
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let large_content = "x".repeat(200);
        let large_value = json!({"output": large_content, "no_spool": true});
        assert!(!spooler.should_spool(&large_value));
    }

    #[tokio::test]
    async fn test_spool_output() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let content = "Line 1\nLine 2\nLine 3\n".repeat(10);
        let value = json!({"output": content});

        let result = spooler.spool_output("test_tool", &value, false).await.unwrap();

        assert!(result.file_path.to_string_lossy().contains("test_tool"));
        assert!(result.original_bytes > 0);

        // Verify file was created
        let full_path = temp.path().join(&result.file_path);
        assert!(full_path.exists());
    }

    #[tokio::test]
    async fn test_process_output_small() {
        let temp = tempdir().unwrap();
        let spooler = ToolOutputSpooler::new(temp.path());

        let small_value = json!({"result": "ok"});
        let result = spooler.process_output("test", small_value.clone(), false).await.unwrap();

        // Should return original value unchanged
        assert_eq!(result, small_value);
    }

    #[tokio::test]
    async fn test_process_output_large() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let large_value = json!({"content": "x".repeat(200)});
        let result = spooler.process_output("test", large_value, false).await.unwrap();

        assert!(result.get("spool_path").is_some());
        assert!(result.get("spooled_to_file").is_none());
        assert!(result.get("content").is_some());
        assert!(result.get("spool_path").is_some());
        assert!(result.get("file_path").is_none());
        assert!(result.get("truncated").is_none());
        assert!(result.get("omitted_bytes").is_none());
    }

    #[test]
    fn pending_bounded_read_trims_incomplete_utf8_character() {
        let temp = tempdir().unwrap();
        let output_dir = temp.path().join(TOOL_OUTPUT_DIR);
        std::fs::create_dir_all(&output_dir).unwrap();
        let spool_path = output_dir.join("pending.txt");
        std::fs::write(&spool_path, "prefix🙂suffix").unwrap();

        let reference_value = json!({
            "spool_path": ".vtcode/context/tool_outputs/pending.txt",
            "spool_state": "pending",
        });
        let reference = SpooledOutputReference::from_value(&reference_value).unwrap();

        // `🙂` is four bytes. Stop after its first byte and retain the valid
        // prefix instead of making the entire preview unavailable.
        let preview = reference.read_pending_bounded(temp.path(), "prefix".len() + 1).unwrap();
        assert_eq!(preview, "prefix");
    }

    #[tokio::test]
    async fn completed_spool_reference_detects_tampering_and_legacy_metadata() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);
        let expected = "x".repeat(200);
        let result = spooler
            .process_output("integrity", json!({"content": expected}), false)
            .await
            .expect("spool output");
        let reference = SpooledOutputReference::from_value(&result).expect("typed spool reference");
        let persisted = reference.read_verified_completed(temp.path()).expect("verified read");
        assert!(persisted.contains(&expected));

        let path = temp.path().join(reference.spool_path);
        std::fs::write(&path, "y".repeat(persisted.len())).expect("tamper fixture");
        assert!(reference.read_verified_completed(temp.path()).is_err());

        let legacy = json!({
            "spool_path": reference.spool_path,
            "spool_complete": true,
        });
        let legacy_reference = SpooledOutputReference::from_value(&legacy).expect("legacy reference");
        assert!(legacy_reference.read_verified_completed(temp.path()).is_err());
    }

    #[test]
    fn test_sanitize_tool_name() {
        assert_eq!(sanitize_tool_name("read_file"), "read_file");
        assert_eq!(sanitize_tool_name("mcp/fetch"), "mcp_fetch");
        assert_eq!(sanitize_tool_name("tool-name"), "tool_name");
    }

    #[tokio::test]
    async fn test_read_file_spools_raw_content() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let file_content = "fn main() {\n    println!(\"Hello, world!\");\n}\n// More code here...";

        // Simulate a read_file response with content field
        let read_file_response = json!({
            "success": true,
            "content": file_content,
            "path": "test.rs"
        });

        let result = spooler.process_output("read_file", read_file_response, false).await.unwrap();

        // Should include source_path for read_file
        let source_path = result.get("source_path").and_then(|v| v.as_str()).unwrap();
        assert_eq!(source_path, "test.rs");

        let content_field = result.get("content").and_then(|v| v.as_str()).unwrap();
        assert!(content_field.contains("fn main()"));
        assert!(!content_field.contains("\"success\"")); // Should not show JSON structure

        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, file_content);
        assert!(!spooled_content.contains("\"success\"")); // Raw content, not JSON
    }

    #[tokio::test]
    async fn test_run_pty_cmd_spools_raw_output() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let command_output = "   Compiling vtcode-core v0.68.1\n   Checking vtcode-core v0.68.1\n    Finished dev [unoptimized + debuginfo] target(s)";

        // Simulate a run_pty_cmd response with output field
        let pty_response = json!({
            "output": command_output,
            "exit_code": 0,
            "wall_time": 1.234,
            "success": true
        });

        let result = spooler.process_output("run_pty_cmd", pty_response, false).await.unwrap();

        // Should return file reference
        assert!(result.get("spool_path").is_some());
        assert!(result.get("spooled_to_file").is_none());

        // Verify spooled file contains raw output, not JSON wrapper
        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, command_output);
        assert!(!spooled_content.contains("\"output\""));
        assert!(!spooled_content.contains("\"exit_code\""));
    }

    #[tokio::test]
    async fn test_pty_tools_spool_raw_output() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let command_output = "Some command output text\nwith multiple lines\nfor testing";

        let send_input_response = json!({
            "output": command_output,
            "wall_time": 0.123,
            "session_id": "session123"
        });

        let result = spooler
            .process_output("send_pty_input", send_input_response, false)
            .await
            .unwrap();

        assert!(result.get("spool_path").is_some());
        assert!(result.get("spooled_to_file").is_none());
        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, command_output);
        assert!(!spooled_content.contains("\"output\""));

        let read_session_response = json!({
            "output": command_output,
            "wall_time": 0.456
        });

        let result = spooler
            .process_output("read_pty_session", read_session_response, false)
            .await
            .unwrap();

        assert!(result.get("spool_path").is_some());
        assert!(result.get("spooled_to_file").is_none());
        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, command_output);
        assert!(!spooled_content.contains("\"output\""));
    }

    #[tokio::test]
    async fn test_forced_pty_spool_keeps_structured_continuation_metadata() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 999_999, ..Default::default() }; // ensure only force triggers
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let output = "x".repeat(10_000);
        let value = json!({
            "output": output,
            "process_id": "run-abc123",
            "next_continue_args": {
                "session_id": "run-abc123"
            },
            "truncated": true
        });

        let result = spooler
            .process_output_with_force("run_pty_cmd", value, false, true)
            .await
            .unwrap();

        assert_eq!(result.get("process_id").and_then(|v| v.as_str()), Some("run-abc123"));
        assert_eq!(
            result.get("next_continue_args"),
            Some(&json!({
                "session_id": "run-abc123"
            }))
        );
        assert!(result.get("follow_up_prompt").is_none());
        assert_eq!(result.get("truncated").and_then(|v| v.as_bool()), Some(true));
        assert!(result.get("spooled_to_file").is_none());
        assert!(
            result
                .get("output")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("bytes omitted"))
        );
        assert!(result.get("spool_hint").is_none());
    }

    #[test]
    fn spool_line_counts_handle_empty_chunks_and_unterminated_lines() {
        for (chunks, expected) in [
            (vec![""], 0),
            (vec!["abc", "", "def"], 1),
            (vec!["a\n", "\n", ""], 2),
            (vec!["first\r", "\nsecond\n", "last"], 3),
            (vec!["日本語", "\n", "final"], 2),
        ] {
            let mut counter = SpoolLineCounter::default();
            for chunk in chunks {
                counter.append(chunk.as_bytes());
            }
            assert_eq!(counter.line_count(), expected);
        }
    }

    #[tokio::test]
    async fn test_exec_command_spools_raw_output() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let command_output = "   Compiling vtcode-core v0.68.1\n   Checking vtcode-core v0.68.1\n    Finished dev";

        let exec_command_response = json!({
            "output": command_output,
            "exit_code": 0,
            "wall_time": 1.234,
            "success": true
        });

        let result = spooler
            .process_output(tools::EXEC_COMMAND, exec_command_response, false)
            .await
            .unwrap();

        assert!(result.get("spool_path").is_some());
        assert!(result.get("spooled_to_file").is_none());

        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, command_output);
        assert_eq!(result["spool_line_count"], 3);
        assert!(result["spool_note"].as_str().unwrap().contains("EOF at line 3"));
        assert!(!spooled_content.contains("\"output\""));
        assert!(!spooled_content.contains("\"exit_code\""));
    }

    #[tokio::test]
    async fn test_exec_command_spools_internal_raw_output_over_preview() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 10, ..Default::default() };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let raw_output = "line 1\nline 2\nline 3\nline 4\nline 5\nline 6";
        let preview_output = "line 1\nline 2\n[Output truncated]";
        let exec_command_response = json!({
            "output": preview_output,
            "raw_output": raw_output,
            "truncated": true,
            "exit_code": 0,
            "wall_time": 1.234,
            "success": true
        });

        let result = spooler
            .process_output(tools::EXEC_COMMAND, exec_command_response, false)
            .await
            .unwrap();

        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, raw_output);
        assert_ne!(spooled_content, preview_output);
        let spool_note = result.get("spool_note").and_then(Value::as_str).unwrap();
        assert!(spool_note.contains("exec_command"));
        assert!(!spool_note.contains("unified_file"));
        assert!(!spool_note.contains("unified_search"));
        assert!(!spool_note.contains("unified_exec"));
    }

    #[tokio::test]
    async fn test_double_serialized_pty_output() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let command_output = "   Compiling vtcode-core v0.68.1\n   Checking vtcode-core v0.68.1\n    Finished dev";

        let inner_json = json!({
            "output": command_output,
            "exit_code": 0,
            "wall_time": 1.234,
            "success": true
        });
        let double_serialized = json!(serde_json::to_string(&inner_json).unwrap());

        let result = spooler.process_output("run_pty_cmd", double_serialized, false).await.unwrap();

        assert!(result.get("spool_path").is_some());
        assert!(result.get("spooled_to_file").is_none());

        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, command_output);
        assert!(!spooled_content.contains("\"output\""));
    }

    #[tokio::test]
    async fn test_bash_and_shell_spool_raw_output() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig { threshold_bytes: 50, ..Default::default() };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let command_output = "total 32\ndrwxr-xr-x  10 user  staff   320 Jan  1 12:00 .";

        let bash_response = json!({
            "output": command_output,
            "exit_code": 0,
            "wall_time": 0.1
        });

        let result = spooler.process_output("bash", bash_response, false).await.unwrap();

        assert!(result.get("spool_path").is_some());
        assert!(result.get("spooled_to_file").is_none());
        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, command_output);

        let shell_response = json!({
            "output": command_output,
            "exit_code": 0,
            "wall_time": 0.2
        });

        let result = spooler.process_output("shell", shell_response, false).await.unwrap();

        assert!(result.get("spool_path").is_some());
        assert!(result.get("spooled_to_file").is_none());
        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, command_output);
    }

    #[test]
    fn test_condense_content_short() {
        let short = "a".repeat(CONDENSE_HEAD_BYTES + CONDENSE_TAIL_BYTES);
        let result = condense_content(&short);
        assert_eq!(result, short);
    }

    #[test]
    fn test_condense_content_long() {
        let total = 20_000;
        let long_content = "a".repeat(total);
        let result = condense_content(&long_content);
        assert!(result.contains("bytes omitted"));
        assert!(result.len() < total);
        assert!(result.starts_with(&"a".repeat(100)));
        assert!(result.ends_with(&"a".repeat(100)));
    }

    #[test]
    fn test_condense_content_utf8_boundary() {
        let mut content = "a".repeat(CONDENSE_HEAD_BYTES - 1);
        content.push('é'); // 2-byte char at boundary
        content.push_str(&"b".repeat(20_000));
        let result = condense_content(&content);
        assert!(result.contains("bytes omitted"));
        assert!(result.is_char_boundary(0));
    }

    #[test]
    fn test_tail_preview_content_shows_only_tail() {
        let input = (0..200).map(|i| format!("line-{i}")).collect::<Vec<_>>().join("\n");
        let preview = tail_preview_text(&input, 500, 10);
        assert!(preview.contains("omitted"));
        assert!(preview.contains("line-199"));
        assert!(!preview.contains("line-1\n"));
    }

    #[tokio::test]
    async fn test_estimate_size_content_field() {
        let temp = tempdir().unwrap();
        let spooler = ToolOutputSpooler::new(temp.path());

        let val = json!({"content": "hello world"});
        assert_eq!(spooler.estimate_size(&val), 11);
    }

    #[tokio::test]
    async fn test_estimate_size_output_field() {
        let temp = tempdir().unwrap();
        let spooler = ToolOutputSpooler::new(temp.path());

        let val = json!({"output": "some output"});
        assert_eq!(spooler.estimate_size(&val), 11);
    }

    #[tokio::test]
    async fn test_estimate_size_string_value() {
        let temp = tempdir().unwrap();
        let spooler = ToolOutputSpooler::new(temp.path());

        let val = json!("raw string");
        assert_eq!(spooler.estimate_size(&val), 10);
    }

    #[tokio::test]
    async fn test_estimate_size_fallback() {
        let temp = tempdir().unwrap();
        let spooler = ToolOutputSpooler::new(temp.path());

        let val = json!({"some_key": 42});
        assert!(spooler.estimate_size(&val) > 0);
    }

    #[tokio::test]
    async fn test_cleanup_old_files_respects_configured_max_age() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig {
            threshold_bytes: 1,
            max_age_secs: 0,
            ..Default::default()
        };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);
        let value = json!({"output": "old output"});

        let result = spooler.spool_output("test_tool", &value, false).await.unwrap();
        let full_path = temp.path().join(&result.file_path);
        assert!(full_path.exists());

        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

        let removed = spooler.cleanup_old_files().await.unwrap();
        assert_eq!(removed, 1);
        assert!(!full_path.exists());
    }

    #[tokio::test]
    async fn test_cleanup_enforces_max_files_across_sessions() {
        // Simulate leftovers from prior sessions: files exist on disk but are
        // not in this instance's tracking vec. max_files must still bound them.
        let temp = tempdir().unwrap();
        let output_dir = temp.path().join(TOOL_OUTPUT_DIR);
        std::fs::create_dir_all(&output_dir).unwrap();
        for index in 0..8 {
            std::fs::write(output_dir.join(format!("stale_{index}.txt")), b"leftover").unwrap();
            // Ensure distinct mtimes so oldest-first eviction is deterministic.
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        let config = SpoolerConfig {
            threshold_bytes: 1,
            // Keep everything young so only the count budget applies.
            max_age_secs: 3600,
            max_files: 3,
            ..Default::default()
        };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);
        let removed = spooler.cleanup_old_files().await.unwrap();
        assert_eq!(removed, 5, "startup prune must drop overflow past max_files");

        let remaining = std::fs::read_dir(&output_dir).unwrap().count();
        assert_eq!(remaining, 3, "on-disk spool count must respect max_files");
    }

    #[tokio::test]
    async fn test_cleanup_skips_pinned_blocked_turn_outputs() {
        let temp = tempdir().unwrap();
        let config = SpoolerConfig {
            threshold_bytes: 1,
            max_age_secs: 0,
            ..Default::default()
        };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);
        let value = json!({"output": "blocked output"});

        let result = spooler.spool_output("exec_command", &value, false).await.unwrap();
        let full_path = temp.path().join(&result.file_path);
        assert!(full_path.exists());

        spooler.pin_spool_files(std::slice::from_ref(&full_path)).await;
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

        let removed = spooler.cleanup_old_files().await.unwrap();
        assert_eq!(removed, 0);
        assert!(full_path.exists());

        spooler.unpin_spool_files(std::slice::from_ref(&full_path)).await;
        let removed = spooler.cleanup_old_files().await.unwrap();
        assert_eq!(removed, 1);
        assert!(!full_path.exists());
    }

    #[tokio::test]
    async fn test_cleanup_expired_files_never_enforces_count_budget() {
        // Startup pruning runs in a fresh process whose pin set cannot see a
        // concurrent session's young spools, so it must be age-only: young
        // files beyond max_files stay on disk until the in-session periodic
        // cleanup enforces the budget.
        let temp = tempdir().unwrap();
        let output_dir = temp.path().join(".vtcode").join("context").join("tool_outputs");
        std::fs::create_dir_all(&output_dir).unwrap();
        for i in 0..8 {
            std::fs::write(output_dir.join(format!("young-{i}.out")), b"young").unwrap();
        }

        let config = SpoolerConfig {
            threshold_bytes: 1,
            max_age_secs: 3600,
            max_files: 3,
            ..Default::default()
        };
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let removed = spooler.cleanup_expired_files().await.unwrap();
        assert_eq!(removed, 0, "startup prune must not evict young files past max_files");
        assert_eq!(std::fs::read_dir(&output_dir).unwrap().count(), 8, "all young spools survive the startup prune");

        let removed = spooler.cleanup_old_files().await.unwrap();
        assert_eq!(removed, 5, "in-session cleanup still enforces the count budget");
        assert_eq!(std::fs::read_dir(&output_dir).unwrap().count(), 3);
    }
}
