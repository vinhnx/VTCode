//! Cross-session task history index for few-shot retrieval.
//!
//! Task outcomes from completed sessions are indexed by type, enabling the agent
//! to retrieve similar past tasks for informed decision making and few-shot
//! prompting. The index is persisted as JSON at `.vtcode/history/task_index.json`.
//!
//! Following the "persistent state" pattern (Hitchhiker's Guide to Agentic AI,
//! Section 18.6.4), this provides the "task history" component of cross-session
//! continuity.

use crate::core::agent::task::{Task, TaskOutcome, TaskResults};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A single task execution recorded in the history index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHistoryEntry {
    /// Stable identifier for the task (from `Task::id`).
    pub task_id: String,
    /// Human-readable title of the task.
    pub title: String,
    /// Final outcome of the task.
    pub outcome: TaskOutcome,
    /// Number of turns executed.
    pub turns_executed: usize,
    /// Total runtime in milliseconds.
    pub total_duration_ms: u128,
    /// Natural-language summary of what happened.
    pub summary: String,
    /// Unix timestamp (seconds) when the task completed.
    pub completed_at: u64,
    /// Tool categories used during this task (deduplicated and sorted).
    pub tools_used: Vec<String>,
    /// Error categories if the task failed.
    pub error_categories: Vec<String>,
}

impl From<TaskResults> for TaskHistoryEntry {
    fn from(results: TaskResults) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Extract error categories from the outcome
        let error_categories = match &results.outcome {
            TaskOutcome::Failed { reason, .. } => vec![categorize_error(reason)],
            TaskOutcome::ToolLoopLimitReached { .. } => vec!["tool_loop".to_string()],
            TaskOutcome::LoopDetected => vec!["infinite_loop".to_string()],
            TaskOutcome::TurnLimitReached { .. } => vec!["turn_limit".to_string()],
            TaskOutcome::BudgetLimitReached { .. } => vec!["budget".to_string()],
            TaskOutcome::Cancelled => vec!["cancelled".to_string()],
            _ => Vec::new(),
        };

        Self {
            task_id: String::new(), // Will be set from the Task when available
            title: String::new(),   // Will be set from the Task
            outcome: results.outcome,
            turns_executed: results.turns_executed,
            total_duration_ms: results.total_duration_ms,
            summary: results.summary,
            completed_at: now,
            tools_used: Vec::new(), // Populated externally
            error_categories,
        }
    }
}

/// Simple heuristic to categorize an error string.
fn categorize_error(reason: &str) -> String {
    let lower = reason.to_lowercase();
    if lower.contains("api") || lower.contains("provider") || lower.contains("timeout") {
        "api_error".to_string()
    } else if lower.contains("tool") || lower.contains("exec") || lower.contains("command") {
        "tool_error".to_string()
    } else if lower.contains("file") || lower.contains("read") || lower.contains("write") {
        "file_error".to_string()
    } else if lower.contains("permission") || lower.contains("denied") {
        "permission".to_string()
    } else if lower.contains("parse") || lower.contains("invalid") || lower.contains("validation") {
        "validation_error".to_string()
    } else {
        "unknown".to_string()
    }
}

/// An indexed collection of completed task executions, used for few-shot
/// retrieval and cross-session learning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHistoryIndex {
    /// Entries, newest first.
    pub entries: Vec<TaskHistoryEntry>,
    /// Maximum number of entries to retain (LRU eviction — oldest removed first).
    pub max_entries: usize,
}

