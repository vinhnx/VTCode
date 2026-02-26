//! Tool Output Spooler for Dynamic Context Discovery
//!
//! Implements Cursor-style dynamic context discovery by writing large tool outputs
//! to files instead of truncating them. This allows agents to retrieve the full
//! output via `read_file` or search it with `grep_file` when needed.
//!
//! ## Design Philosophy
//!
//! Instead of truncating large tool responses (which loses data), we:
//! 1. Write the full output to `.vtcode/context/tool_outputs/{tool}_{timestamp}.txt`
//! 2. Return a file reference to the agent
//! 3. Agent can use `read_file` with offset/limit or `grep_file` to explore
//!
//! This is more token-efficient as only necessary data is pulled into context.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Default threshold for spooling tool output to files (200KB)
/// Matches the byte fuse in output_processing.rs and avoids unnecessary spooling
/// for modern context windows.
pub const DEFAULT_SPOOL_THRESHOLD_BYTES: usize = 200_000;

/// Maximum age for spooled files before cleanup (1 hour)
const MAX_SPOOL_AGE_SECS: u64 = 3600;

const CONDENSE_HEAD_BYTES: usize = 8_000;
const CONDENSE_TAIL_BYTES: usize = 4_000;
const PTY_PREVIEW_TAIL_BYTES: usize = 2_500;
const PTY_PREVIEW_MAX_LINES: usize = 40;

fn is_pty_tool_name(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "run_pty_cmd"
            | "send_pty_input"
            | "read_pty_session"
            | "unified_exec"
            | "bash"
            | "shell"
            | "execute_code"
    )
}

fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn condense_content_with_limits(content: &str, head_bytes: usize, tail_bytes: usize) -> String {
    let byte_len = content.len();
    let max_inline = head_bytes + tail_bytes;
    if byte_len <= max_inline {
        return content.to_string();
    }

    let head_end = floor_char_boundary(content, head_bytes);
    let tail_start_raw = byte_len.saturating_sub(tail_bytes);
    let tail_start = ceil_char_boundary(content, tail_start_raw);

    let omitted = byte_len
        .saturating_sub(head_end)
        .saturating_sub(byte_len - tail_start);

    format!(
        "{}\n\n… [{} bytes omitted — full content in spool file] …\n\n{}",
        &content[..head_end],
        omitted,
        &content[tail_start..]
    )
}

fn condense_content(content: &str) -> String {
    condense_content_with_limits(content, CONDENSE_HEAD_BYTES, CONDENSE_TAIL_BYTES)
}

fn tail_preview_content(content: &str, tail_bytes: usize, max_lines: usize) -> String {
    if content.is_empty() {
        return String::new();
    }

    let tail_start = ceil_char_boundary(content, content.len().saturating_sub(tail_bytes));
    let tail_slice = &content[tail_start..];

    let mut line_start = 0usize;
    if max_lines > 0 {
        let mut seen = 0usize;
        for (idx, b) in tail_slice.as_bytes().iter().enumerate().rev() {
            if *b == b'\n' {
                seen += 1;
                if seen >= max_lines {
                    line_start = idx.saturating_add(1);
                    break;
                }
            }
        }
    }

    let preview = &tail_slice[line_start..];
    let omitted = tail_start.saturating_add(line_start);
    if omitted == 0 {
        return preview.to_string();
    }

    format!(
        "… [{} bytes omitted — showing tail preview] …\n{}",
        omitted, preview
    )
}

fn build_spool_hint(tool_name: &str, spool_path: &str) -> String {
    if is_pty_tool_name(tool_name) {
        format!(
            "Large command output was spooled to \"{}\". Read full output with read_file path=\"{}\" (or keep polling with read_pty_session while the process is running).",
            spool_path, spool_path
        )
    } else {
        format!(
            "Large output was spooled to \"{}\". Use read_file/grep_file to inspect details.",
            spool_path
        )
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

fn default_include_reference() -> bool {
    true
}

impl Default for SpoolerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_bytes: DEFAULT_SPOOL_THRESHOLD_BYTES,
            max_files: 100,
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
    /// Full content written to the spool file
    pub content: String,
}

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
}

impl ToolOutputSpooler {
    /// Create a new spooler for the given workspace
    pub fn new(workspace_root: &Path) -> Self {
        Self::with_config(workspace_root, SpoolerConfig::default())
    }

