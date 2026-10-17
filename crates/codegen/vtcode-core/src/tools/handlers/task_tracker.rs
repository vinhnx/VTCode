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

use super::planning_task_tracker::{PlanningTaskTrackerArgs, PlanningTaskTrackerTool};
use super::planning_workflow::{PlanningWorkflowState, plan_file_for_tracker_file, tracker_file_for_plan_file};
use std::str::FromStr;

use crate::config::constants::tools;
use crate::tools::error_helpers::deserialize_tool_args;
use crate::tools::handlers::task_tracking::{
    TASK_ITEMS_DESCRIPTION, TaskCounts, TaskItemInput, TaskMetadataRef, TaskStepMetadata, TaskTrackingStatus,
    TaskTreeNode, TrackerArtifact, append_notes, append_notes_section, append_task_step_metadata, apply_metadata_input,
    apply_metadata_line, compact_task_tree_view, is_bulk_sync_update, metadata_from_input, normalize_optional_text,
    parse_marked_status_prefix, parse_status_prefix, parse_task_items, persist_tracker_artifacts,
    task_files_property_schema, task_items_array_schema, task_status_property_schema, task_verify_property_schema,
    tracker_schema_allof, validate_action_index_fields, validate_update_shape, write_tracker_artifact,
};
use crate::utils::file_utils::{ensure_dir_exists, read_file_with_context};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use vtcode_commons::workspace_relative_display;

use crate::tools::traits::Tool;

pub type TaskStatus = TaskTrackingStatus;

/// A single task item
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskItem {
    pub index: usize,
    pub description: String,
    pub status: TaskStatus,
    #[serde(default, flatten)]
    pub metadata: TaskStepMetadata,
}

/// The full task checklist
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
            let _ = writeln!(md, "- {} {}", item.status.flat_checkbox(), item.description);
            append_task_step_metadata(&mut md, "", &item.metadata);
        }
        append_notes_section(&mut md, self.notes.as_deref());
        md
    }

    fn to_plan_markdown(&self) -> String {
        let mut md = format!("# {}\n\n## Plan of Work\n\n", self.title);
        for item in &self.items {
            let trimmed = item.description.trim_start();
            let indent = &item.description[..item.description.len() - trimmed.len()];
            let _ = writeln!(md, "{}- {} {}", indent, item.status.plan_checkbox(), trimmed);
            append_task_step_metadata(&mut md, indent, &item.metadata);
        }
        append_notes_section(&mut md, self.notes.as_deref());
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
                    "status": item.status.to_string(),
                    "files": item.metadata.files.clone(),
                    "outcome": item.metadata.outcome.clone(),
                    "verify": item.metadata.verify.clone(),
                })
            }).collect::<Vec<_>>()
            ,
            "notes": self.notes.clone(),
        })
    }

    fn view(&self) -> Value {
        let nodes = self
            .items
            .iter()
            .map(|item| TaskTreeNode {
                index_path: item.index.to_string(),
                description: item.description.clone(),
                status: item.status,
                metadata: item.metadata.clone(),
                children: Vec::new(),
            })
            .collect::<Vec<_>>();

        json!({
            "title": self.title,
            "lines": compact_task_tree_view(&nodes),
        })
    }
}

fn parse_input_items(items: &[TaskItemInput], previous: &[TaskItem]) -> Result<Vec<TaskItem>> {
    let previous_metadata: Vec<_> = previous
        .iter()
        .map(|item| TaskMetadataRef {
            description: &item.description,
            metadata: &item.metadata,
        })
        .collect();
    Ok(parse_task_items(items, &previous_metadata)?
        .into_iter()
        .enumerate()
        .map(|(idx, item)| TaskItem {
            index: idx + 1,
            description: item.description,
            status: item.status,
            metadata: item.metadata,
        })
        .collect())
}

fn parse_single_index_from_path(index_path: &str) -> Result<usize> {
    let mut parts = index_path.trim().split('.');
    let first = parts.next().context("index_path cannot be empty")?;
    if parts.next().is_some() {
        bail!(
            "Hierarchical index_path '{index_path}' requires Planning workflow support. Use 'index' for standard task-tracker updates or switch to Planning workflow."
        );
    }
    let parsed = first
        .parse::<usize>()
        .with_context(|| format!("Invalid index_path '{index_path}': expected integer"))?;
    if parsed == 0 {
        bail!("index_path must be >= 1");
    }
    Ok(parsed)
}

fn parse_plan_mirror_markdown(content: &str) -> Option<TaskChecklist> {
    let mut title = String::new();
    let mut items: Vec<TaskItem> = Vec::new();
    let mut notes_lines = Vec::new();
    let mut in_notes = false;
    let mut in_verify_block = false;
    let mut idx = 1usize;

    for raw in content.lines() {
        let trimmed = raw.trim();

        if title.is_empty()
            && let Some(rest) = trimmed.strip_prefix("# ")
        {
            title = rest.trim().to_string();
            continue;
        }

        if trimmed == "## Notes" {
            in_notes = true;
            continue;
        }

        if let Some(header) = trimmed.strip_prefix("## ") {
            let lowered = header.trim().to_ascii_lowercase();
            in_notes = lowered == "notes";
            continue;
        }

        if in_notes {
            notes_lines.push(raw.to_string());
            continue;
        }

        if let Some(last) = items.last_mut() {
            let indent = raw.chars().take_while(|c| *c == ' ').count();
            if indent >= 2 && apply_metadata_line(&mut last.metadata, raw, &mut in_verify_block) {
                continue;
            }
            in_verify_block = false;
        }

        let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        else {
            continue;
        };

        if let Some((status, description)) = parse_marked_status_prefix(rest) {
            let leading_spaces = raw.chars().take_while(|c| *c == ' ').count();
            let description = format!("{}{}", " ".repeat(leading_spaces), description.trim());
            items.push(TaskItem {
                index: idx,
                description,
                status,
                metadata: TaskStepMetadata::default(),
            });
            idx += 1;
            in_verify_block = false;
        }
    }

    if title.is_empty() && items.is_empty() {
        return None;
    }

    let notes = if notes_lines.is_empty() {
        None
    } else {
        Some(notes_lines.join("\n").trim().to_string())
    };

    Some(TaskChecklist { title, items, notes })
}

