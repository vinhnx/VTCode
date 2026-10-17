//! Planning workflow scoped task tracker persisted next to the active plan file.
//!
//! This tracker is intended for Planning workflow only and writes a sidecar markdown
//! file next to the active plan file (`<plan>.tasks.md`).

use super::planning_workflow::{PlanningWorkflowState, tracker_file_for_plan_file};
use crate::config::constants::tools;
use crate::tools::error_helpers::deserialize_tool_args;
use crate::tools::handlers::task_tracking::{
    TASK_ITEMS_DESCRIPTION, TaskCounts, TaskItemInput, TaskMetadataRef, TaskStepMetadata, TaskTrackingStatus,
    TaskTreeNode, TrackerArtifact, append_notes, append_notes_section, append_task_step_metadata, apply_metadata_input,
    apply_metadata_line, compact_task_tree_view, is_bulk_sync_update, metadata_from_input, parse_marked_status_prefix,
    parse_status_prefix, parse_task_items, persist_tracker_artifacts, task_files_property_schema,
    task_items_array_schema, task_status_property_schema, task_verify_property_schema, tracker_schema_allof,
    validate_action_index_fields, validate_update_shape,
};
use crate::tools::traits::Tool;
use crate::utils::file_utils::read_file_with_context;
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use vtcode_commons::workspace_relative_display;

type PlanTaskStatus = TaskTrackingStatus;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PlanTaskNode {
    description: String,
    status: PlanTaskStatus,
    #[serde(default, flatten)]
    metadata: TaskStepMetadata,
    children: Vec<PlanTaskNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PlanTaskDocument {
    title: String,
    items: Vec<PlanTaskNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningTaskTrackerArgs {
    /// Action to perform: create, update, list, add
    pub action: String,

    /// Title for the checklist (used with create)
    #[serde(default)]
    pub title: Option<String>,

    /// Initial tasks for create
    #[serde(default)]
    pub items: Option<Vec<TaskItemInput>>,

    /// Hierarchical index path (example: "2.1")
    #[serde(default)]
    pub index_path: Option<String>,

    /// Flat index fallback for compatibility with task_tracker calls
    #[serde(default)]
    pub index: Option<usize>,

    /// New status for update
    #[serde(default)]
    pub status: Option<String>,

    /// Description for add
    #[serde(default)]
    pub description: Option<String>,

    /// Optional file paths associated with a single add/update step
    #[serde(default)]
    pub files: Option<Vec<String>>,

    /// Optional expected outcome associated with a single add/update step
    #[serde(default)]
    pub outcome: Option<String>,

    /// Optional verification command or commands associated with a single add/update step
    #[serde(
        default,
        deserialize_with = "crate::tools::handlers::task_tracking::deserialize_optional_string_list"
    )]
    pub verify: Option<Vec<String>>,

    /// Parent path for add (example: "2")
    #[serde(default)]
    pub parent_index_path: Option<String>,

    /// Optional notes to append
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
struct FlatTaskLine {
    level: usize,
    status: PlanTaskStatus,
    description: String,
    metadata: TaskStepMetadata,
}

impl PlanTaskDocument {
    fn to_markdown(&self) -> String {
        let mut out = format!("# {}\n\n## Plan of Work\n\n", self.title);
        write_markdown_nodes(&self.items, 0, &mut out);
        append_notes_section(&mut out, self.notes.as_deref());
        out
    }

    fn summary_json(&self) -> Value {
        let mut counts = TaskCounts::default();
        count_nodes(&self.items, &mut counts);

        json!({
            "title": self.title,
            "total": counts.total,
            "completed": counts.completed,
            "in_progress": counts.in_progress,
            "pending": counts.pending,
            "blocked": counts.blocked,
            "progress_percent": counts.progress_percent(),
            "items": flatten_items_json(&self.items),
            "notes": self.notes.clone(),
        })
    }

    fn view_json(&self) -> Value {
        json!({
            "title": self.title,
            "lines": compact_task_tree_view(&compact_view_nodes(&self.items, "")),
        })
    }
}

fn count_nodes(nodes: &[PlanTaskNode], counts: &mut TaskCounts) {
    for node in nodes {
        counts.add(&node.status);
        count_nodes(&node.children, counts);
    }
}

fn write_markdown_nodes(nodes: &[PlanTaskNode], level: usize, out: &mut String) {
    let indent = "  ".repeat(level);
    for node in nodes {
        out.push_str(&indent);
        out.push_str("- ");
        out.push_str(node.status.plan_checkbox());
        out.push(' ');
        out.push_str(&node.description);
        out.push('\n');
        append_task_step_metadata(out, &indent, &node.metadata);
        write_markdown_nodes(&node.children, level + 1, out);
    }
}

fn flatten_items_json(nodes: &[PlanTaskNode]) -> Vec<Value> {
    let mut items = Vec::new();
    flatten_items_json_inner(nodes, "", 0, &mut items);
    items
}

fn flatten_for_global_items(nodes: &[PlanTaskNode], level: usize, out: &mut Vec<FlatTaskLine>) {
    for node in nodes {
        out.push(FlatTaskLine {
            level,
            status: node.status,
            description: node.description.clone(),
            metadata: node.metadata.clone(),
        });
        flatten_for_global_items(&node.children, level + 1, out);
    }
}

fn flatten_items_json_inner(nodes: &[PlanTaskNode], index_prefix: &str, level: usize, out: &mut Vec<Value>) {
    for (idx, node) in nodes.iter().enumerate() {
        let index_path = if index_prefix.is_empty() {
            format!("{}", idx + 1)
        } else {
            format!("{index_prefix}.{}", idx + 1)
        };
        out.push(json!({
            "index_path": index_path,
            "description": node.description,
            "status": node.status.as_str(),
            "level": level,
            "files": node.metadata.files.clone(),
            "outcome": node.metadata.outcome.clone(),
            "verify": node.metadata.verify.clone(),
        }));
        flatten_items_json_inner(&node.children, &index_path, level + 1, out);
    }
}

fn compact_view_nodes(nodes: &[PlanTaskNode], index_prefix: &str) -> Vec<TaskTreeNode> {
    nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let index_path = if index_prefix.is_empty() {
                format!("{}", index + 1)
            } else {
                format!("{index_prefix}.{}", index + 1)
            };
            TaskTreeNode {
                index_path: index_path.clone(),
                description: node.description.clone(),
                status: node.status,
                metadata: node.metadata.clone(),
                children: compact_view_nodes(&node.children, &index_path),
            }
        })
        .collect()
}

