//! Task Tracker tool for structured task management during complex sessions.
//!
//! Based on NL2Repo-Bench findings: agents that leverage explicit planning
//! tools achieve significantly better scores. This tool provides a first-class
//! mechanism for the agent to create, update, and query a task checklist
//! persisted to `.vtcode/tasks/`.
//!
//! ## Actions
//!
//! - `create`: Create a new task checklist with a title and list of items
//! - `update`: Mark a specific task item as completed, in_progress, or pending
//! - `list`: Show the current task checklist and its status
//! - `add`: Add a new item to an existing checklist

use std::str::FromStr;

use crate::config::constants::tools;
use crate::tools::handlers::task_tracking::{
    TaskCounts, TaskTrackingStatus, append_notes, parse_marked_status_prefix,
};
use crate::utils::file_utils::{
    ensure_dir_exists, read_file_with_context, write_file_with_context,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::tools::traits::Tool;

pub type TaskStatus = TaskTrackingStatus;

/// A single task item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskItem {
    pub index: usize,
    pub description: String,
    pub status: TaskStatus,
}

/// The full task checklist
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskChecklist {
    pub title: String,
    pub items: Vec<TaskItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl TaskChecklist {
    fn to_markdown(&self) -> String {
        let mut md = format!("# {}\n\n", self.title);
        for item in &self.items {
            md.push_str(&format!(
                "- {} {}\n",
                item.status.flat_checkbox(),
                item.description
            ));
        }
        if let Some(ref notes) = self.notes {
            md.push_str(&format!("\n## Notes\n\n{}\n", notes));
        }
        md
    }

    fn summary(&self) -> Value {
        let mut counts = TaskCounts::default();
        for item in &self.items {
            counts.add(&item.status);
        }

        json!({
            "title": self.title,
            "total": counts.total,
            "completed": counts.completed,
            "in_progress": counts.in_progress,
            "pending": counts.pending,
            "blocked": counts.blocked,
            "progress_percent": counts.progress_percent(),
            "items": self.items.iter().map(|item| {
                json!({
                    "index": item.index,
                    "description": item.description,
                    "status": item.status.to_string()
                })
            }).collect::<Vec<_>>()
        })
    }

    fn view(&self) -> Value {
        let lines = self
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let branch = if idx + 1 == self.items.len() { "└" } else { "├" };
                json!({
                    "display": format!("{} {} {}", branch, item.status.view_symbol(), item.description),
                    "status": item.status.to_string(),
                    "text": item.description,
                    "index_path": item.index.to_string(),
                })
            })
            .collect::<Vec<_>>();

        json!({
            "title": "Updated Plan",
            "lines": lines,
        })
    }
}

/// Arguments for the task_tracker tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTrackerArgs {
    /// Action to perform: create, update, list, add
    pub action: String,

    /// Title for the checklist (required for `create`)
    #[serde(default)]
    pub title: Option<String>,

    /// List of task descriptions (required for `create`)
    #[serde(default)]
    pub items: Option<Vec<String>>,

    /// Index of item to update (required for `update`, 1-indexed)
    #[serde(default)]
    pub index: Option<usize>,

    /// New status for the item (required for `update`)
    #[serde(default)]
    pub status: Option<String>,

    /// Description for a new item (required for `add`)
    #[serde(default)]
    pub description: Option<String>,

    /// Optional notes to append
    #[serde(default)]
    pub notes: Option<String>,
}

/// Task Tracker tool state
pub struct TaskTrackerTool {
    workspace_root: PathBuf,
    checklist: Arc<RwLock<Option<TaskChecklist>>>,
}

