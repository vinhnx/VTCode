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

use super::plan_mode::PlanModeState;
use super::plan_task_tracker::{PlanTaskTrackerArgs, PlanTaskTrackerTool};
use std::str::FromStr;

use crate::config::constants::tools;
use crate::tools::handlers::task_tracking::{
    TaskCounts, TaskItemInput, TaskStepMetadata, TaskTrackingStatus, append_notes,
    append_notes_section, append_task_step_metadata, is_bulk_sync_update, metadata_from_input,
    normalize_optional_text, normalize_string_items, parse_marked_status_prefix,
    parse_status_prefix,
};
use crate::utils::file_utils::{
    ensure_dir_exists, read_file_with_context, write_file_with_context,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

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
            md.push_str(&format!(
                "- {} {}\n",
                item.status.flat_checkbox(),
                item.description
            ));
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
            md.push_str(&format!(
                "{}- {} {}\n",
                indent,
                item.status.plan_checkbox(),
                trimmed
            ));
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
        let mut lines = Vec::new();
        for (idx, item) in self.items.iter().enumerate() {
            let branch = if idx + 1 == self.items.len() {
                "└"
            } else {
                "├"
            };
            lines.push(json!({
                "display": format!("{} {} {}", branch, item.status.view_symbol(), item.description),
                "status": item.status.to_string(),
                "text": item.description,
                "index_path": item.index.to_string(),
                "files": item.metadata.files.clone(),
                "outcome": item.metadata.outcome.clone(),
                "verify": item.metadata.verify.clone(),
            }));

            if !item.metadata.files.is_empty() {
                lines.push(json!({
                    "display": format!("  files: {}", item.metadata.files.join(", ")),
                    "status": item.status.to_string(),
                    "text": format!("files: {}", item.metadata.files.join(", ")),
                }));
            }
            if let Some(outcome) = item.metadata.outcome.as_deref() {
                lines.push(json!({
                    "display": format!("  outcome: {}", outcome),
                    "status": item.status.to_string(),
                    "text": format!("outcome: {}", outcome),
                }));
            }
            for command in &item.metadata.verify {
                lines.push(json!({
                    "display": format!("  verify: {}", command),
                    "status": item.status.to_string(),
                    "text": format!("verify: {}", command),
                }));
            }
        }

        json!({
            "title": self.title,
            "lines": lines,
        })
    }
}

fn parse_input_items(items: &[TaskItemInput]) -> Result<Vec<TaskItem>> {
    items
        .iter()
        .filter_map(|item| match item {
            TaskItemInput::Text(raw) => {
                let (status, description) = parse_status_prefix(raw);
                let description = description.trim().to_string();
                if description.is_empty() {
                    return None;
                }
                Some(Ok((status, description, TaskStepMetadata::default())))
            }
            TaskItemInput::Structured(payload) => {
                let (parsed_status, parsed_description) = parse_status_prefix(&payload.description);
                let description = parsed_description.trim().to_string();
                if description.is_empty() {
                    return None;
                }
                let status = match payload.status.as_deref() {
                    Some(raw) => match TaskStatus::from_str(raw) {
                        Ok(status) => status,
                        Err(err) => return Some(Err(err)),
                    },
                    None => parsed_status,
                };
                let metadata = metadata_from_input(
                    payload.files.as_deref(),
                    payload.outcome.as_deref(),
                    payload.verify.as_deref(),
                );
                Some(Ok((status, description, metadata)))
            }
        })
        .enumerate()
        .map(|(idx, item)| {
            let (status, description, metadata) = item?;
            Ok(TaskItem {
                index: idx + 1,
                description,
                status,
                metadata,
            })
        })
        .collect()
}

fn parse_single_index_from_path(index_path: &str) -> Result<usize> {
    let mut parts = index_path.trim().split('.');
    let first = parts.next().context("index_path cannot be empty")?;
    if parts.next().is_some() {
        bail!(
            "Hierarchical index_path '{}' requires Plan Mode support. Use 'index' in Edit mode or switch to Plan Mode.",
            index_path
        );
    }
    let parsed = first
        .parse::<usize>()
        .with_context(|| format!("Invalid index_path '{}': expected integer", index_path))?;
    if parsed == 0 {
        bail!("index_path must be >= 1");
    }
    Ok(parsed)
}