fn parse_task_line(line: &str) -> Option<FlatTaskLine> {
    let indent_spaces = line.chars().take_while(|c| *c == ' ').count();
    let level = indent_spaces / 2;
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))?;

    let (status, description) = parse_marked_status_prefix(rest)?;
    if description.trim().is_empty() {
        return None;
    }
    Some(FlatTaskLine {
        level,
        status,
        description: description.trim().to_string(),
        metadata: TaskStepMetadata::default(),
    })
}

fn build_tree_from_flat(lines: &[FlatTaskLine]) -> Vec<PlanTaskNode> {
    let mut roots = Vec::<PlanTaskNode>::new();
    let mut current_path = Vec::<usize>::new();
    let mut previous_level = 0usize;

    for line in lines {
        let mut level = line.level;
        if level > previous_level + 1 {
            level = previous_level + 1;
        }
        while current_path.len() > level {
            current_path.pop();
        }
        if level > current_path.len() {
            level = current_path.len();
        }

        let node = PlanTaskNode {
            description: line.description.clone(),
            status: line.status,
            metadata: line.metadata.clone(),
            children: Vec::new(),
        };

        if level == 0 || current_path.is_empty() {
            roots.push(node);
            current_path.clear();
            current_path.push(roots.len() - 1);
            previous_level = 0;
            continue;
        }

        if let Some(parent) = get_node_mut_by_indices(&mut roots, &current_path) {
            parent.children.push(node);
            let child_idx = parent.children.len() - 1;
            current_path.push(child_idx);
        } else {
            roots.push(node);
            current_path.clear();
            current_path.push(roots.len() - 1);
        }

        previous_level = level;
    }

    roots
}