fn newer_source(
    global_modified: Option<std::time::SystemTime>,
    plan_modified: Option<std::time::SystemTime>,
    planning_active: bool,
) -> TrackerSource {
    if planning_active {
        return if plan_modified.is_some() {
            TrackerSource::Plan
        } else {
            TrackerSource::Global
        };
    }

    match (global_modified, plan_modified) {
        (Some(global), Some(plan)) => {
            if global > plan {
                TrackerSource::Global
            } else if plan > global {
                TrackerSource::Plan
            } else {
                TrackerSource::Global
            }
        }
        (Some(_), None) => TrackerSource::Global,
        (None, Some(_)) => TrackerSource::Plan,
        (None, None) => {
            if planning_active {
                TrackerSource::Plan
            } else {
                TrackerSource::Global
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackerSource {
    Global,
    Plan,
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
    pub items: Option<Vec<TaskItemInput>>,

    /// Index of item to update (required for `update`, 1-indexed)
    #[serde(default)]
    pub index: Option<usize>,

    /// Hierarchical index path for update (Planning workflow, optional)
    #[serde(default)]
    pub index_path: Option<String>,

    /// New status for the item (required for `update`)
    #[serde(default)]
    pub status: Option<String>,

    /// Description for a new item (required for `add`)
    #[serde(default)]
    pub description: Option<String>,

    /// Optional file paths associated with a step
    #[serde(default)]
    pub files: Option<Vec<String>>,

    /// Optional expected outcome associated with a step
    #[serde(default)]
    pub outcome: Option<String>,

    /// Optional verification command or commands associated with a step
    #[serde(
        default,
        deserialize_with = "crate::tools::handlers::task_tracking::deserialize_optional_string_list"
    )]
    pub verify: Option<Vec<String>>,

    /// Optional parent path for add in Planning workflow (example: "2")
    #[serde(default)]
    pub parent_index_path: Option<String>,

    /// Optional notes to append
    #[serde(default)]
    pub notes: Option<String>,
}

/// Task Tracker tool state
pub struct TaskTrackerTool {
    workspace_root: PathBuf,
    planning_workflow_state: PlanningWorkflowState,
}

fn standard_task_tracker_parameter_schema() -> Value {
    json!({
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
            "items": task_items_array_schema(TASK_ITEMS_DESCRIPTION),
            "index": {
                "type": "integer",
                "minimum": 0,
                "description": "Action=update only: use a 1-based item index. index: 0 is reserved for standard checklist-level completion and is valid only with action: update and status: completed."
            },
            "index_path": {
                "type": "string",
                "pattern": "^[1-9][0-9]*$",
                "description": "Action=update only: positive flat index path for compatibility (example: '2'). Hierarchical paths are accepted only by Planning workflow."
            },
            "status": task_status_property_schema("New status for the item (used with single-item 'update')."),
            "description": {
                "type": "string",
                "description": "Description for a new item (used with 'add')."
            },
            "files": task_files_property_schema("Optional file paths associated with a single add/update item."),
            "outcome": {
                "type": "string",
                "description": "Optional expected outcome associated with a single add/update item."
            },
            "verify": task_verify_property_schema("Optional verification command or commands associated with a single add/update item."),
            "parent_index_path": {
                "type": "string",
                "description": "Optional parent path for add in Planning workflow (example: '2')."
            },
            "notes": {
                "type": "string",
                "description": "Optional notes to append to the checklist."
            }
        },
        "required": ["action"],
        "allOf": tracker_schema_allof(true)
    })
}

pub(crate) fn task_tracker_description_for_workflow(planning_active: bool) -> &'static str {
    if planning_active {
        "Adaptive task tracking for planning. Persists hierarchical plan progress under .vtcode/plans/<plan>.tasks.md and mirrors updates to .vtcode/tasks/current_task.md. Actions: create, update, list, add. Calling action=create again replaces the plan task list and its progress; use action=update or action=add to change it. For action=update, planning item indices are positive 1-based flat or hierarchical index_path values; index: 0 is invalid. Use items only to replace the full checklist with descriptions and explicit statuses, not indexed updates or JSON-encoded strings. Minimal calls: {\"action\":\"list\"}; {\"action\":\"update\",\"index\":1,\"status\":\"in_progress\"}; {\"action\":\"update\",\"items\":[\"[x] Done\",\"[ ] Remaining\"]}. List uses only action; choose one of index, index_path, or items for update; omit unused fields."
    } else {
        "Track task progress through a single checklist API (action: create | update | list | add). Use action=create to start a multi-step plan; action=update as work progresses; action=list to review state. For action=update, item indices are 1-based; checklist-level completion alone may use index: 0 with status: completed. Planning workflow accepts only positive flat or hierarchical index paths. Use items only to replace the full checklist with descriptions and explicit statuses, not indexed updates or JSON-encoded strings. Minimal calls: {\"action\":\"list\"}; {\"action\":\"update\",\"index\":1,\"status\":\"in_progress\"}; {\"action\":\"update\",\"items\":[\"[x] Done\",\"[ ] Remaining\"]}. List uses only action; choose one of index, index_path, or items for update; omit unused fields. Calling action=create again replaces the checklist unless title and items match the active one (a no-op); use action=update or action=add to change it. Tracker state mirrors .vtcode/tasks/current_task.md and active plan sidecars when available."
    }
}

pub(crate) fn task_tracker_parameter_schema_for_workflow(planning_active: bool) -> Value {
    if planning_active {
        super::planning_task_tracker::planning_task_tracker_parameter_schema()
    } else {
        standard_task_tracker_parameter_schema()
    }
}

/// Model-facing correction for `task_tracker` shape failures.
///
/// The schema uses conditional (`if`/`then`/`not`) rules, so a bare
/// "required fields" hint is not enough: `create` forbids `index`/`index_path`,
/// `update` needs exactly one shape, and `add`/`list` have their own shapes.
/// Without this, a `create` bundled with update fields fails with a generic
/// root error and the model retries blindly.
///
/// Note: the schema only *forbids* `index`/`index_path` on
/// `create`/`list`/`add`; other single-item fields (`status`, `description`,
/// `files`, `outcome`, `verify`) are ignored by those actions, not rejected.
/// The wording below matches that enforced contract: it forbids only what the
/// schema forbids and recommends (not requires) the minimal shapes.
pub const TASK_TRACKER_ARGUMENT_CORRECTION: &str = r#"Invalid task_tracker shape. List: {"action":"list"}. Individual update: {"action":"update","index":1,"status":"in_progress"} OR {"action":"update","index_path":"2.1","status":"completed"} (planning). Bulk replacement: {"action":"update","items":["[x] Done","[ ] Remaining"]}. For create, index/index_path are forbidden (use title/items); add uses description; list/add also forbid index/index_path. Never combine index with index_path or items; omit unused fields and retry once."#;