impl TaskHistoryIndex {
    /// Create a new empty index with the given capacity.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::with_capacity(max_entries.min(16)),
            max_entries,
        }
    }

    /// Append a new entry, optionally associating it with a `Task` for metadata.
    pub fn push(&mut self, mut entry: TaskHistoryEntry, task: Option<&Task>) {
        // Fill in task metadata if available
        if let Some(task) = task {
            entry.task_id = task.id.clone();
            entry.title = task.title.clone();
        }

        self.entries.insert(0, entry);

        // Enforce capacity — remove oldest if over limit
        if self.entries.len() > self.max_entries {
            self.entries.truncate(self.max_entries);
        }
    }

    /// Retrieve entries that match a task by keyword overlap.
    ///
    /// Searches `title + summary` of each entry against the current task's
    /// `title + description`. Returns up to `max_results` matches sorted by
    /// relevance (keyword match count descending).
    pub fn find_similar(&self, task: &Task, max_results: usize) -> Vec<&TaskHistoryEntry> {
        if self.entries.is_empty() || max_results == 0 {
            return Vec::new();
        }

        let query_keywords: Vec<String> = tokenize(&format!("{} {}", task.title, task.description));
        if query_keywords.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<(usize, &TaskHistoryEntry)> = self
            .entries
            .iter()
            .map(|entry| {
                let target = format!("{} {}", entry.title, entry.summary);
                let target_keywords = tokenize(&target);
                let overlap = query_keywords
                    .iter()
                    .filter(|kw| target_keywords.iter().any(|tk| tk == *kw))
                    .count();
                (overlap, entry)
            })
            .filter(|(score, _)| *score > 0)
            .collect();

        // Sort by relevance descending
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.truncate(max_results);
        scored.into_iter().map(|(_, entry)| entry).collect()
    }

    /// Load the index from a JSON file.
    pub fn load(path: &Path) -> std::io::Result<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let data = std::fs::read_to_string(path)?;
        let index: TaskHistoryIndex = serde_json::from_str(&data)?;
        Ok(Some(index))
    }

    /// Save the index to a JSON file using atomic write.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let serialized = serde_json::to_string_pretty(self)?;
        // Atomic write via temp file + rename
        let temp_path = path.with_file_name(format!(
            ".{}.tmp",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("task_index")
        ));
        std::fs::write(&temp_path, &serialized)?;
        std::fs::rename(&temp_path, path)?;
        Ok(())
    }

    /// Return the default path for the task index file in a workspace.
    pub fn default_path(workspace_root: &Path) -> PathBuf {
        workspace_root.join(".vtcode").join("history").join("task_index.json")
    }

    /// Return the count of entries in the index.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Simple tokenizer that splits on whitespace and lowercases.
fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter(|s| s.len() >= 3) // skip very short tokens
        .map(|s| s.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    fn sample_results() -> TaskResults {
        TaskResults {
            created_contexts: Vec::new(),
            modified_files: Vec::new(),
            executed_commands: Vec::new(),
            summary: "Refactored the authentication module".to_string(),
            stop_reason: None,
            total_cost_usd: None,
            warnings: Vec::new(),
            thread_events: Vec::new(),
            outcome: TaskOutcome::Success,
            turns_executed: 12,
            total_duration_ms: 45000,
            average_turn_duration_ms: None,
            max_turn_duration_ms: None,
            turn_durations_ms: Vec::new(),
        }
    }

    #[test]
    fn test_push_and_len() {
        let mut index = TaskHistoryIndex::new(100);
        assert!(index.is_empty());
        index.push(TaskHistoryEntry::from(sample_results()), None);
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn test_max_entries_enforced() {
        let mut index = TaskHistoryIndex::new(3);
        for i in 0..5 {
            let mut entry = TaskHistoryEntry::from(sample_results());
            entry.title = format!("Task {i}");
            index.push(entry, None);
        }
        assert_eq!(index.len(), 3);
    }

    #[test]
    fn test_find_similar_by_keyword() {
        let mut index = TaskHistoryIndex::new(100);

        let mut entry1 = TaskHistoryEntry::from(sample_results());
        entry1.title = "Auth refactor".to_string();
        entry1.summary = "Refactored the authentication module".to_string();
        index.push(entry1, None);

        let mut entry2 = TaskHistoryEntry::from(sample_results());
        entry2.title = "Database migration".to_string();
        entry2.summary = "Migrated from SQLite to PostgreSQL".to_string();
        index.push(entry2, None);

        let task = Task::new(
            "test".to_string(),
            "Refactor auth".to_string(),
            "Improve authentication".to_string(),
        );

        let similar = index.find_similar(&task, 5);
        // The "auth" and "refactor" keywords should match entry1
        let titles: Vec<&str> = similar.iter().map(|e| e.title.as_str()).collect();
        assert!(titles.contains(&"Auth refactor"));
    }

    #[test]
    fn test_serde_roundtrip() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("task_index.json");

        let mut index = TaskHistoryIndex::new(100);
        index.push(TaskHistoryEntry::from(sample_results()), None);
        index.save(&path).expect("save");

        let loaded = TaskHistoryIndex::load(&path).expect("load").expect("should exist");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.entries[0].summary, "Refactored the authentication module");
    }

    #[test]
    fn test_categorize_error() {
        assert_eq!(categorize_error("API timeout"), "api_error");
        assert_eq!(categorize_error("tool execution failed"), "tool_error");
        assert_eq!(categorize_error("file not found"), "file_error");
        assert_eq!(categorize_error("permission denied"), "permission");
        assert_eq!(categorize_error("parse error at line 42"), "validation_error");
        assert_eq!(categorize_error("something unexpected"), "unknown");
    }
}