impl TaskTrackerTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            checklist: Arc::new(RwLock::new(None)),
        }
    }

    fn tasks_dir(&self) -> PathBuf {
        self.workspace_root.join(".vtcode").join("tasks")
    }

    fn task_file(&self) -> PathBuf {
        self.tasks_dir().join("current_task.md")
    }

    async fn save_checklist(&self, checklist: &TaskChecklist) -> Result<()> {
        let dir = self.tasks_dir();
        ensure_dir_exists(&dir)
            .await
            .with_context(|| format!("Failed to create tasks directory: {}", dir.display()))?;
        let md = checklist.to_markdown();
        write_file_with_context(&self.task_file(), &md, "task checklist")
            .await
            .with_context(|| "Failed to write task checklist")?;
        Ok(())
    }

    async fn load_checklist(&self) -> Result<Option<TaskChecklist>> {
        let file = self.task_file();
        if !file.exists() {
            return Ok(None);
        }
        let content = read_file_with_context(&file, "task checklist").await?;
        // Parse markdown back into checklist
        let mut title = String::new();
        let mut items = Vec::new();
        let mut notes_lines = Vec::new();
        let mut in_notes = false;
        let mut idx = 1;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("# ") && title.is_empty() {
                title = trimmed.strip_prefix("# ").unwrap_or(trimmed).to_string();
                continue;
            }
            if trimmed == "## Notes" {
                in_notes = true;
                continue;
            }
            if in_notes {
                notes_lines.push(line.to_string());
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("- ")
                && let Some((status, description)) = parse_marked_status_prefix(rest)
            {
                items.push(TaskItem {
                    index: idx,
                    description,
                    status,
                });
                idx += 1;
            }
        }

        if title.is_empty() && items.is_empty() {
            return Ok(None);
        }

        let notes = if notes_lines.is_empty() {
            None
        } else {
            Some(notes_lines.join("\n").trim().to_string())
        };

        Ok(Some(TaskChecklist {
            title,
            items,
            notes,
        }))
    }

    async fn handle_create(&self, args: &TaskTrackerArgs) -> Result<Value> {
        let title = args
            .title
            .as_deref()
            .unwrap_or("Task Checklist")
            .to_string();
        let item_descs = args.items.as_deref().unwrap_or(&[]);
        if item_descs.is_empty() {
            anyhow::bail!(
                "At least one item is required for 'create'. Provide items: [\"step 1\", \"step 2\", ...]"
            );
        }

        let items: Vec<TaskItem> = item_descs
            .iter()
            .enumerate()
            .map(|(i, desc)| TaskItem {
                index: i + 1,
                description: desc.clone(),
                status: TaskStatus::Pending,
            })
            .collect();

        let checklist = TaskChecklist {
            title,
            items,
            notes: args.notes.clone(),
        };

        self.save_checklist(&checklist).await?;
        let summary = checklist.summary();
        let view = checklist.view();
        *self.checklist.write().await = Some(checklist);

        Ok(json!({
            "status": "created",
            "message": "Task checklist created successfully.",
            "task_file": self.task_file().display().to_string(),
            "checklist": summary,
            "view": view
        }))
    }

    async fn handle_update(&self, args: &TaskTrackerArgs) -> Result<Value> {
        let mut guard = self.checklist.write().await;
        if guard.is_none() {
            *guard = self.load_checklist().await?;
        }
        let checklist = guard
            .as_mut()
            .context("No active checklist. Use action='create' first.")?;

        let index = args
            .index
            .context("'index' is required for 'update' (1-indexed)")?;
        let status_str = args
            .status
            .as_deref()
            .context("'status' is required for 'update' (pending|in_progress|completed|blocked)")?;

        let new_status = TaskStatus::from_str(status_str)?;

        // Find position first to avoid borrow conflicts
        let item_count = checklist.items.len();
        let pos = checklist
            .items
            .iter()
            .position(|i| i.index == index)
            .with_context(|| {
                format!("No item at index {}. Valid range: 1-{}", index, item_count)
            })?;

        let old_status = checklist.items[pos].status.to_string();
        checklist.items[pos].status = new_status;
        let new_status_str = checklist.items[pos].status.to_string();

        self.save_checklist(checklist).await?;
        let summary = checklist.summary();

        Ok(json!({
            "status": "updated",
            "message": format!("Item {} status changed: {} → {}", index, old_status, new_status_str),
            "checklist": summary,
            "view": checklist.view()
        }))
    }

    async fn handle_list(&self) -> Result<Value> {
        let mut guard = self.checklist.write().await;
        if guard.is_none() {
            *guard = self.load_checklist().await?;
        }

        match guard.as_ref() {
            Some(checklist) => Ok(json!({
                "status": "ok",
                "checklist": checklist.summary(),
                "view": checklist.view()
            })),
            None => Ok(json!({
                "status": "empty",
                "message": "No active checklist. Use action='create' to start one."
            })),
        }
    }

    async fn handle_add(&self, args: &TaskTrackerArgs) -> Result<Value> {
        let mut guard = self.checklist.write().await;
        if guard.is_none() {
            *guard = self.load_checklist().await?;
        }
        let checklist = guard
            .as_mut()
            .context("No active checklist. Use action='create' first.")?;

        let desc = args
            .description
            .as_deref()
            .context("'description' is required for 'add'")?;

        let new_index = checklist.items.len() + 1;
        checklist.items.push(TaskItem {
            index: new_index,
            description: desc.to_string(),
            status: TaskStatus::Pending,
        });

        checklist.notes = append_notes(checklist.notes.take(), args.notes.as_deref());

        self.save_checklist(checklist).await?;
        let summary = checklist.summary();

        Ok(json!({
            "status": "added",
            "message": format!("Added item {}: {}", new_index, desc),
            "checklist": summary,
            "view": checklist.view()
        }))
    }
}