/// Whether a `task_tracker` schema error is a shape error (conditional
/// `if`/`then`/`not`, `anyOf`/`oneOf`, missing required, fallback) rather
/// than a single-field value error (`enum`, `type`, `const`, ...). The shape
/// correction above only helps the former; appending it to a bad-enum error
/// would point the model at the wrong axis.
pub(crate) fn is_task_tracker_shape_error(error_msg: &str) -> bool {
    const MARKERS: [&str; 6] = [
        "must not include",
        "does not match any allowed shape",
        "matches more than one allowed shape",
        "is forbidden by schema",
        "missing required property",
        "failed validation (schema",
    ];
    MARKERS.iter().any(|marker| error_msg.contains(marker))
}

impl TaskTrackerTool {
    pub fn new(workspace_root: PathBuf, planning_workflow_state: PlanningWorkflowState) -> Self {
        Self { workspace_root, planning_workflow_state }
    }

    fn tasks_dir(&self) -> PathBuf {
        self.workspace_root.join(".vtcode").join("tasks")
    }

    fn task_file(&self) -> PathBuf {
        self.tasks_dir().join("current_task.md")
    }

    fn display_path(&self, path: &Path) -> String {
        workspace_relative_display(&self.workspace_root, path)
    }

    async fn plan_task_file(&self) -> Option<PathBuf> {
        let plan_file = self.planning_workflow_state.get_plan_file().await?;
        tracker_file_for_plan_file(&plan_file)
    }

    async fn save_checklist(&self, checklist: &TaskChecklist) -> Result<()> {
        let dir = self.tasks_dir();
        ensure_dir_exists(&dir)
            .await
            .with_context(|| format!("Failed to create tasks directory: {}", dir.display()))?;
        let md = checklist.to_markdown();
        write_tracker_artifact(&self.task_file(), &md, "task checklist")
            .await
            .with_context(|| "Failed to write task checklist")?;
        Ok(())
    }

    async fn save_plan_mirror_to_file(&self, tracker_file: &Path, checklist: &TaskChecklist) -> Result<()> {
        persist_tracker_artifacts(Self::plan_mirror_artifacts(tracker_file, checklist).await?).await
    }

    async fn plan_mirror_artifacts(tracker_file: &Path, checklist: &TaskChecklist) -> Result<Vec<TrackerArtifact>> {
        let markdown = checklist.to_plan_markdown();
        let mut artifacts =
            vec![TrackerArtifact::new(tracker_file.to_path_buf(), markdown.clone(), "plan task tracker file").await?];
        if let Some(plan_file) = plan_file_for_tracker_file(tracker_file)
            && let Some(artifact) = TrackerArtifact::embedded_plan(plan_file, &markdown).await?
        {
            artifacts.push(artifact);
        }
        Ok(artifacts)
    }

