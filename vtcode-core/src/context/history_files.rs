//! Chat History Files for Dynamic Context Discovery
//!
//! Implements Cursor-style chat history persistence during summarization.
//! When context window fills up and summarization occurs, the full conversation
//! is written to `.vtcode/history/` so agents can recover details via grep_file.
//!
//! ## Design Philosophy
//!
//! Instead of losing conversation details during lossy summarization:
//! 1. Write full conversation to `.vtcode/history/session_{id}_{turn}.jsonl`
//! 2. Include file reference in summary message
//! 3. Agent can search history with `grep_file` when details are needed

use crate::telemetry::perf::PerfSpan;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tokio::fs as async_fs;
use tracing::{debug, info};

/// Configuration for history file persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryConfig {
    /// Enable history file persistence
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Maximum number of history files to keep per session
    #[serde(default = "default_max_files")]
    pub max_files_per_session: usize,

    /// Include detailed tool results in history
    #[serde(default = "default_include_tool_results")]
    pub include_tool_results: bool,
}

fn default_enabled() -> bool {
    true
}

fn default_max_files() -> usize {
    10
}

fn default_include_tool_results() -> bool {
    true
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_files_per_session: 10,
            include_tool_results: true,
        }
    }
}

/// A single message in the history file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryMessage {
    /// Turn number in the conversation
    pub turn: usize,
    /// Role: user, assistant, tool
    pub role: String,
    /// Message content
    pub content: String,
    /// Optional tool call ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Optional tool name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

/// Metadata about the history file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryMetadata {
    /// Session identifier
    pub session_id: String,
    /// Turn number when history was written
    pub turn_number: usize,
    /// Reason for writing (e.g., "summarization", "checkpoint")
    pub reason: String,
    /// Number of messages in file
    pub message_count: usize,
    /// Files modified in this conversation
    pub modified_files: Vec<String>,
    /// Commands executed
    pub executed_commands: Vec<String>,
    /// Timestamp when written
    pub written_at: DateTime<Utc>,
}

/// Result of writing a history file
#[derive(Debug, Clone)]
pub struct HistoryWriteResult {
    /// Path to the history file (relative to workspace)
    pub file_path: PathBuf,
    /// Metadata about the file
    pub metadata: HistoryMetadata,
}

/// Manager for conversation history files
pub struct HistoryFileManager {
    /// Workspace root
    workspace_root: PathBuf,
    /// History directory
    history_dir: PathBuf,
    /// Session identifier
    session_id: String,
    /// Configuration
    config: HistoryConfig,
    /// Counter for history files in this session
    file_counter: usize,
}

impl HistoryFileManager {
    /// Create a new history file manager
    pub fn new(workspace_root: &Path, session_id: impl Into<String>) -> Self {
        Self::with_config(workspace_root, session_id, HistoryConfig::default())
    }

    /// Create a new history file manager with custom config
    pub fn with_config(
        workspace_root: &Path,
        session_id: impl Into<String>,
        config: HistoryConfig,
    ) -> Self {
        let history_dir = workspace_root.join(".vtcode").join("history");
        Self {
            workspace_root: workspace_root.to_path_buf(),
            history_dir,
            session_id: session_id.into(),
            config,
            file_counter: 0,
        }
    }

    /// Check if history persistence is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Write conversation history to a file (synchronous version)
    ///
    /// Returns the file path and metadata if successful.
    /// Use this when calling from synchronous code paths like summarization.
    pub fn write_history_sync(
        &mut self,
        messages: &[HistoryMessage],
        turn_number: usize,
        reason: &str,
        modified_files: &[String],
        executed_commands: &[String],
    ) -> Result<HistoryWriteResult> {
        let mut perf = PerfSpan::new("vtcode.perf.history_write_ms");
        perf.tag("mode", "sync");
        perf.tag("reason", reason.to_string());

        if !self.config.enabled {
            return Err(anyhow::anyhow!("History persistence is disabled"));
        }

        // Ensure directory exists
        fs::create_dir_all(&self.history_dir).with_context(|| {
            format!(
                "Failed to create history directory: {}",
                self.history_dir.display()
            )
        })?;

        // Generate filename
        self.file_counter += 1;
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
        let filename = format!(
            "{}_{:04}_{}.jsonl",
            sanitize_session_id(&self.session_id),
            turn_number,
            timestamp
        );
        let file_path = self.history_dir.join(&filename);

        // Build metadata
        let metadata = HistoryMetadata {
            session_id: self.session_id.clone(),
            turn_number,
            reason: reason.to_string(),
            message_count: messages.len(),
            modified_files: modified_files.to_vec(),
            executed_commands: executed_commands.to_vec(),
            written_at: Utc::now(),
        };

        // Build file content as JSONL
        let mut content = String::new();

        // Write metadata as first line
        content.push_str(&serde_json::to_string(&serde_json::json!({
            "_type": "metadata",
            "_metadata": metadata
        }))?);
        content.push('\n');

        // Write each message as a line
        for msg in messages {
            content.push_str(&serde_json::to_string(msg)?);
            content.push('\n');
        }

        // Write file
        fs::write(&file_path, &content)
            .with_context(|| format!("Failed to write history file: {}", file_path.display()))?;

        // Calculate relative path
        let relative_path = file_path
            .strip_prefix(&self.workspace_root)
            .unwrap_or(&file_path)
            .to_path_buf();

        info!(
            session = %self.session_id,
            turn = turn_number,
            messages = messages.len(),
            path = %relative_path.display(),
            "Wrote conversation history to file"
        );

        // Cleanup old files synchronously
        self.cleanup_old_files_sync();

        Ok(HistoryWriteResult {
            file_path: relative_path,
            metadata,
        })
    }