fn get_node_mut_by_indices<'a>(nodes: &'a mut [PlanTaskNode], path: &[usize]) -> Option<&'a mut PlanTaskNode> {
    let (&head, tail) = path.split_first()?;
    let node = nodes.get_mut(head)?;
    if tail.is_empty() {
        Some(node)
    } else {
        get_node_mut_by_indices(node.children.as_mut_slice(), tail)
    }
}

fn get_node_mut_by_index_path<'a>(nodes: &'a mut [PlanTaskNode], path: &[usize]) -> Option<&'a mut PlanTaskNode> {
    let (&head, tail) = path.split_first()?;
    let idx = head.checked_sub(1)?;
    let node = nodes.get_mut(idx)?;
    if tail.is_empty() {
        Some(node)
    } else {
        get_node_mut_by_index_path(node.children.as_mut_slice(), tail)
    }
}

fn parse_index_path(value: &str) -> Result<Vec<usize>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("index_path cannot be empty");
    }

    trimmed
        .split('.')
        .map(|token| {
            let parsed = token
                .parse::<usize>()
                .with_context(|| format!("Invalid index component '{token}'"))?;
            if parsed == 0 {
                bail!("index_path components must be >= 1");
            }
            Ok(parsed)
        })
        .collect()
}

fn parse_document_from_markdown(content: &str) -> Option<PlanTaskDocument> {
    let mut title = String::new();
    let mut in_plan_section = false;
    let mut in_notes = false;
    let mut notes_lines = Vec::new();
    let mut task_lines = Vec::<FlatTaskLine>::new();
    let mut in_verify_block = false;

    for raw in content.lines() {
        let trimmed = raw.trim();

        if title.is_empty()
            && let Some(rest) = trimmed.strip_prefix("# ")
        {
            title = rest.trim().to_string();
            continue;
        }

        if let Some(header) = trimmed.strip_prefix("## ") {
            let lowered = header.trim().to_ascii_lowercase();
            in_plan_section = matches!(lowered.as_str(), "plan of work" | "concrete steps" | "updated plan")
                || lowered.starts_with("phase ");
            in_notes = lowered == "notes";
            continue;
        }

        if in_notes {
            notes_lines.push(raw.to_string());
            continue;
        }

        if in_plan_section {
            if let Some(line) = parse_task_line(raw) {
                task_lines.push(line);
                in_verify_block = false;
                continue;
            }

            if let Some(last) = task_lines.last_mut() {
                let leading_spaces = raw.chars().take_while(|c| *c == ' ').count();
                let min_indent = (last.level + 1) * 2;
                if leading_spaces >= min_indent && apply_metadata_line(&mut last.metadata, raw, &mut in_verify_block) {
                    continue;
                }
            }
            in_verify_block = false;
        }
    }

    if title.is_empty() && task_lines.is_empty() {
        return None;
    }

    let notes = if notes_lines.is_empty() {
        None
    } else {
        Some(notes_lines.join("\n").trim().to_string())
    };
    let items = build_tree_from_flat(&task_lines);

    Some(PlanTaskDocument { title, items, notes })
}

fn collect_task_metadata<'a>(nodes: &'a [PlanTaskNode], metadata: &mut Vec<TaskMetadataRef<'a>>) {
    for node in nodes {
        metadata.push(TaskMetadataRef {
            description: &node.description,
            metadata: &node.metadata,
        });
        collect_task_metadata(&node.children, metadata);
    }
}

fn build_flat_task_lines(items: &[TaskItemInput], previous: &[PlanTaskNode]) -> Result<Vec<FlatTaskLine>> {
    let mut previous_metadata = Vec::new();
    collect_task_metadata(previous, &mut previous_metadata);
    Ok(parse_task_items(items, &previous_metadata)?
        .into_iter()
        .map(|item| FlatTaskLine {
            level: item.level,
            status: item.status,
            description: item.description,
            metadata: item.metadata,
        })
        .collect())
}

pub struct PlanningTaskTrackerTool {
    state: PlanningWorkflowState,
}