    /// Create a new spooler with custom configuration
    pub fn with_config(workspace_root: &Path, config: SpoolerConfig) -> Self {
        let output_dir = workspace_root
            .join(".vtcode")
            .join("context")
            .join("tool_outputs");

        Self {
            workspace_root: workspace_root.to_path_buf(),
            output_dir,
            config,
            spooled_files: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Check if a value should be spooled based on size
    pub fn should_spool(&self, value: &Value) -> bool {
        if !self.config.enabled {
            return false;
        }
        if value
            .get("no_spool")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return false;
        }
        self.estimate_size(value) > self.config.threshold_bytes
    }

    fn estimate_size(&self, value: &Value) -> usize {
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
    pub async fn spool_output(
        &self,
        tool_name: &str,
        value: &Value,
        is_mcp: bool,
    ) -> Result<SpoolResult> {
        // Ensure output directory exists
        fs::create_dir_all(&self.output_dir)
            .await
            .with_context(|| {
                format!(
                    "Failed to create tool output directory: {}",
                    self.output_dir.display()
                )
            })?;

        // Generate unique filename
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros();
        let filename = format!("{}_{}.txt", sanitize_tool_name(tool_name), timestamp);
        let file_path = self.output_dir.join(&filename);

        // For read_file/unified_file and PTY-related tools, extract raw content so the spooled file is directly usable
        // This allows grep_file to work on the spooled output and makes reading more intuitive
        let content = if (tool_name == "read_file" || tool_name == "unified_file") && !is_mcp {
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
                serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
            }
        } else if (tool_name == "run_pty_cmd"
            || tool_name == "send_pty_input"
            || tool_name == "read_pty_session"
            || tool_name == "unified_exec"
            || tool_name == "bash"
            || tool_name == "shell")
            && !is_mcp
        {
            // For PTY-related tools (including unified_exec which delegates to run_pty_cmd),
            // extract the actual command output from the "output" field.
            // This ensures the spooled file contains the raw command output, not the JSON wrapper.
            //
            // Handle two cases:
            // 1. value is an object with "output" field (normal case)
            // 2. value is a string containing JSON (edge case: double-serialized)
            if let Some(output_content) = value.get("output").and_then(|v| v.as_str()) {
                output_content.to_string()
            } else if let Some(json_str) = value.as_str() {
                // Edge case: value might be a JSON string that needs parsing
                // This can happen if the value was serialized somewhere in the pipeline
                if let Ok(parsed) = serde_json::from_str::<Value>(json_str) {
                    if let Some(output_content) = parsed.get("output").and_then(|v| v.as_str()) {
                        debug!(
                            tool = tool_name,
                            "PTY spool: recovered output from double-serialized JSON string"
                        );
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
                serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
            }
        } else if let Some(s) = value.as_str() {
            s.to_string()
        } else {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        };

        // Sanitize content to redact any secrets before writing to disk
        let sanitized_content = vtcode_commons::sanitizer::redact_secrets(content);
        let original_bytes = sanitized_content.len();

        fs::write(&file_path, &sanitized_content)
            .await
            .with_context(|| format!("Failed to write tool output to: {}", file_path.display()))?;

        {
            let mut files = self.spooled_files.write().await;
            files.push(file_path.clone());

            if files.len() > self.config.max_files {
                let old_file = files.remove(0);
                let _ = fs::remove_file(&old_file).await;
            }
        }

        let relative_path = file_path
            .strip_prefix(&self.workspace_root)
            .unwrap_or(&file_path)
            .to_path_buf();

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
            content: sanitized_content,
        })
    }

    /// Process a tool output, spooling if necessary.
    ///
    /// Returns the original value if below threshold, or a condensed
    /// head+tail payload with a `spool_path` reference if spooled.
    pub async fn process_output(
        &self,
        tool_name: &str,
        value: Value,
        is_mcp: bool,
    ) -> Result<Value> {
        self.process_output_with_force(tool_name, value, is_mcp, false)
            .await
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
        let no_spool = value
            .get("no_spool")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if no_spool {
            return Ok(value);
        }
        if !force_spool && !self.should_spool(&value) {
            return Ok(value);
        }

        let spool_result = self.spool_output(tool_name, &value, is_mcp).await?;
        let condensed = if is_pty_tool_name(tool_name) {
            tail_preview_content(
                &spool_result.content,
                PTY_PREVIEW_TAIL_BYTES,
                PTY_PREVIEW_MAX_LINES,
            )
        } else {
            condense_content(&spool_result.content)
        };
        let spool_path = spool_result.file_path.to_string_lossy().to_string();
        let spool_hint = build_spool_hint(tool_name, &spool_path);

        let mut response = match value {
            Value::Object(map) => Value::Object(map),
            _ => json!({}),
        };
        let is_pty_tool = is_pty_tool_name(tool_name);
        let use_output_field = is_pty_tool
            || response
                .get("output")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty());
        let source_path = if tool_name == "read_file" || tool_name == "unified_file" {
            response
                .get("path")
                .and_then(|v| v.as_str())
                .map(String::from)
        } else {
            None
        };
        let stderr_preview = response.get("stderr").and_then(|v| v.as_str()).map(|s| {
            if s.len() > 500 {
                format!("{}... (truncated)", &s[..500])
            } else {
                s.to_string()
            }
        });

        if let Some(obj) = response.as_object_mut() {
            obj.remove("stdout");

            // Replace only the heavy stream field with condensed preview.
            if use_output_field {
                obj.insert("output".to_string(), json!(condensed));
            } else {
                obj.insert("content".to_string(), json!(condensed));
            }

            obj.insert("spooled_to_file".to_string(), json!(true));
            obj.insert("spool_path".to_string(), json!(spool_path));
            obj.insert(
                "spooled_bytes".to_string(),
                json!(spool_result.original_bytes),
            );
            obj.insert("spool_hint".to_string(), json!(spool_hint));

            if let Some(src) = source_path
                && !obj.contains_key("source_path")
            {
                obj.insert("source_path".to_string(), json!(src));
            }
            if let Some(stderr) = stderr_preview {
                obj.insert("stderr_preview".to_string(), json!(stderr));
            }
        }

        Ok(response)
    }

