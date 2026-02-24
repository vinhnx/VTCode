//! Plan-mode scoped task tracker persisted under `.vtcode/plans/`.
//!
//! This tracker is intended for Plan Mode only and writes a sidecar markdown
//! file next to the active plan file (`<plan>.tasks.md`).

use super::plan_mode::PlanModeState;
use crate::config::constants::tools;
use crate::tools::traits::Tool;
use crate::utils::file_utils::{
    ensure_dir_exists, read_file_with_context, write_file_with_context,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PlanTaskStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
}

impl PlanTaskStatus {
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "blocked" => Ok(Self::Blocked),
            other => bail!(
                "Invalid status '{}'. Use: pending, in_progress, completed, blocked",
                other
            ),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
        }
    }

    fn markdown_marker(&self) -> &'static str {
        match self {
            Self::Pending => "[ ]",
            Self::InProgress => "[~]",
            Self::Completed => "[x]",
            Self::Blocked => "[!]",
        }
    }

    fn view_symbol(&self) -> &'static str {
        match self {
            Self::Pending => "•",
            Self::InProgress => ">",
            Self::Completed => "✔",
            Self::Blocked => "!",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlanTaskNode {
    description: String,
    status: PlanTaskStatus,
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
    pub items: Option<Vec<String>>,

    /// Hierarchical index path (example: "2.1")
    #[serde(default)]
    pub index_path: Option<String>,

    /// New status for update
    #[serde(default)]
    pub status: Option<String>,

    /// Description for add
    #[serde(default)]
    pub description: Option<String>,

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
}

#[derive(Default)]
struct TaskCounts {
    total: usize,
    completed: usize,
    in_progress: usize,
    pending: usize,
    blocked: usize,
}

impl PlanTaskDocument {
    fn to_markdown(&self) -> String {
        let mut out = format!("# {}\n\n## Plan of Work\n\n", self.title);
        write_markdown_nodes(&self.items, 0, &mut out);
        if let Some(notes) = self.notes.as_deref() {
            if !notes.trim().is_empty() {
                out.push_str("\n## Notes\n\n");
                out.push_str(notes.trim());
                out.push('\n');
            }
        }
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
            "progress_percent": if counts.total > 0 {
                (counts.completed as f64 / counts.total as f64 * 100.0).round() as usize
            } else {
                0
            },
            "items": flatten_items_json(&self.items),
        })
    }

    fn view_json(&self) -> Value {
        let mut lines = Vec::new();
        build_view_lines(&self.items, "", "", &mut lines);

        json!({
            "title": "Updated Plan",
            "lines": lines,
        })
    }
}

fn count_nodes(nodes: &[PlanTaskNode], counts: &mut TaskCounts) {
    for node in nodes {
        counts.total += 1;
        match node.status {
            PlanTaskStatus::Pending => counts.pending += 1,
            PlanTaskStatus::InProgress => counts.in_progress += 1,
            PlanTaskStatus::Completed => counts.completed += 1,
            PlanTaskStatus::Blocked => counts.blocked += 1,
        }
        count_nodes(&node.children, counts);
    }
}

fn write_markdown_nodes(nodes: &[PlanTaskNode], level: usize, out: &mut String) {
    let indent = "  ".repeat(level);
    for node in nodes {
        out.push_str(&format!(
            "{}- {} {}\n",
            indent,
            node.status.markdown_marker(),
            node.description
        ));
        write_markdown_nodes(&node.children, level + 1, out);
    }
}

fn flatten_items_json(nodes: &[PlanTaskNode]) -> Vec<Value> {
    let mut items = Vec::new();
    flatten_items_json_inner(nodes, "", 0, &mut items);
    items
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
        }));
        build_view_lines(&node.children, &next_prefix, &index_path, out);
    }
}

fn parse_status_prefix(value: &str) -> (PlanTaskStatus, String) {
    let trimmed = value.trim_start();
    let mapping = [
        ("[x] ", PlanTaskStatus::Completed),
        ("[X] ", PlanTaskStatus::Completed),
        ("[~] ", PlanTaskStatus::InProgress),
        ("[/] ", PlanTaskStatus::InProgress),
        ("[!] ", PlanTaskStatus::Blocked),
        ("[ ] ", PlanTaskStatus::Pending),
    ];
    for (prefix, status) in mapping {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return (status, rest.to_string());
        }
    }
    (PlanTaskStatus::Pending, trimmed.to_string())
}