    /// Write conversation history to a file (async version)
    ///
    /// Returns the file path and metadata if successful
    pub async fn write_history(
        &mut self,
        messages: &[HistoryMessage],
        turn_number: usize,
        reason: &str,
        modified_files: &[String],
        executed_commands: &[String],
    ) -> Result<HistoryWriteResult> {
        let mut perf = PerfSpan::new("vtcode.perf.history_write_ms");
        perf.tag("mode", "async");
        perf.tag("reason", reason.to_string());

        if !self.config.enabled {
            return Err(anyhow::anyhow!("History persistence is disabled"));
        }

        // Ensure directory exists
        async_fs::create_dir_all(&self.history_dir)
            .await
            .with_context(|| {
                format!(
                    "Failed to create history directory: {}",
                    self.history_dir.display()
                )
            })?;

        // Generate filename
        self.file_counter += 1;
        let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
        let filename = format!(
            "{}_{:04}_{}.jsonl",
            sanitize_session_id(&self.session_id),
            turn_number,
            timestamp
        );
        let file_path = self.history_dir.join(&filename);

        // Build metadata
        let metadata = HistoryMetadata {
            session_id: self.session_id.clone(),
            turn_number,
            reason: reason.to_string(),
            message_count: messages.len(),
            modified_files: modified_files.to_vec(),
            executed_commands: executed_commands.to_vec(),
            written_at: Utc::now(),
        };

        // Build file content as JSONL
        let mut content = String::new();

        // Write metadata as first line
        content.push_str(&serde_json::to_string(&serde_json::json!({
            "_type": "metadata",
            "_metadata": metadata
        }))?);
        content.push('\n');

        // Write each message as a line
        for msg in messages {
            content.push_str(&serde_json::to_string(msg)?);
            content.push('\n');
        }

        // Write file
        async_fs::write(&file_path, &content)
            .await
            .with_context(|| format!("Failed to write history file: {}", file_path.display()))?;

        // Calculate relative path
        let relative_path = file_path
            .strip_prefix(&self.workspace_root)
            .unwrap_or(&file_path)
            .to_path_buf();

        info!(
            session = %self.session_id,
            turn = turn_number,
            messages = messages.len(),
            path = %relative_path.display(),
            "Wrote conversation history to file"
        );

        // Cleanup old files if needed
        self.cleanup_old_files().await?;

        Ok(HistoryWriteResult {
            file_path: relative_path,
            metadata,
        })
    }

    /// Generate a summary message with file reference
    pub fn format_summary_with_reference(&self, base_summary: &str, history_path: &Path) -> String {
        format!(
            "{}\n\nFull conversation history saved to: {}\nUse grep_file to search for specific details if needed.",
            base_summary,
            history_path.display()
        )
    }

    /// Cleanup old history files beyond the limit (synchronous)
    fn cleanup_old_files_sync(&self) {
        if !self.history_dir.exists() {
            return;
        }

        let prefix = sanitize_session_id(&self.session_id);
        let mut files: Vec<PathBuf> = Vec::new();

        if let Ok(entries) = fs::read_dir(&self.history_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && name.starts_with(&prefix)
                    && name.ends_with(".jsonl")
                {
                    files.push(path);
                }
            }
        }

        // Sort by name (which includes timestamp) and remove oldest
        files.sort();
        let excess = files
            .len()
            .saturating_sub(self.config.max_files_per_session);