    /// Clean up old spooled files
    pub async fn cleanup_old_files(&self) -> Result<usize> {
        if !self.output_dir.exists() {
            return Ok(0);
        }

        let now = std::time::SystemTime::now();
        let mut removed = 0;

        let mut entries = fs::read_dir(&self.output_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if let Ok(metadata) = entry.metadata().await
                && let Ok(modified) = metadata.modified()
                && let Ok(age) = now.duration_since(modified)
                && age.as_secs() > MAX_SPOOL_AGE_SECS
                && fs::remove_file(&path).await.is_ok()
            {
                removed += 1;
                debug!(path = %path.display(), "Removed old spooled file");
            }
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
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
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
        assert_eq!(
            spooler.config.threshold_bytes,
            DEFAULT_SPOOL_THRESHOLD_BYTES
        );
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
        let mut config = SpoolerConfig::default();
        config.threshold_bytes = 100; // Low threshold for testing
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let large_content = "x".repeat(200);
        let large_value = json!({"content": large_content});
        assert!(spooler.should_spool(&large_value));
    }

    #[tokio::test]
    async fn test_should_not_spool_when_disabled() {
        let temp = tempdir().unwrap();
        let mut config = SpoolerConfig::default();
        config.threshold_bytes = 100; // Low threshold for testing
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let large_content = "x".repeat(200);
        let large_value = json!({"output": large_content, "no_spool": true});
        assert!(!spooler.should_spool(&large_value));
    }

    #[tokio::test]
    async fn test_spool_output() {
        let temp = tempdir().unwrap();
        let mut config = SpoolerConfig::default();
        config.threshold_bytes = 50;
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let content = "Line 1\nLine 2\nLine 3\n".repeat(10);
        let value = json!({"output": content});

        let result = spooler
            .spool_output("test_tool", &value, false)
            .await
            .unwrap();

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
        let result = spooler
            .process_output("test", small_value.clone(), false)
            .await
            .unwrap();

        // Should return original value unchanged
        assert_eq!(result, small_value);
    }

    #[tokio::test]
    async fn test_process_output_large() {
        let temp = tempdir().unwrap();
        let mut config = SpoolerConfig::default();
        config.threshold_bytes = 50;
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let large_value = json!({"content": "x".repeat(200)});
        let result = spooler
            .process_output("test", large_value, false)
            .await
            .unwrap();

        assert!(result.get("spooled_to_file").is_some());
        assert!(result.get("content").is_some());
        assert!(result.get("spool_path").is_some());
        assert!(result.get("file_path").is_none());
        assert!(result.get("truncated").is_none());
        assert!(result.get("omitted_bytes").is_none());
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
        let mut config = SpoolerConfig::default();
        config.threshold_bytes = 50;
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let file_content = "fn main() {\n    println!(\"Hello, world!\");\n}\n// More code here...";

        // Simulate a read_file response with content field
        let read_file_response = json!({
            "success": true,
            "content": file_content,
            "path": "test.rs"
        });

        let result = spooler
            .process_output("read_file", read_file_response, false)
            .await
            .unwrap();

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
        let mut config = SpoolerConfig::default();
        config.threshold_bytes = 50;
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let command_output = "   Compiling vtcode-core v0.68.1\n   Checking vtcode-core v0.68.1\n    Finished dev [unoptimized + debuginfo] target(s)";

        // Simulate a run_pty_cmd response with output field
        let pty_response = json!({
            "output": command_output,
            "exit_code": 0,
            "wall_time": 1.234,
            "success": true
        });

        let result = spooler
            .process_output("run_pty_cmd", pty_response, false)
            .await
            .unwrap();

        // Should return file reference
        assert!(result.get("spooled_to_file").is_some());

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
        let mut config = SpoolerConfig::default();
        config.threshold_bytes = 50;
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

        assert!(result.get("spooled_to_file").is_some());
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

        assert!(result.get("spooled_to_file").is_some());
        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, command_output);
        assert!(!spooled_content.contains("\"output\""));
    }