pub(crate) fn planning_task_tracker_parameter_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["create", "update", "list", "add"],
                "description": "Action to perform on the plan-scoped tracker."
            },
            "title": {
                "type": "string",
                "description": "Title for tracker document (used with create)."
            },
            "items": task_items_array_schema(&format!("{TASK_ITEMS_DESCRIPTION} Leading 2-space indentation in description indicates nesting.")),
            "index_path": {
                "type": "string",
                "pattern": "^[1-9][0-9]*(\\.[1-9][0-9]*)*$",
                "description": "Action=update only: positive flat or hierarchical item path (examples: '2' or '2.1'). Zero is not a valid planning index."
            },
            "index": {
                "type": "integer",
                "minimum": 1,
                "description": "Action=update only: positive top-level item index compatibility fallback. Planning index 0 is invalid; use items for bulk updates."
            },
            "status": task_status_property_schema("New status for update."),
            "description": {
                "type": "string",
                "description": "Task description for add. Optional prefix like '[x] ' or '[~] ' is supported."
            },
            "files": task_files_property_schema("Optional file paths associated with a single add/update item."),
            "outcome": {
                "type": "string",
                "description": "Optional expected outcome associated with a single add/update item."
            },
            "verify": task_verify_property_schema("Optional verification command or commands associated with a single add/update item."),
            "parent_index_path": {
                "type": "string",
                "description": "Optional parent path for add (example: '2'). If omitted, adds top-level task."
            },
            "notes": {
                "type": "string",
                "description": "Optional notes to append."
            }
        },
        "required": ["action"],
        "allOf": tracker_schema_allof(false)
    })
}

impl PlanningTaskTrackerTool {
    pub fn new(state: PlanningWorkflowState) -> Self {
        Self { state }
    }

    fn display_path(&self, path: &Path) -> String {
        self.state
            .workspace_root()
            .map(|workspace| workspace_relative_display(&workspace, path))
            .unwrap_or_else(|| path.to_string_lossy().into_owned())
    }

    fn tracker_file_for_plan(plan_file: &Path) -> Result<PathBuf> {
        tracker_file_for_plan_file(plan_file).context("Active plan file is missing a valid file stem")
    }

    async fn active_plan_file(&self) -> Result<PathBuf> {
        if !self.state.is_active() {
            bail!("task_tracker planning storage is only available while planning");
        }
        self.state
            .get_plan_file()
            .await
            .context("No active plan file. Call start_planning first.")
    }

    async fn tracker_file(&self) -> Result<PathBuf> {
        let plan_file = self.active_plan_file().await?;
        Self::tracker_file_for_plan(&plan_file)
    }

    async fn load_document(&self) -> Result<Option<PlanTaskDocument>> {
        let tracker_file = self.tracker_file().await?;
        if !tokio::fs::try_exists(&tracker_file).await.unwrap_or(false) {
            return Ok(None);
        }
        let content = read_file_with_context(&tracker_file, "plan task tracker file").await?;
        Ok(parse_document_from_markdown(&content))
    }

    fn global_task_file(&self) -> Option<PathBuf> {
        self.state
            .workspace_root()
            .map(|workspace| workspace.join(".vtcode").join("tasks").join("current_task.md"))
    }

    fn global_task_markdown(document: &PlanTaskDocument) -> String {
        let mut lines = Vec::new();
        flatten_for_global_items(&document.items, 0, &mut lines);

        let mut markdown = format!("# {}\n\n", document.title);
        for line in lines {
            let _ =
                writeln!(markdown, "- {} {}{}", line.status.flat_checkbox(), "  ".repeat(line.level), line.description);
            append_task_step_metadata(&mut markdown, "", &line.metadata);
        }
        append_notes_section(&mut markdown, document.notes.as_deref());

        markdown
    }

    fn success_payload(
        &self,
        status: &str,
        message: String,
        tracker_file: &Path,
        document: &PlanTaskDocument,
    ) -> Value {
        json!({
            "status": status,
            "message": message,
            "tracker_file": self.display_path(tracker_file),
            "checklist": document.summary_json(),
            "view": document.view_json(),
        })
    }