fn parse_task_line(line: &str) -> Option<FlatTaskLine> {
    let indent_spaces = line.chars().take_while(|c| *c == ' ').count();
    let level = indent_spaces / 2;
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))?;

    let markers = [
        ("[x] ", PlanTaskStatus::Completed),
        ("[X] ", PlanTaskStatus::Completed),
        ("[~] ", PlanTaskStatus::InProgress),
        ("[/] ", PlanTaskStatus::InProgress),
        ("[!] ", PlanTaskStatus::Blocked),
        ("[ ] ", PlanTaskStatus::Pending),
    ];

    for (marker, status) in markers {
        if let Some(desc) = rest.strip_prefix(marker) {
            let description = desc.trim();
            if description.is_empty() {
                return None;
            }
            return Some(FlatTaskLine {
                level,
                status,
                description: description.to_string(),
            });
        }
    }

    None
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

        if in_plan_section && let Some(line) = parse_task_line(raw) {
            task_lines.push(line);
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

fn build_flat_create_lines(items: &[String]) -> Vec<FlatTaskLine> {
    items
        .iter()
        .filter_map(|raw| {
            let level = raw.chars().take_while(|c| *c == ' ').count() / 2;
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            let (status, description) = parse_status_prefix(trimmed);
            if description.trim().is_empty() {
                return None;
            }
            Some(FlatTaskLine {
                level,
                status,
                description: description.trim().to_string(),
            })
        })
        .collect()
}

fn append_notes(existing: Option<String>, append: Option<&str>) -> Option<String> {
    match (existing, append) {
        (None, None) => None,
        (Some(text), None) => {
            if text.trim().is_empty() {
                None
            } else {
                Some(text)
            }
        }
        (None, Some(extra)) => {
            let trimmed = extra.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        (Some(text), Some(extra)) => {
            let left = text.trim();
            let right = extra.trim();
            if left.is_empty() && right.is_empty() {
                None
            } else if left.is_empty() {
                Some(right.to_string())
            } else if right.is_empty() {
                Some(left.to_string())
            } else {
                Some(format!("{left}\n{right}"))
            }
        }
    }
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

    async fn handle_create(&self, args: &PlanTaskTrackerArgs) -> Result<Value> {
        let items = args.items.as_deref().unwrap_or(&[]);
        if items.is_empty() {
            bail!(
                "At least one item is required for 'create'. Provide items: [\"step 1\", \"step 2\", ...]"
            );
        }

        let flat_lines = build_flat_create_lines(items);
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

        let tracker_file = self.save_document(&document).await?;
        Ok(Self::success_payload(
            "created",
            "Plan task tracker created successfully.".to_string(),
            &tracker_file,
            &document,
        ))
    }

    async fn handle_update(&self, args: &PlanTaskTrackerArgs) -> Result<Value> {
        let mut document = self
            .load_document()
            .await?
            .context("No active plan tracker. Use action='create' first.")?;

        let index_path = args
            .index_path
            .as_deref()
            .context("'index_path' is required for 'update' (example: \"2.1\")")?;
        let path = parse_index_path(index_path)?;
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
            (old_status, node.status.as_str().to_string())
        };

        document.notes = append_notes(document.notes.take(), args.notes.as_deref());

        let tracker_file = self.save_document(&document).await?;
        Ok(Self::success_payload(
            "updated",
            format!(
                "Item {} status changed: {} -> {}",
                index_path, old_status, new_status_str
            ),
            &tracker_file,
            &document,
        ))
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

        let tracker_file = self.save_document(&document).await?;
        Ok(Self::success_payload(
            "added",
            "Plan task added successfully.".to_string(),
            &tracker_file,
            &document,
        ))
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
        "Plan-mode scoped task tracker. Persists hierarchical plan progress under .vtcode/plans/<plan>.tasks.md. Actions: create, update, list, add."
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
                    "items": { "type": "string" },
                    "description": "Initial task items (used with create). Leading 2-space indentation indicates nesting."
                },
                "index_path": {
                    "type": "string",
                    "description": "Hierarchical index path for update (example: '2.1')."
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
                "parent_index_path": {
                    "type": "string",
                    "description": "Optional parent path for add (example: '2'). If omitted, adds top-level task."
                },
                "notes": {
                    "type": "string",
                    "description": "Optional notes to append."
                }
            },
            "required": ["action"]
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