    async fn load_global_checklist(&self) -> Result<Option<TaskChecklist>> {
        let file = self.task_file();
        if !tokio::fs::try_exists(&file).await.unwrap_or(false) {
            return Ok(None);
        }
        let content = read_file_with_context(&file, "task checklist").await?;

        let mut title = String::new();
        let mut items: Vec<TaskItem> = Vec::new();
        let mut notes_lines = Vec::new();
        let mut in_notes = false;
        let mut in_verify_block = false;
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
            if let Some(last) = items.last_mut() {
                let indent = line.chars().take_while(|c| *c == ' ').count();
                if indent >= 2 && apply_metadata_line(&mut last.metadata, line, &mut in_verify_block) {
                    continue;
                }
                in_verify_block = false;
            }
            if let Some(rest) = trimmed.strip_prefix("- ")
                && let Some((status, description)) = parse_marked_status_prefix(rest)
            {
                items.push(TaskItem {
                    index: idx,
                    description,
                    status,
                    metadata: TaskStepMetadata::default(),
                });
                idx += 1;
                in_verify_block = false;
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

        Ok(Some(TaskChecklist { title, items, notes }))
    }

    async fn load_plan_checklist_from(&self, tracker_file: &Path) -> Result<Option<TaskChecklist>> {
        if !tokio::fs::try_exists(tracker_file).await.unwrap_or(false) {
            return Ok(None);
        }
        let content = read_file_with_context(tracker_file, "plan task tracker file").await?;
        Ok(parse_plan_mirror_markdown(&content))
    }

    async fn load_preferred_checklist(&self) -> Result<Option<TaskChecklist>> {
        let task_file = self.task_file();
        let plan_file = self.plan_task_file().await;

        let global_exists = tokio::fs::try_exists(&task_file).await.unwrap_or(false);
        let plan_exists = match plan_file.as_ref() {
            Some(path) => tokio::fs::try_exists(path).await.unwrap_or(false),
            None => false,
        };

        if !global_exists && !plan_exists {
            return Ok(None);
        }

        let selected = if global_exists && plan_exists {
            let global_modified = tokio::fs::metadata(&task_file).await.ok().and_then(|meta| meta.modified().ok());
            let plan_modified = match &plan_file {
                Some(path) => tokio::fs::metadata(path).await.ok().and_then(|meta| meta.modified().ok()),
                None => None,
            };
            newer_source(global_modified, plan_modified, self.planning_workflow_state.is_active())
        } else if plan_exists {
            TrackerSource::Plan
        } else {
            TrackerSource::Global
        };

        let loaded = match selected {
            TrackerSource::Global => self.load_global_checklist().await?,
            TrackerSource::Plan => {
                if let Some(path) = plan_file.as_ref() {
                    self.load_plan_checklist_from(path).await?
                } else {
                    None
                }
            }
        };

        if let Some(checklist) = loaded.as_ref() {
            match selected {
                TrackerSource::Global => {
                    if let Some(path) = plan_file.as_ref() {
                        self.save_plan_mirror_to_file(path, checklist).await?;
                    }
                }
                TrackerSource::Plan => {
                    self.save_checklist(checklist).await?;
                }
            }
        }

        Ok(loaded)
    }

    async fn persist_edit_mode_snapshot(&self, checklist: &TaskChecklist) -> Result<()> {
        let mut artifacts =
            vec![TrackerArtifact::new(self.task_file(), checklist.to_markdown(), "task checklist").await?];
        if let Some(tracker_file) = self.plan_task_file().await {
            artifacts.extend(Self::plan_mirror_artifacts(&tracker_file, checklist).await?);
        }
        persist_tracker_artifacts(artifacts).await
    }

    async fn persist_and_build_view(&self, checklist: &TaskChecklist) -> Result<(Value, Value)> {
        self.persist_edit_mode_snapshot(checklist).await?;
        Ok((checklist.summary(), checklist.view()))
    }

    async fn persist_update(
        &self,
        previous: Option<&TaskChecklist>,
        checklist: &TaskChecklist,
        message: String,
    ) -> Result<Value> {
        let unchanged = previous == Some(checklist);
        if !unchanged {
            self.persist_edit_mode_snapshot(checklist).await?;
        }
        Ok(json!({
            "status": if unchanged { "unchanged" } else { "updated" },
            "message": if unchanged { "Checklist already matches requested state.".to_string() } else { message },
            "checklist": checklist.summary(),
            "view": checklist.view(),
        }))
    }

    fn to_plan_args(args: &TaskTrackerArgs) -> PlanningTaskTrackerArgs {
        PlanningTaskTrackerArgs {
            action: args.action.clone(),
            title: args.title.clone(),
            items: args.items.clone(),
            index: args.index,
            index_path: args.index_path.clone(),
            status: args.status.clone(),
            description: args.description.clone(),
            files: args.files.clone(),
            outcome: args.outcome.clone(),
            verify: args.verify.clone(),
            parent_index_path: args.parent_index_path.clone(),
            notes: args.notes.clone(),
        }
    }

    async fn execute_in_planning_workflow(&self, args: &TaskTrackerArgs) -> Result<Value> {
        let plan_tool = PlanningTaskTrackerTool::new(self.planning_workflow_state.clone());
        let mapped = Self::to_plan_args(args);
        plan_tool.execute(serde_json::to_value(mapped)?).await
    }

    /// Check if a `create` call is idempotent (same checklist already active).
    ///
    /// Returns `Some(unchanged_response)` when the existing checklist should be
    /// preserved, or `None` when a new checklist must be created.
    fn check_create_idempotency(
        existing: &TaskChecklist,
        title: &str,
        items: &[TaskItem],
        task_file_display: &str,
    ) -> Option<Value> {
        let same_structure = existing.title == title
            && existing.items.len() == items.len()
            && existing
                .items
                .iter()
                .zip(items.iter())
                .all(|(left, right)| left.description == right.description);
        let requested_has_explicit_status = items.iter().any(|item| item.status != TaskStatus::Pending);
        let requested_has_step_metadata = items.iter().any(|item| {
            !item.metadata.files.is_empty() || item.metadata.outcome.is_some() || !item.metadata.verify.is_empty()
        });

        let unchanged = if same_structure && !requested_has_explicit_status && !requested_has_step_metadata {
            Some("Checklist already active; preserved current progress.")
        } else if existing.title == title
            && existing.items.len() == items.len()
            && existing.items.iter().zip(items.iter()).all(|(l, r)| l == r)
        {
            Some("Requested checklist already matches current tracker state.")
        } else {
            None
        };

        unchanged.map(|message| {
            json!({
                "status": "unchanged",
                "message": message,
                "task_file": task_file_display,
                "checklist": existing.summary(),
                "view": existing.view()
            })
        })
    }

    async fn handle_create(&self, args: &TaskTrackerArgs) -> Result<Value> {
        let title = args.title.as_deref().unwrap_or("Task Checklist").to_string();
        let item_descs = args.items.as_deref().unwrap_or(&[]);
        if item_descs.is_empty() {
            anyhow::bail!("At least one item is required for 'create'. Provide items: [\"step 1\", \"step 2\", ...]");
        }

        let items = parse_input_items(item_descs, &[])?;
        if items.is_empty() {
            anyhow::bail!("No valid task items were provided for create.");
        }
        let notes = append_notes(None, args.notes.as_deref());

        let existing_checklist = self.load_preferred_checklist().await?;
        let mut existing_item_count = 0usize;
        if let Some(existing) = existing_checklist.as_ref() {
            existing_item_count = existing.items.len();
            if let Some(unchanged) =
                Self::check_create_idempotency(existing, &title, &items, &self.display_path(&self.task_file()))
            {
                return Ok(unchanged);
            }
        }

        let checklist = TaskChecklist { title, items, notes };

        let (summary, view) = self.persist_and_build_view(&checklist).await?;

        let (status, message) = if existing_item_count > 0 {
            (
                "replaced",
                format!("Previous checklist replaced with new structure (was {existing_item_count} items).",),
            )
        } else {
            ("created", "Task checklist created successfully.".to_string())
        };

        Ok(json!({
            "status": status,
            "message": message,
            "task_file": self.display_path(&self.task_file()),
            "checklist": summary,
            "view": view
        }))
    }

    async fn handle_update(&self, args: &TaskTrackerArgs) -> Result<Value> {
        validate_update_shape(args.items.as_deref(), args.index, args.index_path.as_deref(), args.status.as_deref())?;
        let is_bulk_update =
            is_bulk_sync_update(args.items.as_deref(), args.index, args.index_path.as_deref(), args.status.as_deref());
        let mut current = self.load_preferred_checklist().await?;
        let previous = current.clone();
        if is_bulk_update {
            let input_items = args.items.as_deref().unwrap_or(&[]);
            let items = parse_input_items(
                input_items,
                current.as_ref().map(|checklist| checklist.items.as_slice()).unwrap_or(&[]),
            )?;
            if items.is_empty() {
                anyhow::bail!("No valid items provided for checklist sync.");
            }

            let title = args
                .title
                .clone()
                .or_else(|| current.as_ref().map(|checklist| checklist.title.clone()))
                .unwrap_or_else(|| "Task Checklist".to_string());

            let checklist = current.get_or_insert(TaskChecklist {
                title: title.clone(),
                items: Vec::new(),
                notes: None,
            });

            checklist.title = title;
            checklist.items = items;
            checklist.notes = append_notes(checklist.notes.take(), args.notes.as_deref());
            let snapshot = checklist.clone();
            return self
                .persist_update(previous.as_ref(), &snapshot, "Checklist synchronized from provided items.".to_string())
                .await;
        }

        let checklist = current.as_mut().context("No active checklist. Use action='create' first.")?;

        let index = match (args.index, args.index_path.as_deref()) {
            (Some(idx), _) => idx,
            (None, Some(path)) => parse_single_index_from_path(path)?,
            (None, None) => {
                bail!(
                    "'index' is required for 'update' (1-indexed), or provide 'index_path' for Planning workflow updates, or 'items' for bulk sync"
                )
            }
        };

        let status_str = args
            .status
            .as_deref()
            .context("'status' is required for 'update' (pending|in_progress|completed|blocked), or provide 'items' for bulk sync")?;

        let new_status = TaskStatus::from_str(status_str)?;

        if index == 0 {
            if new_status != TaskStatus::Completed {
                bail!("index 0 is reserved for checklist-level completion; individual item indices are 1-indexed");
            }

            if let Some(outcome) = normalize_optional_text(args.outcome.as_deref()) {
                let checklist_outcome = format!("Checklist outcome: {outcome}");
                checklist.notes = append_notes(checklist.notes.take(), Some(checklist_outcome.as_str()));
            }
            checklist.notes = append_notes(checklist.notes.take(), args.notes.as_deref());

            let snapshot = checklist.clone();
            return self
                .persist_update(
                    previous.as_ref(),
                    &snapshot,
                    "Checklist-level completion acknowledged; checklist progress remains derived from item statuses."
                        .to_string(),
                )
                .await;
        }

        let item_count = checklist.items.len();
        let pos = checklist
            .items
            .iter()
            .position(|i| i.index == index)
            .with_context(|| format!("No item at index {index}. Valid range: 1-{item_count}"))?;

        let old_status = checklist.items[pos].status.to_string();
        checklist.items[pos].status = new_status;
        let new_status_str = checklist.items[pos].status.to_string();
        apply_metadata_input(
            &mut checklist.items[pos].metadata,
            args.files.as_deref(),
            args.outcome.as_deref(),
            args.verify.as_deref(),
        );
        checklist.notes = append_notes(checklist.notes.take(), args.notes.as_deref());

        let snapshot = checklist.clone();
        self.persist_update(
            previous.as_ref(),
            &snapshot,
            format!("Item {} status changed: {} → {}", index, old_status, new_status_str),
        )
        .await
    }

    async fn handle_list(&self) -> Result<Value> {
        match self.load_preferred_checklist().await? {
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
        if let Some(parent_path) = args.parent_index_path.as_deref()
            && !parent_path.trim().is_empty()
        {
            bail!(
                "'parent_index_path' is only supported for hierarchical Planning workflow updates. Use Planning workflow or omit parent_index_path for standard task-tracker updates."
            );
        }

        let mut checklist = self
            .load_preferred_checklist()
            .await?
            .context("No active checklist. Use action='create' first.")?;

        let desc = args.description.as_deref().context("'description' is required for 'add'")?;
        let (status, parsed_description) = parse_status_prefix(desc);
        let description = parsed_description.trim().to_string();
        if description.is_empty() {
            bail!("description cannot be empty");
        }

        let new_index = checklist.items.len() + 1;
        checklist.items.push(TaskItem {
            index: new_index,
            description: description.clone(),
            status,
            metadata: metadata_from_input(args.files.as_deref(), args.outcome.as_deref(), args.verify.as_deref()),
        });

        checklist.notes = append_notes(checklist.notes.take(), args.notes.as_deref());
        let (summary, view) = self.persist_and_build_view(&checklist).await?;

        Ok(json!({
            "status": "added",
            "message": format!("Added item {}: {}", new_index, description),
            "checklist": summary,
            "view": view
        }))
    }
}

#[async_trait]
impl Tool for TaskTrackerTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let args: TaskTrackerArgs = deserialize_tool_args(&args, "task_tracker")?;
        validate_action_index_fields(&args.action, args.index, args.index_path.as_deref())?;

        if self.planning_workflow_state.is_active() {
            return self.execute_in_planning_workflow(&args).await;
        }

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

    fn name(&self) -> &str {
        tools::TASK_TRACKER
    }

    fn description(&self) -> &str {
        task_tracker_description_for_workflow(self.planning_workflow_state.is_active())
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(task_tracker_parameter_schema_for_workflow(self.planning_workflow_state.is_active()))
    }

    fn is_mutating(&self) -> bool {
        false // Writes tracker artifacts only (.vtcode/tasks and .vtcode/plans)
    }

    fn is_parallel_safe(&self) -> bool {
        false // State management should be sequential
    }
}

#[cfg(test)]
mod update_regressions;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_tool(temp: &TempDir) -> (PlanningWorkflowState, TaskTrackerTool) {
        let state = PlanningWorkflowState::new(temp.path().to_path_buf());
        let tool = TaskTrackerTool::new(temp.path().to_path_buf(), state.clone());
        (state, tool)
    }