    #[tokio::test]
    async fn test_forced_pty_spool_preserves_follow_up_metadata() {
        let temp = tempdir().unwrap();
        let mut config = SpoolerConfig::default();
        config.threshold_bytes = 999_999; // ensure only force triggers
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let output = "x".repeat(10_000);
        let value = json!({
            "output": output,
            "process_id": "run-abc123",
            "follow_up_prompt": "Read more with read_pty_session session_id=\"run-abc123\".",
            "truncated": true
        });

        let result = spooler
            .process_output_with_force("run_pty_cmd", value, false, true)
            .await
            .unwrap();

        assert_eq!(
            result.get("process_id").and_then(|v| v.as_str()),
            Some("run-abc123")
        );
        assert!(
            result
                .get("follow_up_prompt")
                .and_then(|v| v.as_str())
                .is_some()
        );
        assert_eq!(
            result.get("truncated").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            result.get("spooled_to_file").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(
            result
                .get("output")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("tail preview"))
        );
        assert!(
            result
                .get("spool_hint")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("read_file"))
        );
    }

    #[tokio::test]
    async fn test_unified_exec_spools_raw_output() {
        let temp = tempdir().unwrap();
        let mut config = SpoolerConfig::default();
        config.threshold_bytes = 50;
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let command_output =
            "   Compiling vtcode-core v0.68.1\n   Checking vtcode-core v0.68.1\n    Finished dev";

        let unified_exec_response = json!({
            "output": command_output,
            "exit_code": 0,
            "wall_time": 1.234,
            "success": true
        });

        let result = spooler
            .process_output("unified_exec", unified_exec_response, false)
            .await
            .unwrap();

        assert!(result.get("spooled_to_file").is_some());

        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, command_output);
        assert!(!spooled_content.contains("\"output\""));
        assert!(!spooled_content.contains("\"exit_code\""));
    }

    #[tokio::test]
    async fn test_double_serialized_pty_output() {
        let temp = tempdir().unwrap();
        let mut config = SpoolerConfig::default();
        config.threshold_bytes = 50;
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let command_output =
            "   Compiling vtcode-core v0.68.1\n   Checking vtcode-core v0.68.1\n    Finished dev";

        let inner_json = json!({
            "output": command_output,
            "exit_code": 0,
            "wall_time": 1.234,
            "success": true
        });
        let double_serialized = json!(serde_json::to_string(&inner_json).unwrap());

        let result = spooler
            .process_output("run_pty_cmd", double_serialized, false)
            .await
            .unwrap();

        assert!(result.get("spooled_to_file").is_some());

        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, command_output);
        assert!(!spooled_content.contains("\"output\""));
    }

    #[tokio::test]
    async fn test_bash_and_shell_spool_raw_output() {
        let temp = tempdir().unwrap();
        let mut config = SpoolerConfig::default();
        config.threshold_bytes = 50;
        let spooler = ToolOutputSpooler::with_config(temp.path(), config);

        let command_output = "total 32\ndrwxr-xr-x  10 user  staff   320 Jan  1 12:00 .";

        let bash_response = json!({
            "output": command_output,
            "exit_code": 0,
            "wall_time": 0.1
        });

        let result = spooler
            .process_output("bash", bash_response, false)
            .await
            .unwrap();

        assert!(result.get("spooled_to_file").is_some());
        let spooled_path = result.get("spool_path").and_then(|v| v.as_str()).unwrap();
        let spooled_content = std::fs::read_to_string(temp.path().join(spooled_path)).unwrap();
        assert_eq!(spooled_content, command_output);

        let shell_response = json!({
            "output": command_output,
            "exit_code": 0,
            "wall_time": 0.2
        });

        let result = spooler
            .process_output("shell", shell_response, false)
            .await
            .unwrap();

        assert!(result.get("spooled_to_file").is_some());
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
        let input = (0..200)
            .map(|i| format!("line-{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let preview = tail_preview_content(&input, 500, 10);
        assert!(preview.contains("tail preview"));
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
}