fn parse_files_metadata(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn apply_task_metadata_line(item: &mut TaskItem, raw: &str, in_verify_block: &mut bool) -> bool {
    let trimmed = raw.trim_start();

    if *in_verify_block {
        if let Some(command) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            if let Some(command) = normalize_optional_text(Some(command)) {
                item.metadata.verify.push(command);
            }
            return true;
        }
        *in_verify_block = false;
    }

    if let Some(rest) = trimmed.strip_prefix("files:") {
        item.metadata.files = parse_files_metadata(rest);
        return true;
    }

    if let Some(rest) = trimmed.strip_prefix("outcome:") {
        item.metadata.outcome = normalize_optional_text(Some(rest));
        return true;
    }

    if trimmed == "verify:" {
        item.metadata.verify.clear();
        *in_verify_block = true;
        return true;
    }

    if let Some(rest) = trimmed.strip_prefix("verify:") {
        item.metadata.verify = normalize_string_items(Some(&[rest.to_string()]));
        return true;
    }

    false
}

fn parse_plan_mirror_markdown(content: &str) -> Option<TaskChecklist> {
    let mut title = String::new();
    let mut items = Vec::new();
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
            if indent >= 2 && apply_task_metadata_line(last, raw, &mut in_verify_block) {
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

    Some(TaskChecklist {
        title,
        items,
        notes,
    })
}

fn newer_source(
    global_modified: Option<std::time::SystemTime>,
    plan_modified: Option<std::time::SystemTime>,
    plan_mode: bool,
) -> TrackerSource {
    if plan_mode {
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
            if plan_mode {
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

    /// Hierarchical index path for update (Plan Mode, optional)
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

    /// Optional parent path for add in Plan Mode (example: "2")
    #[serde(default)]
    pub parent_index_path: Option<String>,

    /// Optional notes to append
    #[serde(default)]
    pub notes: Option<String>,
}

/// Task Tracker tool state
pub struct TaskTrackerTool {
    workspace_root: PathBuf,
    plan_mode_state: PlanModeState,
    checklist: Arc<RwLock<Option<TaskChecklist>>>,
}

impl TaskTrackerTool {
    pub fn new(workspace_root: PathBuf, plan_mode_state: PlanModeState) -> Self {
        Self {
            workspace_root,
            plan_mode_state,
            checklist: Arc::new(RwLock::new(None)),
        }
    }

    fn tasks_dir(&self) -> PathBuf {
        self.workspace_root.join(".vtcode").join("tasks")
    }

    fn task_file(&self) -> PathBuf {
        self.tasks_dir().join("current_task.md")
    }

    async fn plan_task_file(&self) -> Option<PathBuf> {
        let plan_file = self.plan_mode_state.get_plan_file().await?;
        let stem = plan_file.file_stem()?.to_str()?;
        Some(plan_file.with_file_name(format!("{stem}.tasks.md")))
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

    async fn save_plan_mirror_to_file(
        &self,
        tracker_file: &Path,
        checklist: &TaskChecklist,
    ) -> Result<()> {
        if let Some(parent) = tracker_file.parent() {
            ensure_dir_exists(parent).await.with_context(|| {
                format!(
                    "Failed to create plan tracker directory: {}",
                    parent.display()
                )
            })?;
        }
        write_file_with_context(
            tracker_file,
            &checklist.to_plan_markdown(),
            "plan task tracker file",
        )
        .await
        .with_context(|| {
            format!(
                "Failed to write plan task tracker file: {}",
                tracker_file.display()
            )
        })?;
        Ok(())
    }

    async fn save_plan_mirror(&self, checklist: &TaskChecklist) -> Result<()> {
        let Some(tracker_file) = self.plan_task_file().await else {
            return Ok(());
        };
        self.save_plan_mirror_to_file(&tracker_file, checklist)
            .await?;
        Ok(())
    }

    async fn load_global_checklist(&self) -> Result<Option<TaskChecklist>> {
        let file = self.task_file();
        if !file.exists() {
            return Ok(None);
        }
        let content = read_file_with_context(&file, "task checklist").await?;

        let mut title = String::new();
        let mut items = Vec::new();
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
                if indent >= 2 && apply_task_metadata_line(last, line, &mut in_verify_block) {
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

        Ok(Some(TaskChecklist {
            title,
            items,
            notes,
        }))
    }

    async fn load_plan_checklist_from(&self, tracker_file: &Path) -> Result<Option<TaskChecklist>> {
        if !tracker_file.exists() {
            return Ok(None);
        }
        let content = read_file_with_context(tracker_file, "plan task tracker file").await?;
        Ok(parse_plan_mirror_markdown(&content))
    }

    async fn load_preferred_checklist(&self) -> Result<Option<TaskChecklist>> {
        let task_file = self.task_file();
        let plan_file = self.plan_task_file().await;

        let global_exists = task_file.exists();
        let plan_exists = plan_file.as_ref().is_some_and(|path| path.exists());

        if !global_exists && !plan_exists {
            return Ok(None);
        }

        let selected = if global_exists && plan_exists {
            let global_modified = std::fs::metadata(&task_file)
                .ok()
                .and_then(|meta| meta.modified().ok());
            let plan_modified = plan_file
                .as_ref()
                .and_then(|path| std::fs::metadata(path).ok())
                .and_then(|meta| meta.modified().ok());
            newer_source(
                global_modified,
                plan_modified,
                self.plan_mode_state.is_active(),
            )
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

    async fn ensure_checklist_loaded(&self) -> Result<()> {
        let loaded = self.load_preferred_checklist().await?;
        let mut guard = self.checklist.write().await;
        *guard = loaded;
        Ok(())
    }

    async fn persist_edit_mode_snapshot(&self, checklist: &TaskChecklist) -> Result<()> {
        self.save_checklist(checklist).await?;
        self.save_plan_mirror(checklist).await?;
        Ok(())
    }

    async fn persist_and_build_view(&self, checklist: &TaskChecklist) -> Result<(Value, Value)> {
        self.persist_edit_mode_snapshot(checklist).await?;
        Ok((checklist.summary(), checklist.view()))
    }

    fn to_plan_args(args: &TaskTrackerArgs) -> PlanTaskTrackerArgs {
        PlanTaskTrackerArgs {
            action: args.action.clone(),
            title: args.title.clone(),
            items: args.items.clone(),
            index: args.index,
            index_path: args
                .index_path
                .clone()
                .or_else(|| args.index.map(|value| value.to_string())),
            status: args.status.clone(),
            description: args.description.clone(),
            files: args.files.clone(),
            outcome: args.outcome.clone(),
            verify: args.verify.clone(),
            parent_index_path: args.parent_index_path.clone(),
            notes: args.notes.clone(),
        }
    }

    async fn execute_in_plan_mode(&self, args: &TaskTrackerArgs) -> Result<Value> {
        let plan_tool = PlanTaskTrackerTool::new(self.plan_mode_state.clone());
        let mapped = Self::to_plan_args(args);
        let output = plan_tool.execute(serde_json::to_value(mapped)?).await?;
        self.ensure_checklist_loaded().await?;

        Ok(output)
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

        let items = parse_input_items(item_descs)?;
        if items.is_empty() {
            anyhow::bail!("No valid task items were provided for create.");
        }
        let notes = append_notes(None, args.notes.as_deref());
        let requested = TaskChecklist {
            title: title.clone(),
            items: items.clone(),
            notes: notes.clone(),
        };

        self.ensure_checklist_loaded().await?;
        let guard = self.checklist.write().await;
        if let Some(existing) = guard.as_ref() {
            let same_structure = existing.title == title
                && existing.items.len() == items.len()
                && existing
                    .items
                    .iter()
                    .zip(items.iter())
                    .all(|(left, right)| left.description == right.description);
            let requested_has_explicit_status =
                items.iter().any(|item| item.status != TaskStatus::Pending);
            let requested_has_step_metadata = items.iter().any(|item| {
                !item.metadata.files.is_empty()
                    || item.metadata.outcome.is_some()
                    || !item.metadata.verify.is_empty()
            });
            if same_structure && !requested_has_explicit_status && !requested_has_step_metadata {
                return Ok(json!({
                    "status": "unchanged",
                    "message": "Checklist already active; preserved current progress.",
                    "task_file": self.task_file().display().to_string(),
                    "checklist": existing.summary(),
                    "view": existing.view()
                }));
            }

            if existing == &requested {
                return Ok(json!({
                    "status": "unchanged",
                    "message": "Requested checklist already matches current tracker state.",
                    "task_file": self.task_file().display().to_string(),
                    "checklist": existing.summary(),
                    "view": existing.view()
                }));
            }
        }

        let checklist = TaskChecklist {
            title,
            items,
            notes,
        };

        drop(guard);
        let (summary, view) = self.persist_and_build_view(&checklist).await?;
        let mut guard = self.checklist.write().await;
        *guard = Some(checklist);

        Ok(json!({
            "status": "created",
            "message": "Task checklist created successfully.",
            "task_file": self.task_file().display().to_string(),
            "checklist": summary,
            "view": view
        }))
    }

    async fn handle_update(&self, args: &TaskTrackerArgs) -> Result<Value> {
        self.ensure_checklist_loaded().await?;
        let mut guard = self.checklist.write().await;
        if is_bulk_sync_update(
            args.items.as_deref(),
            args.index,
            args.index_path.as_deref(),
            args.status.as_deref(),
        ) {
            let input_items = args.items.as_deref().unwrap_or(&[]);
            let items = parse_input_items(input_items)?;
            if items.is_empty() {
                anyhow::bail!("No valid items provided for checklist sync.");
            }

            let title = args
                .title
                .clone()
                .or_else(|| guard.as_ref().map(|checklist| checklist.title.clone()))
                .unwrap_or_else(|| "Task Checklist".to_string());

            let checklist = guard.get_or_insert(TaskChecklist {
                title: title.clone(),
                items: Vec::new(),
                notes: None,
            });

            checklist.title = title;
            checklist.items = items;
            checklist.notes = append_notes(checklist.notes.take(), args.notes.as_deref());
            let snapshot = checklist.clone();
            drop(guard);
            let (summary, view) = self.persist_and_build_view(&snapshot).await?;
            return Ok(json!({
                "status": "updated",
                "message": "Checklist synchronized from provided items.",
                "checklist": summary,
                "view": view
            }));
        }

        let checklist = guard
            .as_mut()
            .context("No active checklist. Use action='create' first.")?;

        let index = match (args.index, args.index_path.as_deref()) {
            (Some(idx), _) => idx,
            (None, Some(path)) => parse_single_index_from_path(path)?,
            (None, None) => {
                bail!(
                    "'index' is required for 'update' (1-indexed), or provide 'index_path' for adaptive mode, or 'items' for bulk sync"
                )
            }
        };

        let status_str = args
            .status
            .as_deref()
            .context("'status' is required for 'update' (pending|in_progress|completed|blocked), or provide 'items' for bulk sync")?;

        let new_status = TaskStatus::from_str(status_str)?;

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
        if let Some(files) = args.files.as_deref() {
            checklist.items[pos].metadata.files = normalize_string_items(Some(files));
        }
        if args.outcome.is_some() {
            checklist.items[pos].metadata.outcome =
                normalize_optional_text(args.outcome.as_deref());
        }
        if let Some(verify) = args.verify.as_deref() {
            checklist.items[pos].metadata.verify = normalize_string_items(Some(verify));
        }
        checklist.notes = append_notes(checklist.notes.take(), args.notes.as_deref());

        let snapshot = checklist.clone();
        drop(guard);
        let (summary, view) = self.persist_and_build_view(&snapshot).await?;

        Ok(json!({
            "status": "updated",
            "message": format!("Item {} status changed: {} → {}", index, old_status, new_status_str),
            "checklist": summary,
            "view": view
        }))
    }

    async fn handle_list(&self) -> Result<Value> {
        self.ensure_checklist_loaded().await?;
        let guard = self.checklist.read().await;

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
        if let Some(parent_path) = args.parent_index_path.as_deref()
            && !parent_path.trim().is_empty()
        {
            bail!(
                "'parent_index_path' is only supported for hierarchical Plan Mode updates. Use Plan Mode or omit parent_index_path in Edit mode."
            );
        }

        self.ensure_checklist_loaded().await?;
        let mut guard = self.checklist.write().await;
        let checklist = guard
            .as_mut()
            .context("No active checklist. Use action='create' first.")?;

        let desc = args
            .description
            .as_deref()
            .context("'description' is required for 'add'")?;
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
            metadata: metadata_from_input(
                args.files.as_deref(),
                args.outcome.as_deref(),
                args.verify.as_deref(),
            ),
        });

        checklist.notes = append_notes(checklist.notes.take(), args.notes.as_deref());
        let snapshot = checklist.clone();
        drop(guard);
        let (summary, view) = self.persist_and_build_view(&snapshot).await?;

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
        let args: TaskTrackerArgs = serde_json::from_value(args)
            .context("Invalid task_tracker arguments. Required: {\"action\": \"create|update|list|add\", ...}")?;

        if self.plan_mode_state.is_active() {
            return self.execute_in_plan_mode(&args).await;
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

    fn name(&self) -> &'static str {
        tools::TASK_TRACKER
    }

    fn description(&self) -> &'static str {
        "Adaptive task tracker for both Plan and Edit modes. Uses one checklist API (`create|update|list|add`) and mirrors tracker state between `.vtcode/tasks/current_task.md` and active plan sidecar files when available."
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
                    "items": {
                        "anyOf": [
                            { "type": "string" },
                            {
                                "type": "object",
                                "properties": {
                                    "description": { "type": "string" },
                                    "status": {
                                        "type": "string",
                                        "enum": ["pending", "in_progress", "completed", "blocked"]
                                    },
                                    "files": {
                                        "type": "array",
                                        "items": { "type": "string" }
                                    },
                                    "outcome": { "type": "string" },
                                    "verify": {
                                        "anyOf": [
                                            { "type": "string" },
                                            {
                                                "type": "array",
                                                "items": { "type": "string" }
                                            }
                                        ]
                                    }
                                },
                                "required": ["description"]
                            }
                        ]
                    },
                    "description": "List of task descriptions or structured task items (used with 'create'; also supports bulk 'update' sync with optional [x]/[~]/[!]/[ ] prefixes and indentation for hierarchy in Plan Mode)."
                },
                "index": {
                    "type": "integer",
                    "description": "1-indexed item number to update (flat mode)."
                },
                "index_path": {
                    "type": "string",
                    "description": "Hierarchical index path for update in Plan Mode (example: '2.1'). Single value (e.g. '2') also works in Edit mode."
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "blocked"],
                    "description": "New status for the item (used with single-item 'update')."
                },
                "description": {
                    "type": "string",
                    "description": "Description for a new item (used with 'add')."
                },
                "files": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional file paths associated with a single add/update item."
                },
                "outcome": {
                    "type": "string",
                    "description": "Optional expected outcome associated with a single add/update item."
                },
                "verify": {
                    "anyOf": [
                        { "type": "string" },
                        {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    ],
                    "description": "Optional verification command or commands associated with a single add/update item."
                },
                "parent_index_path": {
                    "type": "string",
                    "description": "Optional parent path for add in Plan Mode (example: '2')."
                },
                "notes": {
                    "type": "string",
                    "description": "Optional notes to append to the checklist."
                }
            },
            "required": ["action"],
            "allOf": [
                {
                    "if": {
                        "properties": { "action": { "const": "create" } },
                        "required": ["action"]
                    },
                    "then": {
                        "required": ["items"]
                    }
                },
                {
                    "if": {
                        "properties": { "action": { "const": "update" } },
                        "required": ["action"]
                    },
                    "then": {
                        "anyOf": [
                            { "required": ["index", "status"] },
                            { "required": ["index_path", "status"] },
                            { "required": ["items"] }
                        ]
                    }
                },
                {
                    "if": {
                        "properties": { "action": { "const": "add" } },
                        "required": ["action"]
                    },
                    "then": {
                        "required": ["description"]
                    }
                }
            ]
        }))
    }

    fn is_mutating(&self) -> bool {
        false // Writes tracker artifacts only (.vtcode/tasks and .vtcode/plans)
    }

    fn is_parallel_safe(&self) -> bool {
        false // State management should be sequential
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_tool(temp: &TempDir) -> (PlanModeState, TaskTrackerTool) {
        let state = PlanModeState::new(temp.path().to_path_buf());
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

        assert_eq!(
            result["checklist"]["items"][0]["files"],
            json!(["docs/ARCHITECTURE.md"])
        );
        assert_eq!(
            result["checklist"]["items"][0]["outcome"],
            "Document the harness map"
        );
        assert_eq!(
            result["checklist"]["items"][0]["verify"],
            json!(["cargo check"])
        );
        assert_eq!(
            result["checklist"]["items"][1]["verify"],
            json!([
                "cargo test -p vtcode-core continuation",
                "cargo check -p vtcode"
            ])
        );

        let persisted =
            std::fs::read_to_string(temp.path().join(".vtcode/tasks/current_task.md")).unwrap();
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
    async fn test_plan_mode_task_tracker_delegates_and_mirrors_global() {
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
    async fn test_plan_mode_mirror_preserves_notes() {
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
        std::fs::write(
            &sidecar,
            "# Freshness\n\n## Plan of Work\n\n- [x] newer plan\n",
        )
        .unwrap();

        let listed = tool.execute(json!({"action": "list"})).await.unwrap();
        assert_eq!(listed["status"], "ok");
        assert_eq!(listed["checklist"]["completed"], 1);
        assert_eq!(listed["checklist"]["pending"], 0);

        let global_synced = std::fs::read_to_string(global_file).unwrap();
        assert!(global_synced.contains("newer plan"));
    }

    #[tokio::test]
    async fn test_plan_mode_prefers_plan_sidecar_even_if_global_is_newer() {
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
        std::fs::write(
            &sidecar,
            "# Plan Primary\n\n## Plan of Work\n\n- [ ] plan source\n",
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(15));
        std::fs::write(&global_file, "# Plan Primary\n\n- [x] global newest\n").unwrap();

        let listed = tool.execute(json!({"action": "list"})).await.unwrap();
        assert_eq!(listed["status"], "ok");
        assert_eq!(listed["checklist"]["pending"], 1);
        assert_eq!(listed["checklist"]["completed"], 0);
    }
}