    async fn persist_document_and_payload(
        &self,
        status: &str,
        message: String,
        document: &PlanTaskDocument,
    ) -> Result<Value> {
        let tracker_file = self.tracker_file().await?;
        let markdown = document.to_markdown();
        let mut artifacts =
            vec![TrackerArtifact::new(tracker_file.clone(), markdown.clone(), "plan task tracker file").await?];
        if let Some(task_file) = self.global_task_file() {
            artifacts
                .push(TrackerArtifact::new(task_file, Self::global_task_markdown(document), "task checklist").await?);
        }
        if let Some(plan_file) = self.state.get_plan_file().await
            && let Some(artifact) = TrackerArtifact::embedded_plan(plan_file, &markdown).await?
        {
            artifacts.push(artifact);
        }
        persist_tracker_artifacts(artifacts).await?;
        Ok(self.success_payload(status, message, &tracker_file, document))
    }

    async fn handle_create(&self, args: &PlanningTaskTrackerArgs) -> Result<Value> {
        let items = args.items.as_deref().unwrap_or(&[]);
        if items.is_empty() {
            bail!("At least one item is required for 'create'. Provide items: [\"step 1\", \"step 2\", ...]");
        }

        let flat_lines = build_flat_task_lines(items, &[])?;
        if flat_lines.is_empty() {
            bail!("No valid task items were provided for create");
        }

        let mut document = PlanTaskDocument {
            title: args.title.clone().unwrap_or_else(|| "Updated Plan".to_string()),
            items: build_tree_from_flat(&flat_lines),
            notes: None,
        };
        document.notes = append_notes(document.notes.take(), args.notes.as_deref());

        self.persist_document_and_payload("created", "Plan task tracker created successfully.".to_string(), &document)
            .await
    }

    async fn handle_update(&self, args: &PlanningTaskTrackerArgs) -> Result<Value> {
        validate_update_shape(args.items.as_deref(), args.index, args.index_path.as_deref(), args.status.as_deref())?;
        let is_bulk_update =
            is_bulk_sync_update(args.items.as_deref(), args.index, args.index_path.as_deref(), args.status.as_deref());
        let mut document = self
            .load_document()
            .await?
            .context("No active plan tracker. Use action='create' first.")?;
        let previous = document.clone();

        if is_bulk_update {
            let input_items = args.items.as_deref().unwrap_or(&[]);
            let flat_lines = build_flat_task_lines(input_items, &document.items)?;
            if flat_lines.is_empty() {
                bail!("No valid items provided for checklist sync");
            }
            if let Some(title) = args.title.as_deref() {
                document.title = title.to_string();
            }
            document.items = build_tree_from_flat(&flat_lines);
            document.notes = append_notes(document.notes.take(), args.notes.as_deref());

            return self
                .persist_update(&previous, &document, "Checklist synchronized from provided items.".to_string())
                .await;
        }

        let index_path = args
            .index_path
            .clone()
            .or_else(|| args.index.map(|value| value.to_string()))
            .context(
                "'index_path' is required for 'update' (example: \"2.1\"), or provide 'index' for top-level compatibility",
            )?;
        let path = parse_index_path(&index_path)?;
        let status_str = args
            .status
            .as_deref()
            .context("'status' is required for 'update' (pending|in_progress|completed|blocked)")?;
        let new_status = PlanTaskStatus::from_str(status_str)?;

        let (old_status, new_status_str) = {
            let node = get_node_mut_by_index_path(document.items.as_mut_slice(), &path)
                .with_context(|| format!("No item at index_path '{index_path}'"))?;
            let old_status = node.status.as_str().to_string();
            node.status = new_status;
            apply_metadata_input(
                &mut node.metadata,
                args.files.as_deref(),
                args.outcome.as_deref(),
                args.verify.as_deref(),
            );
            (old_status, node.status.as_str().to_string())
        };

        document.notes = append_notes(document.notes.take(), args.notes.as_deref());

        self.persist_update(
            &previous,
            &document,
            format!("Item {index_path} status changed: {old_status} -> {new_status_str}"),
        )
        .await
    }

    async fn persist_update(
        &self,
        previous: &PlanTaskDocument,
        document: &PlanTaskDocument,
        message: String,
    ) -> Result<Value> {
        let (status, message) = if previous == document {
            ("unchanged", "Checklist already matches requested state.".to_string())
        } else {
            ("updated", message)
        };
        self.persist_document_and_payload(status, message, document).await
    }

