//! Chat History Files for Dynamic Context Discovery
//!
//! Implements Cursor-style chat history persistence during summarization.
//! When context window fills up and summarization occurs, the full conversation
//! is written to `.vtcode/history/` so agents can recover details via `unified_search`.
//!
//! ## Design Philosophy
//!
//! Instead of losing conversation details during lossy summarization:
//! 1. Write full conversation to `.vtcode/history/session_{id}_{turn}.jsonl`
//! 2. Include file reference in summary message
//! 3. Agent can search history with `unified_search` when details are needed

use crate::llm::provider::{ContentPart, Message, MessageContent, MessageRole};
use crate::telemetry::perf::PerfSpan;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use hashbrown::HashMap;
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
            "{}\n\nFull conversation history saved to: {}\nUse `unified_search` (action='grep') or `unified_file` (action='read') to inspect specific details if needed.",
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

fn history_text_from_message_content(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        MessageContent::Parts(parts) => parts
            .iter()
            .map(|part| match part {
                ContentPart::Text { text } => text.clone(),
                ContentPart::Image { .. } => "[Image]".to_string(),
                ContentPart::File {
                    filename,
                    file_id,
                    file_url,
                    ..
                } => filename
                    .clone()
                    .or_else(|| file_id.clone())
                    .or_else(|| file_url.clone())
                    .map(|value| format!("[File: {value}]"))
                    .unwrap_or_else(|| "[File]".to_string()),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// Convert provider-agnostic messages into persisted history messages.
pub fn messages_to_history_messages(
    messages: &[Message],
    start_turn: usize,
) -> Vec<HistoryMessage> {
    let mut history_messages = Vec::with_capacity(messages.len());
    let now = Utc::now();
    let mut tool_names_by_call_id = HashMap::new();

    for (i, message) in messages.iter().enumerate() {
        let turn = start_turn + i;
        let role = message.role.as_generic_str().to_string();

        if let Some(tool_calls) = &message.tool_calls {
            for tool_call in tool_calls {
                if let Some(function) = &tool_call.function {
                    tool_names_by_call_id.insert(tool_call.id.clone(), function.name.clone());
                }
            }
        }

        let content = if message.role == MessageRole::Tool {
            let tool_name = message
                .origin_tool
                .clone()
                .or_else(|| {
                    message
                        .tool_call_id
                        .as_ref()
                        .and_then(|id| tool_names_by_call_id.get(id).cloned())
                })
                .unwrap_or_else(|| "tool".to_string());
            format!(
                "[Tool response from {}: {}]",
                tool_name,
                history_text_from_message_content(&message.content)
            )
        } else {
            let mut text_parts = Vec::new();
            let content_text = history_text_from_message_content(&message.content);
            if !content_text.is_empty() {
                text_parts.push(content_text);
            }

            if let Some(reasoning) = message.reasoning.as_ref()
                && !reasoning.trim().is_empty()
            {
                text_parts.push(format!("[Reasoning: {}]", reasoning.trim()));
            }

            if let Some(tool_calls) = &message.tool_calls {
                for tool_call in tool_calls {
                    if let Some(function) = &tool_call.function {
                        text_parts.push(format!(
                            "[Tool call: {} with args: {}]",
                            function.name, function.arguments
                        ));
                    }
                }
            }

            text_parts.join("\n")
        };

        history_messages.push(HistoryMessage {
            turn,
            role,
            content,
            tool_call_id: message.tool_call_id.clone(),
            tool_name: message
                .tool_call_id
                .as_ref()
                .and_then(|id| tool_names_by_call_id.get(id).cloned()),
            timestamp: now,
        });
    }

    history_messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::{FunctionCall, Message, ToolCall};
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
        assert!(summary.contains("unified_search"));
    }

    #[test]
    fn messages_to_history_messages_preserves_tool_names() {
        let messages = vec![
            Message::assistant_with_tools(
                "Calling tool".to_string(),
                vec![ToolCall {
                    id: "call_1".to_string(),
                    call_type: "function".to_string(),
                    function: Some(FunctionCall {
                        namespace: None,
                        name: "read_file".to_string(),
                        arguments: "{\"path\":\"src/main.rs\"}".to_string(),
                    }),
                    text: None,
                    thought_signature: None,
                }],
            ),
            Message::tool_response("call_1".to_string(), "{\"ok\":true}".to_string()),
        ];

        let history = messages_to_history_messages(&messages, 4);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].turn, 4);
        assert!(history[0].content.contains("read_file"));
        assert_eq!(history[1].tool_name.as_deref(), Some("read_file"));
        assert!(history[1].content.contains("Tool response from read_file"));
    }
}