    #[tokio::test]
    async fn test_create_checklist() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);

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
        assert_eq!(result["view"]["title"], "Refactor Auth");
        assert_eq!(result["task_file"], ".vtcode/tasks/current_task.md");
    }

    #[tokio::test]
    async fn test_create_accepts_metadata_and_verify_string_forms() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);

        let result = tool
            .execute(json!({
                "action": "create",
                "title": "Harness tracker",
                "items": [
                    {
                        "description": "Analyze current harness",
                        "files": ["docs/ARCHITECTURE.md"],
                        "outcome": "Document the harness map",
                        "verify": "cargo check"
                    },
                    {
                        "description": "Wire continuation",
                        "verify": ["cargo test -p vtcode-core continuation", "cargo check -p vtcode"]
                    }
                ]
            }))
            .await
            .unwrap();

        assert_eq!(result["checklist"]["items"][0]["files"], json!(["docs/ARCHITECTURE.md"]));
        assert_eq!(result["checklist"]["items"][0]["outcome"], "Document the harness map");
        assert_eq!(result["checklist"]["items"][0]["verify"], json!(["cargo check"]));
        assert_eq!(
            result["checklist"]["items"][1]["verify"],
            json!(["cargo test -p vtcode-core continuation", "cargo check -p vtcode"])
        );

        let persisted = std::fs::read_to_string(temp.path().join(".vtcode/tasks/current_task.md")).unwrap();
        assert!(persisted.contains("files: docs/ARCHITECTURE.md"));
        assert!(persisted.contains("outcome: Document the harness map"));
        assert!(persisted.contains("verify: cargo check"));
    }

    #[tokio::test]
    async fn test_update_item() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);

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
    async fn test_update_index_zero_allows_checklist_completion_note() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);

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
                "index": 0,
                "status": "completed",
                "outcome": "Reported summary to user"
            }))
            .await
            .unwrap();

        assert_eq!(result["status"], "updated");
        assert_eq!(result["checklist"]["completed"], 0);
        assert_eq!(result["checklist"]["notes"], "Checklist outcome: Reported summary to user");
    }

    #[test]
    fn task_tracker_schema_describes_action_aware_indices() {
        let (_state, tool) = setup_tool(&TempDir::new().unwrap());
        let schema = tool.parameter_schema().expect("task tracker schema");

        assert_eq!(schema["properties"]["index"]["minimum"], 0);
        let items_description = schema["properties"]["items"]["description"].as_str().unwrap();
        assert!(items_description.contains("Full checklist replacement"));
        assert!(items_description.contains("never JSON-encoded strings"));
        assert_eq!(schema["properties"]["index_path"]["pattern"], "^[1-9][0-9]*$");
        let description = schema["properties"]["index"]["description"]
            .as_str()
            .expect("index description");
        assert!(description.contains("index: 0"));
        assert!(description.contains("action: update"));
        assert!(description.contains("completed"));
        assert_eq!(schema["allOf"][2]["then"]["properties"]["status"]["const"], "completed");
    }

    #[test]
    fn standard_task_tracker_schema_rejects_zero_except_legacy_completion() {
        let schema = standard_task_tracker_parameter_schema();
        let valid_cases = [
            json!({"action": "update", "index": 0, "status": "completed"}),
            json!({"action": "update", "index": 1, "status": "completed"}),
            json!({"action": "update", "items": ["[x] Done"]}),
        ];
        let invalid_cases = [
            json!({"action": "list", "index": 0}),
            json!({"action": "list", "index": 1}),
            json!({"action": "list", "index_path": "1"}),
            json!({"action": "add", "index": 0, "description": "New item"}),
            json!({"action": "add", "index": 1, "description": "New item"}),
            json!({"action": "create", "index": 1, "items": ["New item"]}),
            json!({"action": "update", "index": 0, "status": "pending"}),
        ];

        for args in valid_cases {
            assert!(jsonschema::validate(&schema, &args).is_ok(), "expected valid args: {args}");
        }
        for args in invalid_cases {
            assert!(jsonschema::validate(&schema, &args).is_err(), "expected invalid args: {args}");
        }
    }

    #[test]
    fn standard_task_tracker_schema_rejects_mixed_bulk_and_single_updates() {
        let schema = standard_task_tracker_parameter_schema();
        let invalid_cases = [
            json!({"action": "update", "items": ["Done"], "index": 1, "status": "completed"}),
            json!({"action": "update", "items": ["Done"], "index_path": "1", "status": "completed"}),
            json!({"action": "update", "index": 1, "index_path": "1", "status": "completed"}),
            json!({"action": "update", "items": ["Done"], "status": "completed"}),
            json!({"action": "update", "items": ["Done"], "index": 0, "status": "completed"}),
        ];

        for args in invalid_cases {
            assert!(jsonschema::validate(&schema, &args).is_err(), "expected invalid args: {args}");
        }
    }

    #[test]
    fn tracker_schemas_share_update_shapes_but_diverge_on_index_zero() {
        let standard = standard_task_tracker_parameter_schema();
        let planning = crate::tools::handlers::planning_task_tracker::planning_task_tracker_parameter_schema();

        // Shared shapes: both address modes validate on both schemas,
        // proving the normalized anyOf arm order is behavior-preserving.
        for args in [
            json!({"action": "update", "index": 1, "status": "completed"}),
            json!({"action": "update", "index_path": "1", "status": "completed"}),
            json!({"action": "update", "items": ["[x] Done"]}),
            json!({"action": "create", "title": "T", "items": ["New item"]}),
            json!({"action": "add", "description": "New item"}),
            json!({"action": "list"}),
        ] {
            assert!(jsonschema::validate(&standard, &args).is_ok(), "standard rejects shared shape: {args}");
            assert!(jsonschema::validate(&planning, &args).is_ok(), "planning rejects shared shape: {args}");
        }

        // Shared rejections on both schemas.
        for args in [
            json!({"action": "list", "index": 1}),
            json!({"action": "create", "index": 1, "items": ["New item"]}),
            json!({"action": "update", "index": 1, "index_path": "1", "status": "completed"}),
        ] {
            assert!(jsonschema::validate(&standard, &args).is_err(), "standard accepts rejected shape: {args}");
            assert!(jsonschema::validate(&planning, &args).is_err(), "planning accepts rejected shape: {args}");
        }

        // Divergent boundary: index 0 completes a standard checklist but is
        // invalid in planning (indices start at 1).
        let zero_completion = json!({"action": "update", "index": 0, "status": "completed"});
        assert!(jsonschema::validate(&standard, &zero_completion).is_ok());
        assert!(jsonschema::validate(&planning, &zero_completion).is_err());
    }

    #[tokio::test]
    async fn bulk_update_rejects_encoded_commands_without_resetting_completed_steps() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);
        tool.execute(json!({
            "action": "create",
            "title": "Preserve progress",
            "items": [
                {"description": "Update introduction", "status": "completed", "files": ["README.md"], "outcome": "Clear overview", "verify": ["review headings"]},
                {"description": "Review links", "status": "blocked", "outcome": "Await network"}
            ]
        })).await.unwrap();
        let tracker_path = temp.path().join(".vtcode/tasks/current_task.md");
        let persisted_before = std::fs::read_to_string(&tracker_path).unwrap();
        let before = tool.execute(json!({"action": "list"})).await.unwrap();

        for (action, encoded) in [
            ("update", r#"{"index_path":"1","status":"completed","outcome":"Done","verify":"review headings"}"#),
            ("update", r#"  [x] {"index_path":"1","status":"completed"}"#),
            ("create", r#"{"index":1,"status":"completed"}"#),
        ] {
            let error = tool
                .execute(json!({
                    "action": action,
                    "title": "Incorrect replacement",
                    "items": ["Replacement must not leak", encoded]
                }))
                .await
                .expect_err("encoded updates must not replace the tracker");
            assert!(error.to_string().contains("JSON-encoded task updates"));
            assert!(
                error
                    .to_string()
                    .contains(r#"{"action":"update","index_path":"1","status":"completed"}"#)
            );
            assert_eq!(tool.execute(json!({"action": "list"})).await.unwrap(), before);
            assert_eq!(std::fs::read_to_string(&tracker_path).unwrap(), persisted_before);
        }

        tool.execute(json!({"action": "update", "index_path": "2", "status": "completed"}))
            .await
            .unwrap();
        let after = tool.execute(json!({"action": "list"})).await.unwrap();
        assert_eq!(after["checklist"]["completed"], 2);
        assert_eq!(after["checklist"]["items"][0]["description"], "Update introduction");
        assert_eq!(after["checklist"]["items"][0]["files"], json!(["README.md"]));
    }

    #[tokio::test]
    async fn update_rejects_mixed_bulk_and_single_fields_before_mutation() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);

        tool.execute(json!({
            "action": "create",
            "title": "Test",
            "items": ["Original"]
        }))
        .await
        .unwrap();

        let error = tool
            .execute(json!({
                "action": "update",
                "items": ["Replacement"],
                "index": 1,
                "status": "completed"
            }))
            .await
            .expect_err("mixed update must fail closed");
        assert!(error.to_string().contains("cannot combine 'items'"));

        let result = tool.execute(json!({"action": "list"})).await.unwrap();
        assert_eq!(result["checklist"]["items"][0]["description"], "Original");
        assert_eq!(result["checklist"]["items"][0]["status"], "pending");
    }

    #[tokio::test]
    async fn update_rejects_both_index_forms_before_mutation() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);

        tool.execute(json!({
            "action": "create",
            "title": "Test",
            "items": ["Original"]
        }))
        .await
        .unwrap();

        let error = tool
            .execute(json!({
                "action": "update",
                "index": 1,
                "index_path": "1",
                "status": "completed"
            }))
            .await
            .expect_err("ambiguous index forms must fail closed");
        assert!(error.to_string().contains("cannot combine 'index' and 'index_path'"));

        let result = tool.execute(json!({"action": "list"})).await.unwrap();
        assert_eq!(result["checklist"]["items"][0]["description"], "Original");
        assert_eq!(result["checklist"]["items"][0]["status"], "pending");
    }

    #[tokio::test]
    async fn execute_rejects_indices_for_non_update_actions() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);

        let invalid_inputs = [
            json!({"action": "list", "index": 1}),
            json!({"action": "create", "index_path": "1", "items": ["New item"]}),
            json!({"action": "add", "index": 1, "description": "New item"}),
            json!({"action": "unexpected", "index_path": "1"}),
        ];

        for input in invalid_inputs {
            let error = tool.execute(input).await.expect_err("non-update index must fail closed");
            assert!(error.to_string().contains("cannot use 'index' or 'index_path'"), "unexpected error: {error}");
        }
    }

    #[test]
    fn task_tracker_metadata_switches_with_planning_state() {
        let temp = TempDir::new().unwrap();
        let (state, tool) = setup_tool(&temp);

        assert_eq!(tool.description(), task_tracker_description_for_workflow(false));
        assert_eq!(tool.parameter_schema().expect("standard schema")["properties"]["index"]["minimum"], 0);

        state.enable();

        assert_eq!(tool.description(), task_tracker_description_for_workflow(true));
        assert_eq!(tool.parameter_schema().expect("planning schema")["properties"]["index"]["minimum"], 1);
    }

    #[tokio::test]
    async fn test_add_item() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);

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
    async fn test_create_is_idempotent_for_same_structure() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);

        tool.execute(json!({
            "action": "create",
            "title": "Clippy Warnings",
            "items": ["Fix A", "Fix B"]
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

        let duplicate = tool
            .execute(json!({
                "action": "create",
                "title": "Clippy Warnings",
                "items": ["Fix A", "Fix B"]
            }))
            .await
            .unwrap();

        assert_eq!(duplicate["status"], "unchanged");
        assert_eq!(duplicate["checklist"]["completed"], 1);
    }

    #[tokio::test]
    async fn test_create_with_new_structure_replaces_checklist() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);

        tool.execute(json!({
            "action": "create",
            "title": "First",
            "items": ["Fix A", "Fix B"]
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

        let replaced = tool
            .execute(json!({
                "action": "create",
                "title": "Second",
                "items": ["Fix C"]
            }))
            .await
            .unwrap();

        assert_eq!(replaced["status"], "replaced");
        assert_eq!(replaced["checklist"]["total"], 1);
        assert_eq!(replaced["checklist"]["completed"], 0);
    }

    #[test]
    fn task_tracker_descriptions_state_that_repeat_create_replaces() {
        for planning_active in [false, true] {
            let description = task_tracker_description_for_workflow(planning_active);
            assert!(
                description.contains("Calling action=create again replaces"),
                "planning_active={planning_active}: {description}"
            );
            assert!(!description.contains("subsequent calls update"), "planning_active={planning_active}");
        }
        assert!(task_tracker_description_for_workflow(false).contains("no-op"));
    }

    #[tokio::test]
    async fn test_update_supports_bulk_item_sync() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);

        tool.execute(json!({
            "action": "create",
            "title": "Sync Test",
            "items": ["Step 1", "Step 2", "Step 3"]
        }))
        .await
        .unwrap();

        let updated = tool
            .execute(json!({
                "action": "update",
                "items": ["[x] Step 1", "[~] Step 2", "[ ] Step 3"]
            }))
            .await
            .unwrap();

        assert_eq!(updated["status"], "updated");
        assert_eq!(updated["checklist"]["completed"], 1);
        assert_eq!(updated["checklist"]["in_progress"], 1);
        assert_eq!(updated["checklist"]["pending"], 1);
    }

    #[tokio::test]
    async fn test_list_empty() {
        let temp = TempDir::new().unwrap();
        let (_state, tool) = setup_tool(&temp);

        let result = tool.execute(json!({"action": "list"})).await.unwrap();
        assert_eq!(result["status"], "empty");
    }

    #[tokio::test]
    async fn test_persistence_across_loads() {
        let temp = TempDir::new().unwrap();

        {
            let (_state, tool) = setup_tool(&temp);
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

        let (_state, tool2) = setup_tool(&temp);
        let result = tool2.execute(json!({"action": "list"})).await.unwrap();

        assert_eq!(result["status"], "ok");
        assert_eq!(result["checklist"]["total"], 2);
        assert_eq!(result["checklist"]["completed"], 1);
    }

    #[tokio::test]
    async fn test_planning_workflow_task_tracker_delegates_and_mirrors_global() {
        let temp = TempDir::new().unwrap();
        let (state, tool) = setup_tool(&temp);

        let plans_dir = state.plans_dir();
        std::fs::create_dir_all(&plans_dir).unwrap();
        let plan_file = plans_dir.join("adaptive.md");
        std::fs::write(&plan_file, "# Adaptive\n").unwrap();
        state.set_plan_file(Some(plan_file)).await;
        state.enable();

        let created = tool
            .execute(json!({
                "action": "create",
                "title": "Adaptive Plan",
                "items": ["Root task", "  Child task"]
            }))
            .await
            .unwrap();

        assert_eq!(created["status"], "created");
        assert_eq!(created["checklist"]["total"], 2);

        let task_file = temp.path().join(".vtcode/tasks/current_task.md");
        let persisted = std::fs::read_to_string(task_file).unwrap();
        assert!(persisted.contains("Root task"));
        assert!(persisted.contains("Child task"));
    }

    #[tokio::test]
    async fn test_planning_workflow_adaptive_tool_accepts_hierarchical_index_path() {
        let temp = TempDir::new().unwrap();
        let (state, tool) = setup_tool(&temp);

        let plans_dir = state.plans_dir();
        std::fs::create_dir_all(&plans_dir).unwrap();
        let plan_file = plans_dir.join("hierarchical.md");
        std::fs::write(&plan_file, "# Hierarchical\n").unwrap();
        state.set_plan_file(Some(plan_file)).await;
        state.enable();

        tool.execute(json!({
            "action": "create",
            "items": ["Parent task", "  Child task"]
        }))
        .await
        .unwrap();

        let updated = tool
            .execute(json!({
                "action": "update",
                "index_path": "1.1",
                "status": "completed"
            }))
            .await
            .unwrap();

        assert_eq!(updated["status"], "updated");
        assert_eq!(updated["checklist"]["completed"], 1);
    }

    #[tokio::test]
    async fn test_planning_workflow_adaptive_tool_rejects_index_zero() {
        let temp = TempDir::new().unwrap();
        let (state, tool) = setup_tool(&temp);

        let plans_dir = state.plans_dir();
        std::fs::create_dir_all(&plans_dir).unwrap();
        let plan_file = plans_dir.join("reject-zero.md");
        std::fs::write(&plan_file, "# Reject zero\n").unwrap();
        state.set_plan_file(Some(plan_file)).await;
        state.enable();

        tool.execute(json!({
            "action": "create",
            "items": ["Parent task"]
        }))
        .await
        .unwrap();

        let error = tool
            .execute(json!({
                "action": "update",
                "index": 0,
                "status": "completed"
            }))
            .await
            .expect_err("planning index zero must be rejected");

        assert!(error.to_string().contains("index_path components must be >= 1"));
    }

    #[tokio::test]
    async fn test_planning_workflow_mirror_preserves_notes() {
        let temp = TempDir::new().unwrap();
        let (state, tool) = setup_tool(&temp);

        let plans_dir = state.plans_dir();
        std::fs::create_dir_all(&plans_dir).unwrap();
        let plan_file = plans_dir.join("notes.md");
        std::fs::write(&plan_file, "# Notes\n").unwrap();
        state.set_plan_file(Some(plan_file)).await;
        state.enable();

        tool.execute(json!({
            "action": "create",
            "items": ["Root task"],
            "notes": "Keep this note"
        }))
        .await
        .unwrap();

        let task_file = temp.path().join(".vtcode/tasks/current_task.md");
        let persisted = std::fs::read_to_string(task_file).unwrap();
        assert!(persisted.contains("## Notes"));
        assert!(persisted.contains("Keep this note"));
    }

    #[tokio::test]
    async fn test_edit_mode_prefers_newer_plan_mirror_when_present() {
        let temp = TempDir::new().unwrap();
        let (state, tool) = setup_tool(&temp);

        let plans_dir = state.plans_dir();
        std::fs::create_dir_all(&plans_dir).unwrap();
        let plan_file = plans_dir.join("freshness.md");
        std::fs::write(&plan_file, "# Freshness\n").unwrap();
        state.set_plan_file(Some(plan_file.clone())).await;

        let global_file = temp.path().join(".vtcode/tasks/current_task.md");
        std::fs::create_dir_all(global_file.parent().unwrap()).unwrap();
        std::fs::write(&global_file, "# Freshness\n\n- [ ] stale global\n").unwrap();

        std::thread::sleep(std::time::Duration::from_millis(15));

        let sidecar = plans_dir.join("freshness.tasks.md");
        std::fs::write(&sidecar, "# Freshness\n\n## Plan of Work\n\n- [x] newer plan\n").unwrap();

        let listed = tool.execute(json!({"action": "list"})).await.unwrap();
        assert_eq!(listed["status"], "ok");
        assert_eq!(listed["checklist"]["completed"], 1);
        assert_eq!(listed["checklist"]["pending"], 0);

        let global_synced = std::fs::read_to_string(global_file).unwrap();
        assert!(global_synced.contains("newer plan"));
    }

    #[tokio::test]
    async fn test_planning_workflow_prefers_plan_sidecar_even_if_global_is_newer() {
        let temp = TempDir::new().unwrap();
        let (state, tool) = setup_tool(&temp);

        let plans_dir = state.plans_dir();
        std::fs::create_dir_all(&plans_dir).unwrap();
        let plan_file = plans_dir.join("plan-primary.md");
        std::fs::write(&plan_file, "# Plan Primary\n").unwrap();
        state.set_plan_file(Some(plan_file.clone())).await;
        state.enable();

        let global_file = temp.path().join(".vtcode/tasks/current_task.md");
        std::fs::create_dir_all(global_file.parent().unwrap()).unwrap();
        std::fs::write(&global_file, "# Plan Primary\n\n- [x] global newer\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(15));

        let sidecar = plans_dir.join("plan-primary.tasks.md");
        std::fs::write(&sidecar, "# Plan Primary\n\n## Plan of Work\n\n- [ ] plan source\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(15));
        std::fs::write(&global_file, "# Plan Primary\n\n- [x] global newest\n").unwrap();

        let listed = tool.execute(json!({"action": "list"})).await.unwrap();
        assert_eq!(listed["status"], "ok");
        assert_eq!(listed["checklist"]["pending"], 1);
        assert_eq!(listed["checklist"]["completed"], 0);
    }
}