    async fn handle_list(&self) -> Result<Value> {
        let tracker_file = self.tracker_file().await?;
        match self.load_document().await? {
            Some(document) => {
                Ok(self.success_payload("ok", "Plan task tracker loaded.".to_string(), &tracker_file, &document))
            }
            None => Ok(json!({
                "status": "empty",
                "message": "No active plan tracker. Use action='create' to start one.",
                "tracker_file": self.display_path(&tracker_file),
            })),
        }
    }

    async fn handle_add(&self, args: &PlanningTaskTrackerArgs) -> Result<Value> {
        let mut document = self
            .load_document()
            .await?
            .context("No active plan tracker. Use action='create' first.")?;

        let description = args.description.as_deref().context("'description' is required for 'add'")?;
        let (status, parsed_description) = parse_status_prefix(description);
        let node = PlanTaskNode {
            description: parsed_description.trim().to_string(),
            status,
            metadata: metadata_from_input(args.files.as_deref(), args.outcome.as_deref(), args.verify.as_deref()),
            children: Vec::new(),
        };
        if node.description.is_empty() {
            bail!("description cannot be empty");
        }

        if let Some(parent_path_str) = args.parent_index_path.as_deref() {
            let parent_path = parse_index_path(parent_path_str)?;
            let parent = get_node_mut_by_index_path(document.items.as_mut_slice(), &parent_path)
                .with_context(|| format!("No parent item at parent_index_path '{parent_path_str}'"))?;
            parent.children.push(node);
        } else {
            document.items.push(node);
        }

        document.notes = append_notes(document.notes.take(), args.notes.as_deref());

        self.persist_document_and_payload("added", "Plan task added successfully.".to_string(), &document)
            .await
    }
}

