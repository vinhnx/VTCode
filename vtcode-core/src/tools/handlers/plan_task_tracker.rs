//! Plan-mode scoped task tracker persisted next to the active plan file.
//!
//! This tracker is intended for Plan Mode only and writes a sidecar markdown
//! file next to the active plan file (`<plan>.tasks.md`).

use super::plan_mode::PlanModeState;
use crate::config::constants::tools;
use crate::tools::handlers::task_tracking::{
    TaskCounts, TaskItemInput, TaskStepMetadata, TaskTrackingStatus, append_notes,
    append_notes_section, append_task_step_metadata, is_bulk_sync_update, metadata_from_input,
    normalize_optional_text, normalize_string_items, parse_marked_status_prefix,
    parse_status_prefix,
};
use crate::tools::traits::Tool;
use crate::utils::file_utils::{
    ensure_dir_exists, read_file_with_context, write_file_with_context,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::str::FromStr;

type PlanTaskStatus = TaskTrackingStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlanTaskNode {
    description: String,
    status: PlanTaskStatus,
    #[serde(default, flatten)]
    metadata: TaskStepMetadata,
    children: Vec<PlanTaskNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlanTaskDocument {
    title: String,
    items: Vec<PlanTaskNode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTaskTrackerArgs {
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
        let mut lines = Vec::new();
        build_view_lines(&self.items, "", "", &mut lines);

        json!({
            "title": self.title,
            "lines": lines,
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
        out.push_str(&format!(
            "{}- {} {}\n",
            indent,
            node.status.plan_checkbox(),
            node.description
        ));
        append_task_step_metadata(out, &indent, &node.metadata);
        write_markdown_nodes(&node.children, level + 1, out);
    }
}

fn flatten_items_json(nodes: &[PlanTaskNode]) -> Vec<Value> {
    let mut items = Vec::new();
    flatten_items_json_inner(nodes, "", 0, &mut items);
    items
}

fn flatten_for_global_items(
    nodes: &[PlanTaskNode],
    level: usize,
    out: &mut Vec<(PlanTaskStatus, String, TaskStepMetadata)>,
) {
    for node in nodes {
        out.push((
            node.status.clone(),
            format!("{}{}", "  ".repeat(level), node.description),
            node.metadata.clone(),
        ));
        flatten_for_global_items(&node.children, level + 1, out);
    }
}

fn flatten_items_json_inner(
    nodes: &[PlanTaskNode],
    index_prefix: &str,
    level: usize,
    out: &mut Vec<Value>,
) {
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

fn build_view_lines(
    nodes: &[PlanTaskNode],
    tree_prefix: &str,
    index_prefix: &str,
    out: &mut Vec<Value>,
) {
    for (idx, node) in nodes.iter().enumerate() {
        let is_last = idx + 1 == nodes.len();
        let branch = if is_last { "└" } else { "├" };
        let next_prefix = if is_last {
            format!("{tree_prefix}  ")
        } else {
            format!("{tree_prefix}│ ")
        };
        let index_path = if index_prefix.is_empty() {
            format!("{}", idx + 1)
        } else {
            format!("{index_prefix}.{}", idx + 1)
        };
        let display = format!(
            "{tree_prefix}{branch} {} {}",
            node.status.view_symbol(),
            node.description
        );

        out.push(json!({
            "display": display,
            "index_path": index_path,
            "status": node.status.as_str(),
            "text": node.description,
            "files": node.metadata.files.clone(),
            "outcome": node.metadata.outcome.clone(),
            "verify": node.metadata.verify.clone(),
        }));
        if !node.metadata.files.is_empty() {
            out.push(json!({
                "display": format!("{next_prefix}files: {}", node.metadata.files.join(", ")),
                "status": node.status.as_str(),
                "text": format!("files: {}", node.metadata.files.join(", ")),
            }));
        }
        if let Some(outcome) = node.metadata.outcome.as_deref() {
            out.push(json!({
                "display": format!("{next_prefix}outcome: {}", outcome),
                "status": node.status.as_str(),
                "text": format!("outcome: {}", outcome),
            }));
        }
        for command in &node.metadata.verify {
            out.push(json!({
                "display": format!("{next_prefix}verify: {}", command),
                "status": node.status.as_str(),
                "text": format!("verify: {}", command),
            }));
        }
        build_view_lines(&node.children, &next_prefix, &index_path, out);
    }
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

fn parse_files_metadata(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn apply_flat_line_metadata(
    line: &mut FlatTaskLine,
    raw: &str,
    in_verify_block: &mut bool,
) -> bool {
    let trimmed = raw.trim_start();

    if *in_verify_block {
        if let Some(command) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            if let Some(command) = normalize_optional_text(Some(command)) {
                line.metadata.verify.push(command);
            }
            return true;
        }
        *in_verify_block = false;
    }

    if let Some(rest) = trimmed.strip_prefix("files:") {
        line.metadata.files = parse_files_metadata(rest);
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("outcome:") {
        line.metadata.outcome = normalize_optional_text(Some(rest));
        return true;
    }
    if trimmed == "verify:" {
        line.metadata.verify.clear();
        *in_verify_block = true;
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("verify:") {
        line.metadata.verify = normalize_string_items(Some(&[rest.to_string()]));
        return true;
    }

    false
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
            status: line.status.clone(),
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

fn get_node_mut_by_indices<'a>(
    nodes: &'a mut [PlanTaskNode],
    path: &[usize],
) -> Option<&'a mut PlanTaskNode> {
    let (&head, tail) = path.split_first()?;
    let node = nodes.get_mut(head)?;
    if tail.is_empty() {
        Some(node)
    } else {
        get_node_mut_by_indices(node.children.as_mut_slice(), tail)
    }
}

fn get_node_mut_by_index_path<'a>(
    nodes: &'a mut [PlanTaskNode],
    path: &[usize],
) -> Option<&'a mut PlanTaskNode> {
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

    let mut out = Vec::new();
    for token in trimmed.split('.') {
        let parsed = token
            .parse::<usize>()
            .with_context(|| format!("Invalid index component '{}'", token))?;
        if parsed == 0 {
            bail!("index_path components must be >= 1");
        }
        out.push(parsed);
    }
    Ok(out)
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
            in_plan_section = matches!(
                lowered.as_str(),
                "plan of work" | "concrete steps" | "updated plan"
            ) || lowered.starts_with("phase ");
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
                if leading_spaces >= min_indent
                    && apply_flat_line_metadata(last, raw, &mut in_verify_block)
                {
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

    Some(PlanTaskDocument {
        title,
        items,
        notes,
    })
}

fn build_flat_create_lines(items: &[TaskItemInput]) -> Result<Vec<FlatTaskLine>> {
    items
        .iter()
        .filter_map(|raw| match raw {
            TaskItemInput::Text(raw) => {
                let level = raw.chars().take_while(|c| *c == ' ').count() / 2;
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return None;
                }
                let (status, description) = parse_status_prefix(trimmed);
                if description.trim().is_empty() {
                    return None;
                }
                Some(Ok(FlatTaskLine {
                    level,
                    status,
                    description: description.trim().to_string(),
                    metadata: TaskStepMetadata::default(),
                }))
            }
            TaskItemInput::Structured(payload) => {
                let level = payload
                    .description
                    .chars()
                    .take_while(|c| *c == ' ')
                    .count()
                    / 2;
                let (parsed_status, description) = parse_status_prefix(payload.description.trim());
                let description = description.trim().to_string();
                if description.is_empty() {
                    return None;
                }
                let status = match payload.status.as_deref() {
                    Some(value) => match PlanTaskStatus::from_str(value) {
                        Ok(status) => status,
                        Err(err) => return Some(Err(err)),
                    },
                    None => parsed_status,
                };
                Some(Ok(FlatTaskLine {
                    level,
                    status,
                    description,
                    metadata: metadata_from_input(
                        payload.files.as_deref(),
                        payload.outcome.as_deref(),
                        payload.verify.as_deref(),
                    ),
                }))
            }
        })
        .collect()
}

pub struct PlanTaskTrackerTool {
    state: PlanModeState,
}

impl PlanTaskTrackerTool {
    pub fn new(state: PlanModeState) -> Self {
        Self { state }
    }

    fn tracker_file_for_plan(plan_file: &Path) -> Result<PathBuf> {
        let stem = plan_file
            .file_stem()
            .and_then(|s| s.to_str())
            .context("Active plan file is missing a valid file stem")?;
        Ok(plan_file.with_file_name(format!("{stem}.tasks.md")))
    }

    async fn active_plan_file(&self) -> Result<PathBuf> {
        if !self.state.is_active() {
            bail!("plan_task_tracker is only available in Plan Mode");
        }
        self.state
            .get_plan_file()
            .await
            .context("No active plan file. Call enter_plan_mode first.")
    }

    async fn tracker_file(&self) -> Result<PathBuf> {
        let plan_file = self.active_plan_file().await?;
        Self::tracker_file_for_plan(&plan_file)
    }

    async fn load_document(&self) -> Result<Option<PlanTaskDocument>> {
        let tracker_file = self.tracker_file().await?;
        if !tracker_file.exists() {
            return Ok(None);
        }
        let content = read_file_with_context(&tracker_file, "plan task tracker file").await?;
        Ok(parse_document_from_markdown(&content))
    }

    async fn save_document(&self, document: &PlanTaskDocument) -> Result<PathBuf> {
        let tracker_file = self.tracker_file().await?;
        if let Some(parent) = tracker_file.parent() {
            ensure_dir_exists(parent).await.with_context(|| {
                format!("Failed to create plans directory: {}", parent.display())
            })?;
        }
        write_file_with_context(
            &tracker_file,
            &document.to_markdown(),
            "plan task tracker file",
        )
        .await
        .with_context(|| {
            format!(
                "Failed to write plan task tracker file: {}",
                tracker_file.display()
            )
        })?;
        Ok(tracker_file)
    }

    fn global_task_file(&self) -> Option<PathBuf> {
        self.state.workspace_root().map(|workspace| {
            workspace
                .join(".vtcode")
                .join("tasks")
                .join("current_task.md")
        })
    }

    async fn mirror_global_task_file(&self, document: &PlanTaskDocument) -> Result<()> {
        let Some(task_file) = self.global_task_file() else {
            return Ok(());
        };

        if let Some(parent) = task_file.parent() {
            ensure_dir_exists(parent).await.with_context(|| {
                format!("Failed to create tasks directory: {}", parent.display())
            })?;
        }

        let mut lines = Vec::new();
        flatten_for_global_items(&document.items, 0, &mut lines);

        let mut markdown = format!("# {}\n\n", document.title);
        for (status, description, metadata) in lines {
            markdown.push_str(&format!("- {} {}\n", status.flat_checkbox(), description));
            append_task_step_metadata(&mut markdown, "", &metadata);
        }
        append_notes_section(&mut markdown, document.notes.as_deref());

        write_file_with_context(&task_file, &markdown, "task checklist")
            .await
            .with_context(|| {
                format!(
                    "Failed to write mirrored task checklist file: {}",
                    task_file.display()
                )
            })?;
        Ok(())
    }

    fn success_payload(
        status: &str,
        message: String,
        tracker_file: &Path,
        document: &PlanTaskDocument,
    ) -> Value {
        json!({
            "status": status,
            "message": message,
            "tracker_file": tracker_file.display().to_string(),
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
        let tracker_file = self.save_document(document).await?;
        self.mirror_global_task_file(document).await?;
        Ok(Self::success_payload(
            status,
            message,
            &tracker_file,
            document,
        ))
    }

    async fn handle_create(&self, args: &PlanTaskTrackerArgs) -> Result<Value> {
        let items = args.items.as_deref().unwrap_or(&[]);
        if items.is_empty() {
            bail!(
                "At least one item is required for 'create'. Provide items: [\"step 1\", \"step 2\", ...]"
            );
        }

        let flat_lines = build_flat_create_lines(items)?;
        if flat_lines.is_empty() {
            bail!("No valid task items were provided for create");
        }

        let mut document = PlanTaskDocument {
            title: args
                .title
                .clone()
                .unwrap_or_else(|| "Updated Plan".to_string()),
            items: build_tree_from_flat(&flat_lines),
            notes: None,
        };
        document.notes = append_notes(document.notes.take(), args.notes.as_deref());

        self.persist_document_and_payload(
            "created",
            "Plan task tracker created successfully.".to_string(),
            &document,
        )
        .await
    }

    async fn handle_update(&self, args: &PlanTaskTrackerArgs) -> Result<Value> {
        let mut document = self
            .load_document()
            .await?
            .context("No active plan tracker. Use action='create' first.")?;

        if is_bulk_sync_update(
            args.items.as_deref(),
            args.index,
            args.index_path.as_deref(),
            args.status.as_deref(),
        ) {
            let input_items = args.items.as_deref().unwrap_or(&[]);
            let flat_lines = build_flat_create_lines(input_items)?;
            if flat_lines.is_empty() {
                bail!("No valid items provided for checklist sync");
            }
            if let Some(title) = args.title.as_deref() {
                document.title = title.to_string();
            }
            document.items = build_tree_from_flat(&flat_lines);
            document.notes = append_notes(document.notes.take(), args.notes.as_deref());

            return self
                .persist_document_and_payload(
                    "updated",
                    "Checklist synchronized from provided items.".to_string(),
                    &document,
                )
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
                .with_context(|| format!("No item at index_path '{}'", index_path))?;
            let old_status = node.status.as_str().to_string();
            node.status = new_status;
            if let Some(files) = args.files.as_deref() {
                node.metadata.files = normalize_string_items(Some(files));
            }
            if args.outcome.is_some() {
                node.metadata.outcome = normalize_optional_text(args.outcome.as_deref());
            }
            if let Some(verify) = args.verify.as_deref() {
                node.metadata.verify = normalize_string_items(Some(verify));
            }
            (old_status, node.status.as_str().to_string())
        };

        document.notes = append_notes(document.notes.take(), args.notes.as_deref());

        self.persist_document_and_payload(
            "updated",
            format!(
                "Item {} status changed: {} -> {}",
                index_path, old_status, new_status_str
            ),
            &document,
        )
        .await
    }

    async fn handle_list(&self) -> Result<Value> {
        let tracker_file = self.tracker_file().await?;
        match self.load_document().await? {
            Some(document) => Ok(Self::success_payload(
                "ok",
                "Plan task tracker loaded.".to_string(),
                &tracker_file,
                &document,
            )),
            None => Ok(json!({
                "status": "empty",
                "message": "No active plan tracker. Use action='create' to start one.",
                "tracker_file": tracker_file.display().to_string(),
            })),
        }
    }

    async fn handle_add(&self, args: &PlanTaskTrackerArgs) -> Result<Value> {
        let mut document = self
            .load_document()
            .await?
            .context("No active plan tracker. Use action='create' first.")?;

        let description = args
            .description
            .as_deref()
            .context("'description' is required for 'add'")?;
        let (status, parsed_description) = parse_status_prefix(description);
        let node = PlanTaskNode {
            description: parsed_description.trim().to_string(),
            status,
            metadata: metadata_from_input(
                args.files.as_deref(),
                args.outcome.as_deref(),
                args.verify.as_deref(),
            ),
            children: Vec::new(),
        };
        if node.description.is_empty() {
            bail!("description cannot be empty");
        }

        if let Some(parent_path_str) = args.parent_index_path.as_deref() {
            let parent_path = parse_index_path(parent_path_str)?;
            let parent = get_node_mut_by_index_path(document.items.as_mut_slice(), &parent_path)
                .with_context(|| {
                    format!("No parent item at parent_index_path '{}'", parent_path_str)
                })?;
            parent.children.push(node);
        } else {
            document.items.push(node);
        }

        document.notes = append_notes(document.notes.take(), args.notes.as_deref());

        self.persist_document_and_payload(
            "added",
            "Plan task added successfully.".to_string(),
            &document,
        )
        .await
    }
}

#[async_trait]
impl Tool for PlanTaskTrackerTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let args: PlanTaskTrackerArgs = serde_json::from_value(args).context(
            "Invalid plan_task_tracker arguments. Required: {\"action\": \"create|update|list|add\", ...}",
        )?;

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

    fn name(&self) -> &'static str {
        tools::PLAN_TASK_TRACKER
    }

    fn description(&self) -> &'static str {
        "Plan-mode compatibility alias for adaptive task tracking. Persists hierarchical plan progress under .vtcode/plans/<plan>.tasks.md and mirrors updates to .vtcode/tasks/current_task.md. Actions: create, update, list, add."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(json!({
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
                    "description": "Initial task items (used with create). Leading 2-space indentation in description indicates nesting."
                },
                "index_path": {
                    "type": "string",
                    "description": "Hierarchical index path for update (example: '2.1')."
                },
                "index": {
                    "type": "integer",
                    "description": "Top-level index compatibility fallback for update."
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "blocked"],
                    "description": "New status for update."
                },
                "description": {
                    "type": "string",
                    "description": "Task description for add. Optional prefix like '[x] ' or '[~] ' is supported."
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
                    "description": "Optional parent path for add (example: '2'). If omitted, adds top-level task."
                },
                "notes": {
                    "type": "string",
                    "description": "Optional notes to append."
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
                            { "required": ["index_path", "status"] },
                            { "required": ["index", "status"] },
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

    async fn setup_plan_mode() -> (TempDir, PlanModeState, PlanTaskTrackerTool) {
        let temp_dir = TempDir::new().expect("temp dir");
        let state = PlanModeState::new(temp_dir.path().to_path_buf());
        let plans_dir = state.plans_dir();
        std::fs::create_dir_all(&plans_dir).expect("create plans dir");
        let plan_file = plans_dir.join("test-plan.md");
        std::fs::write(&plan_file, "# Test Plan\n").expect("write plan");
        state.set_plan_file(Some(plan_file)).await;
        state.enable();

        let tool = PlanTaskTrackerTool::new(state.clone());
        (temp_dir, state, tool)
    }

    #[tokio::test]
    async fn create_and_list_tracker_with_hierarchy() {
        let (_temp_dir, _state, tool) = setup_plan_mode().await;

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

        let lines = created["view"]["lines"]
            .as_array()
            .expect("view lines array");
        assert!(!lines.is_empty());
        let first = lines[0]["display"].as_str().unwrap_or_default();
        assert!(first.contains('└') || first.contains('├'));
    }

    #[tokio::test]
    async fn create_accepts_metadata_and_verify_string_forms() {
        let (_temp_dir, _state, tool) = setup_plan_mode().await;

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

        assert_eq!(
            created["checklist"]["items"][0]["files"],
            json!(["docs/ARCHITECTURE.md"])
        );
        assert_eq!(
            created["checklist"]["items"][0]["outcome"],
            "Map the harness"
        );
        assert_eq!(
            created["checklist"]["items"][0]["verify"],
            json!(["cargo check"])
        );
        assert_eq!(
            created["checklist"]["items"][1]["verify"],
            json!([
                "cargo test -p vtcode-core task_tracker",
                "cargo check -p vtcode"
            ])
        );
    }

    #[tokio::test]
    async fn add_and_update_nested_item() {
        let (_temp_dir, _state, tool) = setup_plan_mode().await;

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
        let (_temp_dir, state, tool) = setup_plan_mode().await;

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

        let tool2 = PlanTaskTrackerTool::new(state);
        let listed = tool2
            .execute(json!({"action": "list"}))
            .await
            .expect("list tracker");

        assert_eq!(listed["status"], "ok");
        assert_eq!(listed["checklist"]["completed"], 1);
    }

    #[tokio::test]
    async fn update_supports_bulk_item_sync_and_global_mirror() {
        let (temp_dir, _state, tool) = setup_plan_mode().await;

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

        let mirrored = temp_dir
            .path()
            .join(".vtcode")
            .join("tasks")
            .join("current_task.md");
        let mirrored_content = std::fs::read_to_string(mirrored).expect("read mirrored checklist");
        assert!(mirrored_content.contains("Step 3"));
    }

    #[tokio::test]
    async fn update_accepts_flat_index_fallback() {
        let (_temp_dir, _state, tool) = setup_plan_mode().await;

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
    async fn rejects_when_plan_mode_is_inactive() {
        let temp_dir = TempDir::new().expect("temp dir");
        let state = PlanModeState::new(temp_dir.path().to_path_buf());
        let tool = PlanTaskTrackerTool::new(state);

        let err = tool
            .execute(json!({"action": "list"}))
            .await
            .expect_err("should fail outside plan mode");

        assert!(err.to_string().contains("only available in Plan Mode"));
    }
}