        for old_file in files.into_iter().take(excess) {
            if fs::remove_file(&old_file).is_ok() {
                debug!(path = %old_file.display(), "Removed old history file");
            }
        }
    }

    /// Cleanup old history files beyond the limit (async)
    async fn cleanup_old_files(&self) -> Result<()> {
        if !self.history_dir.exists() {
            return Ok(());
        }

        let prefix = sanitize_session_id(&self.session_id);
        let mut files: Vec<PathBuf> = Vec::new();

        let mut entries = async_fs::read_dir(&self.history_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && name.starts_with(&prefix)
                && name.ends_with(".jsonl")
            {
                files.push(path);
            }
        }

        // Sort by name (which includes timestamp) and remove oldest
        files.sort();
        let excess = files
            .len()
            .saturating_sub(self.config.max_files_per_session);

        for old_file in files.into_iter().take(excess) {
            if async_fs::remove_file(&old_file).await.is_ok() {
                debug!(path = %old_file.display(), "Removed old history file");
            }
        }

        Ok(())
    }

    /// Get the history directory path
    pub fn history_dir(&self) -> &Path {
        &self.history_dir
    }
}

/// Sanitize session ID for use in filename
fn sanitize_session_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(32)
        .collect()
}

/// Convert conversation content to history messages
pub fn content_to_history_messages(
    conversation: &[crate::gemini::Content],
    start_turn: usize,
) -> Vec<HistoryMessage> {
    let mut messages = Vec::with_capacity(conversation.len());
    let now = Utc::now();

    for (i, content) in conversation.iter().enumerate() {
        let turn = start_turn + i;
        let role = content.role.clone();

        // Extract text content from parts
        let mut text_parts: Vec<String> = Vec::new();
        for part in &content.parts {
            match part {
                crate::gemini::Part::Text { text, .. } => {
                    text_parts.push(text.clone());
                }
                crate::gemini::Part::InlineData { .. } => {
                    text_parts.push("[Image]".to_string());
                }
                crate::gemini::Part::FunctionCall { function_call, .. } => {
                    text_parts.push(format!(
                        "[Tool call: {} with args: {}]",
                        function_call.name, function_call.args
                    ));
                }
                crate::gemini::Part::FunctionResponse {
                    function_response, ..
                } => {
                    text_parts.push(format!(
                        "[Tool response from {}: {}]",
                        function_response.name, function_response.response
                    ));
                }
            }
        }

        messages.push(HistoryMessage {
            turn,
            role,
            content: text_parts.join("\n"),
            tool_call_id: None,
            tool_name: None,
            timestamp: now,
        });
    }

    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_history_manager_creation() {
        let temp = tempdir().unwrap();
        let manager = HistoryFileManager::new(temp.path(), "test_session");

        assert!(manager.is_enabled());
        assert_eq!(manager.session_id, "test_session");
    }

    #[tokio::test]
    async fn test_write_history_async() {
        let temp = tempdir().unwrap();
        let mut manager = HistoryFileManager::new(temp.path(), "test_session");

        let messages = vec![
            HistoryMessage {
                turn: 1,
                role: "user".to_string(),
                content: "Hello".to_string(),
                tool_call_id: None,
                tool_name: None,
                timestamp: Utc::now(),
            },
            HistoryMessage {
                turn: 2,
                role: "assistant".to_string(),
                content: "Hi there".to_string(),
                tool_call_id: None,
                tool_name: None,
                timestamp: Utc::now(),
            },
        ];

        let result = manager
            .write_history(
                &messages,
                5,
                "summarization",
                &["file.rs".to_string()],
                &["cargo build".to_string()],
            )
            .await
            .unwrap();

        assert!(result.file_path.to_string_lossy().contains("test_session"));
        assert_eq!(result.metadata.message_count, 2);
        assert_eq!(result.metadata.turn_number, 5);
    }

    #[test]
    fn test_write_history_sync() {
        let temp = tempdir().unwrap();
        let mut manager = HistoryFileManager::new(temp.path(), "test_session_sync");

        let messages = vec![HistoryMessage {
            turn: 1,
            role: "user".to_string(),
            content: "Hello sync".to_string(),
            tool_call_id: None,
            tool_name: None,
            timestamp: Utc::now(),
        }];

        let result = manager
            .write_history_sync(&messages, 3, "test", &[], &[])
            .unwrap();

        assert!(
            result
                .file_path
                .to_string_lossy()
                .contains("test_session_sync")
        );
        assert_eq!(result.metadata.message_count, 1);
    }

    #[test]
    fn test_sanitize_session_id() {
        assert_eq!(sanitize_session_id("simple"), "simple");
        assert_eq!(sanitize_session_id("with spaces"), "with_spaces");
        assert_eq!(sanitize_session_id("a/b/c"), "a_b_c");
    }

    #[test]
    fn test_format_summary_with_reference() {
        let temp = tempdir().unwrap();
        let manager = HistoryFileManager::new(temp.path(), "test");

        let summary = manager.format_summary_with_reference(
            "Summarized 10 turns.",
            Path::new(".vtcode/history/test.jsonl"),
        );

        assert!(summary.contains("Summarized 10 turns"));
        assert!(summary.contains(".vtcode/history/test.jsonl"));
        assert!(summary.contains("grep_file"));
    }
}