#[async_trait]
impl Tool for PlanningTaskTrackerTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let args: PlanningTaskTrackerArgs = deserialize_tool_args(&args, "task_tracker")?;
        validate_action_index_fields(&args.action, args.index, args.index_path.as_deref())?;

        match args.action.as_str() {
            "create" => self.handle_create(&args).await,
            "update" => self.handle_update(&args).await,
            "list" => self.handle_list().await,
            "add" => self.handle_add(&args).await,
            other => Ok(json!({
                "status": "error",
                "message": format!("Unknown action '{}'. Use: create, update, list, add", other),
            })),
        }
    }

    fn name(&self) -> &str {
        tools::TASK_TRACKER
    }

    fn description(&self) -> &str {
        super::task_tracker::task_tracker_description_for_workflow(true)
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(planning_task_tracker_parameter_schema())
    }

    fn is_mutating(&self) -> bool {
        false
    }

    fn is_parallel_safe(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup_planning_workflow() -> (TempDir, PlanningWorkflowState, PlanningTaskTrackerTool) {
        let temp_dir = TempDir::new().expect("temp dir");
        let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
        let plans_dir = state.plans_dir();
        std::fs::create_dir_all(&plans_dir).expect("create plans dir");
        let plan_file = plans_dir.join("test-plan.md");
        std::fs::write(&plan_file, "# Test Plan\n").expect("write plan");
        state.set_plan_file(Some(plan_file)).await;
        state.enable();

        let tool = PlanningTaskTrackerTool::new(state.clone());
        (temp_dir, state, tool)
    }

    #[tokio::test]
    async fn create_and_list_tracker_with_hierarchy() {
        let (_temp_dir, _state, tool) = setup_planning_workflow().await;

        let created = tool
            .execute(json!({
                "action": "create",
                "title": "Updated Plan",
                "items": [
                    "Add config cap",
                    "  Use cap in guard logic",
                    "[~] Expose setting in template"
                ]
            }))
            .await
            .expect("create tracker");

        assert_eq!(created["status"], "created");
        assert_eq!(created["checklist"]["total"], 3);
        assert_eq!(created["checklist"]["in_progress"], 1);
        assert_eq!(created["view"]["title"], "Updated Plan");
        assert_eq!(created["tracker_file"], ".vtcode/plans/test-plan.tasks.md");

        let lines = created["view"]["lines"].as_array().expect("view lines array");
        assert!(!lines.is_empty());
        let first = lines[0]["display"].as_str().unwrap_or_default();
        assert!(first.contains('└') || first.contains('├'));
    }

    #[tokio::test]
    async fn create_accepts_metadata_and_verify_string_forms() {
        let (_temp_dir, _state, tool) = setup_planning_workflow().await;

        let created = tool
            .execute(json!({
                "action": "create",
                "title": "Harness plan",
                "items": [
                    {
                        "description": "Analyze",
                        "files": ["docs/ARCHITECTURE.md"],
                        "outcome": "Map the harness",
                        "verify": "cargo check"
                    },
                    {
                        "description": "Implement",
                        "verify": ["cargo test -p vtcode-core task_tracker", "cargo check -p vtcode"]
                    }
                ]
            }))
            .await
            .expect("create tracker");

        assert_eq!(created["checklist"]["items"][0]["files"], json!(["docs/ARCHITECTURE.md"]));
        assert_eq!(created["checklist"]["items"][0]["outcome"], "Map the harness");
        assert_eq!(created["checklist"]["items"][0]["verify"], json!(["cargo check"]));
        assert_eq!(
            created["checklist"]["items"][1]["verify"],
            json!(["cargo test -p vtcode-core task_tracker", "cargo check -p vtcode"])
        );
    }

    #[tokio::test]
    async fn add_and_update_nested_item() {
        let (_temp_dir, _state, tool) = setup_planning_workflow().await;

        tool.execute(json!({
            "action": "create",
            "items": ["Parent task"]
        }))
        .await
        .expect("create tracker");

        tool.execute(json!({
            "action": "add",
            "parent_index_path": "1",
            "description": "Child task"
        }))
        .await
        .expect("add nested task");

        let updated = tool
            .execute(json!({
                "action": "update",
                "index_path": "1.1",
                "status": "completed"
            }))
            .await
            .expect("update nested task");

        assert_eq!(updated["status"], "updated");
        assert_eq!(updated["checklist"]["completed"], 1);
    }

    #[tokio::test]
    async fn persistence_across_instances() {
        let (_temp_dir, state, tool) = setup_planning_workflow().await;

        tool.execute(json!({
            "action": "create",
            "items": ["Persisted step"]
        }))
        .await
        .expect("create tracker");

        tool.execute(json!({
            "action": "update",
            "index_path": "1",
            "status": "completed"
        }))
        .await
        .expect("update tracker");

        let tool2 = PlanningTaskTrackerTool::new(state);
        let listed = tool2.execute(json!({"action": "list"})).await.expect("list tracker");

        assert_eq!(listed["status"], "ok");
        assert_eq!(listed["checklist"]["completed"], 1);
    }

    #[tokio::test]
    async fn update_supports_bulk_item_sync_and_global_mirror() {
        let (temp_dir, _state, tool) = setup_planning_workflow().await;

        tool.execute(json!({
            "action": "create",
            "items": ["Step 1", "Step 2"]
        }))
        .await
        .expect("create tracker");

        let updated = tool
            .execute(json!({
                "action": "update",
                "items": ["[x] Step 1", "[~] Step 2", "[ ] Step 3"]
            }))
            .await
            .expect("bulk update");

        assert_eq!(updated["status"], "updated");
        assert_eq!(updated["checklist"]["completed"], 1);
        assert_eq!(updated["checklist"]["in_progress"], 1);
        assert_eq!(updated["checklist"]["pending"], 1);

        let mirrored = temp_dir.path().join(".vtcode").join("tasks").join("current_task.md");
        let mirrored_content = std::fs::read_to_string(mirrored).expect("read mirrored checklist");
        assert!(mirrored_content.contains("Step 3"));
    }

    #[tokio::test]
    async fn update_accepts_flat_index_fallback() {
        let (_temp_dir, _state, tool) = setup_planning_workflow().await;

        tool.execute(json!({
            "action": "create",
            "items": ["Parent task"]
        }))
        .await
        .expect("create tracker");

        let updated = tool
            .execute(json!({
                "action": "update",
                "index": 1,
                "status": "completed"
            }))
            .await
            .expect("flat-index update");

        assert_eq!(updated["status"], "updated");
        assert_eq!(updated["checklist"]["completed"], 1);
    }

    #[tokio::test]
    async fn update_rejects_zero_flat_index_even_for_checklist_completion() {
        let (_temp_dir, _state, tool) = setup_planning_workflow().await;

        tool.execute(json!({
            "action": "create",
            "items": ["Parent task"]
        }))
        .await
        .expect("create tracker");

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
    async fn planning_task_tracker_schema_requires_positive_indices() {
        let (_temp_dir, _state, tool) = setup_planning_workflow().await;
        let schema = tool.parameter_schema().expect("planning task tracker schema");

        assert_eq!(schema["properties"]["index"]["minimum"], 1);
        let items_description = schema["properties"]["items"]["description"].as_str().unwrap();
        assert!(items_description.contains("Full checklist replacement"));
        assert!(items_description.contains("never JSON-encoded strings"));
        assert!(items_description.contains("2-space indentation"));
        assert_eq!(schema["properties"]["index_path"]["pattern"], "^[1-9][0-9]*(\\.[1-9][0-9]*)*$");
        assert!(
            schema["properties"]["index_path"]["description"]
                .as_str()
                .expect("index path description")
                .contains("positive")
        );
    }

    #[tokio::test]
    async fn planning_task_tracker_schema_rejects_mixed_bulk_and_single_updates() {
        let (_temp_dir, _state, tool) = setup_planning_workflow().await;
        let schema = tool.parameter_schema().expect("planning task tracker schema");
        let invalid_cases = [
            json!({"action": "list", "index": 1}),
            json!({"action": "list", "index_path": "1"}),
            json!({"action": "create", "index": 1, "items": ["New item"]}),
            json!({"action": "add", "index_path": "1", "description": "New item"}),
            json!({"action": "update", "items": ["Done"], "index": 1, "status": "completed"}),
            json!({"action": "update", "items": ["Done"], "index_path": "1.1", "status": "completed"}),
            json!({"action": "update", "index": 1, "index_path": "1.1", "status": "completed"}),
            json!({"action": "update", "items": ["Done"], "status": "completed"}),
        ];

        for args in invalid_cases {
            assert!(jsonschema::validate(&schema, &args).is_err(), "expected invalid args: {args}");
        }
    }

    #[tokio::test]
    async fn bulk_update_rejects_encoded_commands_without_resetting_completed_steps() {
        let (_temp_dir, _state, tool) = setup_planning_workflow().await;
        tool.execute(json!({
            "action": "create",
            "title": "Preserve progress",
            "items": [
                {"description": "Update introduction", "status": "completed", "files": ["README.md"], "outcome": "Clear overview", "verify": ["review headings"]},
                {"description": "Review links", "status": "blocked", "outcome": "Await network"}
            ]
        })).await.unwrap();
        let tracker_path = _temp_dir.path().join(".vtcode/tasks/current_task.md");
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
    async fn planning_update_rejects_mixed_bulk_and_single_fields_before_mutation() {
        let (_temp_dir, _state, tool) = setup_planning_workflow().await;

        tool.execute(json!({
            "action": "create",
            "items": ["Original"]
        }))
        .await
        .expect("create tracker");

        let error = tool
            .execute(json!({
                "action": "update",
                "items": ["Replacement"],
                "index_path": "1",
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
    async fn planning_update_rejects_both_index_forms_before_mutation() {
        let (_temp_dir, _state, tool) = setup_planning_workflow().await;

        tool.execute(json!({
            "action": "create",
            "items": ["Original"]
        }))
        .await
        .expect("create tracker");

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
        let (_temp_dir, _state, tool) = setup_planning_workflow().await;

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

    #[tokio::test]
    async fn rejects_when_planning_workflow_is_inactive() {
        let temp_dir = TempDir::new().expect("temp dir");
        let state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
        let tool = PlanningTaskTrackerTool::new(state);

        let err = tool
            .execute(json!({"action": "list"}))
            .await
            .expect_err("should fail outside planning workflow");

        assert!(err.to_string().contains("only available while planning"));
    }
}