#[async_trait]
impl Tool for TaskTrackerTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let args: TaskTrackerArgs = serde_json::from_value(args)
            .context("Invalid task_tracker arguments. Required: {\"action\": \"create|update|list|add\", ...}")?;

        match args.action.as_str() {
            "create" => self.handle_create(&args).await,
            "update" => self.handle_update(&args).await,
            "list" => self.handle_list().await,
            "add" => self.handle_add(&args).await,
            other => Ok(json!({
                "status": "error",
                "message": format!("Unknown action '{}'. Use: create, update, list, add", other)
            })),
        }
    }

    fn name(&self) -> &'static str {
        tools::TASK_TRACKER
    }

    fn description(&self) -> &'static str {
        "Track task progress with a structured checklist. Use for complex multi-step work to avoid losing track of progress. Actions: create (new checklist), update (change item status), list (show progress), add (append item)."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "update", "list", "add"],
                    "description": "Action to perform on the task checklist."
                },
                "title": {
                    "type": "string",
                    "description": "Title for the checklist (used with 'create')."
                },
                "items": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "List of task descriptions (used with 'create')."
                },
                "index": {
                    "type": "integer",
                    "description": "1-indexed item number to update (used with 'update')."
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "blocked"],
                    "description": "New status for the item (used with 'update')."
                },
                "description": {
                    "type": "string",
                    "description": "Description for a new item (used with 'add')."
                },
                "notes": {
                    "type": "string",
                    "description": "Optional notes to append to the checklist."
                }
            },
            "required": ["action"]
        }))
    }

    fn is_mutating(&self) -> bool {
        false // Writes to .vtcode/tasks/ only, not user code
    }

    fn is_parallel_safe(&self) -> bool {
        false // State management should be sequential
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_checklist() {
        let temp = TempDir::new().unwrap();
        let tool = TaskTrackerTool::new(temp.path().to_path_buf());

        let result = tool
            .execute(json!({
                "action": "create",
                "title": "Refactor Auth",
                "items": ["Extract middleware", "Add tests", "Update docs"]
            }))
            .await
            .unwrap();

        assert_eq!(result["status"], "created");
        assert_eq!(result["checklist"]["total"], 3);
        assert_eq!(result["checklist"]["completed"], 0);
    }

    #[tokio::test]
    async fn test_update_item() {
        let temp = TempDir::new().unwrap();
        let tool = TaskTrackerTool::new(temp.path().to_path_buf());

        tool.execute(json!({
            "action": "create",
            "title": "Test",
            "items": ["Step 1", "Step 2"]
        }))
        .await
        .unwrap();

        let result = tool
            .execute(json!({
                "action": "update",
                "index": 1,
                "status": "completed"
            }))
            .await
            .unwrap();

        assert_eq!(result["status"], "updated");
        assert_eq!(result["checklist"]["completed"], 1);
        assert_eq!(result["checklist"]["progress_percent"], 50);
    }

    #[tokio::test]
    async fn test_add_item() {
        let temp = TempDir::new().unwrap();
        let tool = TaskTrackerTool::new(temp.path().to_path_buf());

        tool.execute(json!({
            "action": "create",
            "title": "Test",
            "items": ["Step 1"]
        }))
        .await
        .unwrap();

        let result = tool
            .execute(json!({
                "action": "add",
                "description": "Step 2"
            }))
            .await
            .unwrap();

        assert_eq!(result["status"], "added");
        assert_eq!(result["checklist"]["total"], 2);
    }

    #[tokio::test]
    async fn test_list_empty() {
        let temp = TempDir::new().unwrap();
        let tool = TaskTrackerTool::new(temp.path().to_path_buf());

        let result = tool.execute(json!({"action": "list"})).await.unwrap();
        assert_eq!(result["status"], "empty");
    }

    #[tokio::test]
    async fn test_persistence_across_loads() {
        let temp = TempDir::new().unwrap();

        // Create with one tool instance
        {
            let tool = TaskTrackerTool::new(temp.path().to_path_buf());
            tool.execute(json!({
                "action": "create",
                "title": "Persist Test",
                "items": ["Alpha", "Beta"]
            }))
            .await
            .unwrap();

            tool.execute(json!({
                "action": "update",
                "index": 1,
                "status": "completed"
            }))
            .await
            .unwrap();
        }

        // Load with fresh tool instance
        let tool2 = TaskTrackerTool::new(temp.path().to_path_buf());
        let result = tool2.execute(json!({"action": "list"})).await.unwrap();

        assert_eq!(result["status"], "ok");
        assert_eq!(result["checklist"]["total"], 2);
        assert_eq!(result["checklist"]["completed"], 1);
    }
}
