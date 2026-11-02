use std::str::FromStr;

use anyhow::{Result, bail};
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod persistence;
pub(crate) use persistence::{
    TrackerArtifact, persist_tracker_artifacts, read_tracker_artifact, write_tracker_artifact,
};

mod schema;
pub(crate) use schema::{
    TASK_ITEMS_DESCRIPTION, task_files_property_schema, task_items_array_schema, task_status_property_schema,
    task_verify_property_schema, tracker_schema_allof,
};
#[cfg(test)]
pub(crate) use schema::{TASK_STATUS_VALUES, task_status_schema};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskStepMetadata {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verify: Vec<String>,
    /// Workspace-scope evidence, kept separate from functional verification.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope_review: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum TaskItemInput {
    Text(String),
    Structured(TaskItemInputObject),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskItemInputObject {
    pub description: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_list")]
    pub files: Option<Vec<String>>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string_list")]
    pub verify: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_string_list")]
    pub scope_review: Option<Vec<String>>,
}

pub(crate) struct TaskMetadataRef<'a> {
    pub description: &'a str,
    pub metadata: &'a TaskStepMetadata,
}

pub(crate) struct ParsedTaskItem {
    pub level: usize,
    pub description: String,
    pub status: TaskTrackingStatus,
    pub metadata: TaskStepMetadata,
}

/// Normalize both tracker variants and preserve omitted metadata only when
/// a description identifies exactly one old step and exactly one new step.
/// Creates pass no previous items; explicit empty metadata still clears it.
pub(crate) fn parse_task_items(
    items: &[TaskItemInput],
    previous: &[TaskMetadataRef<'_>],
) -> Result<Vec<ParsedTaskItem>> {
    validate_task_item_inputs(items)?;
    let mut previous_by_description = HashMap::new();
    for item in previous {
        previous_by_description
            .entry(item.description.trim())
            .and_modify(|metadata| *metadata = None)
            .or_insert(Some(item.metadata));
    }
    let mut description_counts = HashMap::new();
    for item in items {
        let raw = match item {
            TaskItemInput::Text(raw) => raw,
            TaskItemInput::Structured(input) => &input.description,
        };
        let (_, description) = parse_status_prefix(raw.trim());
        *description_counts.entry(description.trim().to_owned()).or_insert(0usize) += 1;
    }

    let mut parsed_items = Vec::with_capacity(items.len());
    for item in items {
        let raw = match item {
            TaskItemInput::Text(raw) => raw,
            TaskItemInput::Structured(input) => &input.description,
        };
        let (mut status, description) = parse_status_prefix(raw.trim());
        let description = description.trim().to_owned();
        if description.is_empty() {
            continue;
        }
        let previous_metadata = if description_counts.get(&description) == Some(&1) {
            previous_by_description.get(description.as_str()).copied().flatten()
        } else {
            None
        };
        let has_previous_scope_review = previous
            .iter()
            .any(|previous| previous.description.trim() == description && !previous.metadata.scope_review.is_empty());
        if previous_metadata.is_none()
            && previous
                .iter()
                .any(|item| item.description.trim() == description && item.metadata != &TaskStepMetadata::default())
            && !matches!(item, TaskItemInput::Structured(input) if input.files.is_some() && input.outcome.is_some() && input.verify.is_some() && (!has_previous_scope_review || input.scope_review.is_some()))
        {
            bail!(
                "Ambiguous task description '{description}': provide explicit files, outcome, verify, and scope_review metadata for duplicate steps. Use empty arrays and an empty outcome to clear them. The checklist was not changed."
            );
        }
        let mut metadata = previous_metadata.cloned().unwrap_or_default();
        if let TaskItemInput::Structured(input) = item {
            if let Some(raw_status) = input.status.as_deref() {
                status = TaskTrackingStatus::from_str(raw_status)?;
            }
            apply_metadata_input(
                &mut metadata,
                input.files.as_deref(),
                input.outcome.as_deref(),
                input.verify.as_deref(),
            );
            if let Some(scope_review) = input.scope_review.as_deref() {
                metadata.scope_review = normalize_verify_items(scope_review.iter().map(String::as_str));
            }
        }
        parsed_items.push(ParsedTaskItem {
            level: raw.chars().take_while(|character| *character == ' ').count() / 2,
            description,
            status,
            metadata,
        });
    }
    Ok(parsed_items)
}

pub(crate) fn apply_metadata_input(
    metadata: &mut TaskStepMetadata,
    files: Option<&[String]>,
    outcome: Option<&str>,
    verify: Option<&[String]>,
) {
    if let Some(files) = files {
        metadata.files = normalize_string_items(Some(files));
    }
    if let Some(outcome) = outcome {
        metadata.outcome = normalize_optional_text(Some(outcome));
    }
    if let Some(verify) = verify {
        metadata.verify = normalize_verify_items(verify.iter().map(String::as_str));
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskTrackingStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
}

impl std::fmt::Display for TaskTrackingStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for TaskTrackingStatus {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "blocked" => Ok(Self::Blocked),
            other => {
                bail!("Invalid status '{other}'. Use: pending, in_progress, completed, blocked")
            }
        }
    }
}

impl TaskTrackingStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
        }
    }

    pub fn flat_checkbox(&self) -> &'static str {
        match self {
            Self::Pending => "[ ]",
            Self::InProgress => "[/]",
            Self::Completed => "[x]",
            Self::Blocked => "[!]",
        }
    }

    pub fn plan_checkbox(&self) -> &'static str {
        match self {
            Self::Pending => "[ ]",
            Self::InProgress => "[~]",
            Self::Completed => "[x]",
            Self::Blocked => "[!]",
        }
    }

    pub fn view_symbol(&self) -> &'static str {
        match self {
            Self::Pending => "•",
            Self::InProgress => ">",
            Self::Completed => "✔",
            Self::Blocked => "!",
        }
    }

    pub fn compact_view_symbol(&self) -> &'static str {
        match self {
            Self::Pending => "□",
            Self::InProgress => "[-]",
            Self::Completed => "[x]",
            Self::Blocked => "[!]",
        }
    }
}

pub fn parse_marked_status_prefix(value: &str) -> Option<(TaskTrackingStatus, String)> {
    let trimmed = value.trim_start();
    let mapping = [
        ("[x] ", TaskTrackingStatus::Completed),
        ("[X] ", TaskTrackingStatus::Completed),
        ("[~] ", TaskTrackingStatus::InProgress),
        ("[/] ", TaskTrackingStatus::InProgress),
        ("[!] ", TaskTrackingStatus::Blocked),
        ("[ ] ", TaskTrackingStatus::Pending),
    ];
    for (prefix, status) in mapping {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some((status, rest.to_string()));
        }
    }
    None
}

pub fn parse_status_prefix(value: &str) -> (TaskTrackingStatus, String) {
    parse_marked_status_prefix(value).unwrap_or((TaskTrackingStatus::Pending, value.trim_start().to_string()))
}

pub fn append_notes(existing: Option<String>, append: Option<&str>) -> Option<String> {
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

pub fn append_notes_section(markdown: &mut String, notes: Option<&str>) {
    if let Some(text) = notes {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            markdown.push_str("\n## Notes\n\n");
            markdown.push_str(trimmed);
            markdown.push('\n');
        }
    }
}

pub(crate) fn validate_update_shape<T>(
    items: Option<&[T]>,
    index: Option<usize>,
    index_path: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    if index.is_some() && index_path.is_some() {
        bail!("task-tracker updates cannot combine 'index' and 'index_path'");
    }

    if items.is_some() && (index.is_some() || index_path.is_some() || status.is_some()) {
        bail!("bulk task-tracker updates cannot combine 'items' with 'index', 'index_path', or 'status'");
    }

    Ok(())
}

pub(crate) fn validate_action_index_fields(action: &str, index: Option<usize>, index_path: Option<&str>) -> Result<()> {
    if action != "update" && (index.is_some() || index_path.is_some()) {
        bail!(
            "task-tracker action '{action}' cannot use 'index' or 'index_path'; indices are only valid with action='update'"
        );
    }

    Ok(())
}

/// Checklist replacement must not interpret encoded update commands as descriptions.
pub(crate) fn validate_task_item_inputs(items: &[TaskItemInput]) -> Result<()> {
    for item in items {
        let TaskItemInput::Text(text) = item else {
            continue;
        };
        let (_, description) = parse_status_prefix(text.trim());
        if let Ok(Value::Object(object)) = serde_json::from_str::<Value>(&description)
            && ["index", "index_path", "description", "status"]
                .iter()
                .any(|key| object.contains_key(*key))
        {
            bail!(
                "'items' replaces the complete checklist; JSON-encoded task updates are not task descriptions. \
                 Update an existing step directly: {{\"action\":\"update\",\"index_path\":\"1\",\"status\":\"completed\"}}. \
                 For full synchronization, send descriptions with statuses, for example \
                 {{\"action\":\"update\",\"items\":[{{\"description\":\"Original step\",\"status\":\"completed\"}}]}}. \
                 The checklist was not changed. Retry once with the intended shape."
            );
        }
    }
    Ok(())
}

pub fn is_bulk_sync_update<T>(
    items: Option<&[T]>,
    index: Option<usize>,
    index_path: Option<&str>,
    status: Option<&str>,
) -> bool {
    items.is_some() && index.is_none() && index_path.is_none() && status.is_none()
}

pub fn deserialize_optional_string_list<'de, D>(deserializer: D) -> std::result::Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }

    let parsed = Option::<OneOrMany>::deserialize(deserializer)?;
    Ok(parsed.map(|value| match value {
        OneOrMany::One(item) => vec![item],
        OneOrMany::Many(items) => items,
    }))
}

/// Strip one balanced markdown code-span pair from a `verify` entry.
///
/// `verify` values are shell commands, and models faithfully emit them in the
/// code-spans the prompt examples use (`verify: [`cargo check`]`). A stored
/// code span is never valid shell, so the decoration must not reach
/// strings that are later compared literally or re-executed — otherwise the
/// completion gate demands an "exact command" the executor is forbidden to
/// type (preflight rejects code spans as command substitution).
/// Callers choose where to apply this: producer ingestion
/// (`apply_metadata_input`/`apply_metadata_line`) for canonical storage, or
/// a consumer defense boundary. Markdown mirrors keep code spans.
pub(crate) fn unwrap_verify_code_span(command: &str) -> &str {
    let trimmed = command.trim();
    if trimmed.starts_with('`') && trimmed.ends_with('`') && trimmed.len() >= 2 {
        let inner = &trimmed[1..trimmed.len() - 1];
        if !inner.contains('`') {
            return inner;
        }
    }
    trimmed
}

/// Normalize the executable `verify` item: trim, drop the code-span
/// decoration, and drop empties.
pub(crate) fn normalize_verify_items<'a>(items: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    items
        .into_iter()
        .filter_map(|item| {
            let unwrapped = unwrap_verify_code_span(item);
            (!unwrapped.is_empty()).then(|| unwrapped.to_owned())
        })
        .collect()
}

pub fn normalize_string_items(items: Option<&[String]>) -> Vec<String> {
    items
        .unwrap_or(&[])
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned)
}

/// Parse a `files: [...]` metadata value into trimmed non-empty paths.
/// Commas inside single/double quotes stay part of one item.
pub fn parse_files_metadata(value: &str) -> Vec<String> {
    super::planning_workflow::split_bracket_items(value)
        .into_iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

/// Consume one indented markdown metadata line (`files:`, `outcome:`,
/// `verify:`, `scope-review:`) into `metadata`, tracking multi-line `verify:` blocks through
/// `in_verify_block`. Returns `true` when the line was metadata.
pub fn apply_metadata_line(metadata: &mut TaskStepMetadata, raw: &str, in_verify_block: &mut bool) -> bool {
    let trimmed = raw.trim_start();

    if *in_verify_block {
        if let Some(command) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            if let Some(command) = normalize_verify_items([command]).pop() {
                metadata.verify.push(command);
            }
            return true;
        }
        *in_verify_block = false;
    }

    if let Some(rest) = trimmed.strip_prefix("files:") {
        metadata.files = parse_files_metadata(rest);
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("outcome:") {
        metadata.outcome = normalize_optional_text(Some(rest));
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("scope-review:") {
        if rest.trim().is_empty() {
            metadata.scope_review.clear();
        } else {
            metadata.scope_review.extend(normalize_verify_items([rest]));
        }
        return true;
    }
    if trimmed == "verify:" {
        metadata.verify.clear();
        *in_verify_block = true;
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("verify:") {
        metadata.verify = normalize_verify_items([rest]);
        return true;
    }

    false
}

pub fn append_task_step_metadata(markdown: &mut String, indent: &str, metadata: &TaskStepMetadata) {
    if !metadata.files.is_empty() {
        markdown.push_str(indent);
        markdown.push_str("  files: ");
        markdown.push_str(&metadata.files.join(", "));
        markdown.push('\n');
    }

    if let Some(outcome) = metadata.outcome.as_deref() {
        markdown.push_str(indent);
        markdown.push_str("  outcome: ");
        markdown.push_str(outcome);
        markdown.push('\n');
    }

    for command in &metadata.scope_review {
        markdown.push_str(indent);
        markdown.push_str("  scope-review: ");
        markdown.push_str(command);
        markdown.push('\n');
    }

    if metadata.verify.len() == 1 {
        markdown.push_str(indent);
        markdown.push_str("  verify: ");
        markdown.push_str(&metadata.verify[0]);
        markdown.push('\n');
    } else if !metadata.verify.is_empty() {
        markdown.push_str(indent);
        markdown.push_str("  verify:\n");
        for command in &metadata.verify {
            markdown.push_str(indent);
            markdown.push_str("    - ");
            markdown.push_str(command);
            markdown.push('\n');
        }
    }
}

pub fn metadata_from_input(
    files: Option<&[String]>,
    outcome: Option<&str>,
    verify: Option<&[String]>,
) -> TaskStepMetadata {
    let mut metadata = TaskStepMetadata::default();
    apply_metadata_input(&mut metadata, files, outcome, verify);
    metadata
}

/// Markers that may trail a task description as inline `Action -> files: [...]
/// -> verify: [...]` metadata. `outcome` is accepted so a polluted suffix is
/// still stripped from the visible row even though the compact view never
/// renders it.
const INLINE_TASK_METADATA_MARKERS: &[&str] = &[" -> files:", " -> verify:", " -> outcome:"];

fn parse_inline_bracket_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(trimmed);
    super::planning_workflow::split_bracket_items(inner)
        .into_iter()
        .map(|item| item.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect()
}

/// Split an inline `Action -> files: [...] -> verify: [...]` suffix out of a
/// task description.
///
/// Returns `(clean_description, files, verify)`. Markers are recognized only
/// outside inline-code spans so `` `a -> files: b` `` stays intact, and the
/// Unicode `→` arrow is normalized first so both spellings agree with plan
/// validation. When no marker exists outside backticks the description is
/// returned trimmed with empty metadata.
pub fn split_task_description_metadata(description: &str) -> (String, Vec<String>, Vec<String>) {
    let normalized;
    let source = if description.contains('→') {
        normalized = description.replace('→', "->");
        normalized.as_str()
    } else {
        description
    };
    let Some(first) = find_inline_metadata_marker(source, 0) else {
        return (source.trim().to_string(), Vec::new(), Vec::new());
    };
    let head = source[..first.0].trim();
    if head.is_empty() {
        return (source.trim().to_string(), Vec::new(), Vec::new());
    }

    let mut files = Vec::new();
    let mut verify = Vec::new();
    let mut cursor = first.0;
    while let Some((marker_start, marker)) = find_inline_metadata_marker(source, cursor) {
        let value_start = marker_start + marker.len();
        let next = find_inline_metadata_marker(source, value_start)
            .map(|(start, _)| start)
            .unwrap_or(source.len());
        let value = source[value_start..next].trim();
        if marker.contains("files:") {
            files.extend(parse_inline_bracket_list(value));
        } else if marker.contains("verify:") {
            verify.extend(parse_inline_bracket_list(value));
        }
        cursor = next;
        if cursor >= source.len() {
            break;
        }
    }
    (head.to_string(), files, verify)
}

/// Strip an inline `-> files:` / `-> verify:` / `-> outcome:` suffix for
/// display. Structured metadata stays in the payload; the visible compact row
/// must not reintroduce it as detail text.
pub fn strip_task_description_metadata(description: &str) -> String {
    split_task_description_metadata(description).0
}

/// One short description for a task row: the leading clause before detail
/// separators (` — `/` – `/` - `/`: `, `; `, `. `, ` (`).
///
/// Plan steps often carry a long detail tail
/// (`"Add X – document Y in Z, sourced from W"`); the TODO transcript and
/// panel must show only `"Add X"` so each item stays on one scannable row.
/// Inline `-> files:`/`-> verify:` suffixes are stripped first; the full text
/// stays in structured tracker metadata.
pub fn short_task_description(description: &str) -> String {
    let (clean, _, _) = split_task_description_metadata(description);
    let base = if clean.trim().is_empty() {
        description.trim()
    } else {
        clean.trim()
    };
    let first_line = base.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    let collapsed = vtcode_commons::formatting::collapse_whitespace(first_line);
    if collapsed.is_empty() {
        return String::new();
    }
    let mut cut = collapsed.len();
    for separator in [" — ", " – ", " - ", ": ", "; ", ". ", " ("] {
        if let Some(index) = collapsed.find(separator) {
            cut = cut.min(index);
        }
    }
    let short = collapsed[..cut].trim().trim_end_matches([',', ';', ':', '.']).trim();
    if short.is_empty() { collapsed } else { short.to_string() }
}

fn find_inline_metadata_marker(source: &str, from: usize) -> Option<(usize, &'static str)> {
    let mut inline_ticks: Option<usize> = None;
    let mut cursor = from.min(source.len());
    while cursor < source.len() {
        let remainder = &source[cursor..];
        if remainder.starts_with('`') {
            let run = remainder.bytes().take_while(|byte| *byte == b'`').count();
            if inline_ticks.is_some_and(|ticks| ticks == run) {
                inline_ticks = None;
            } else if inline_ticks.is_none() {
                inline_ticks = Some(run);
            }
            cursor += run;
            continue;
        }
        if inline_ticks.is_none() {
            for marker in INLINE_TASK_METADATA_MARKERS {
                if remainder.starts_with(marker) {
                    return Some((cursor, marker));
                }
            }
        }
        let character = remainder.chars().next()?;
        cursor += character.len_utf8();
    }
    None
}

/// A renderer-independent task tree used by both task-tracker implementations.
///
/// The persisted checklist formats remain implementation-specific; this type is
/// only the shared shape required to produce the compact visible view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskTreeNode {
    pub(crate) index_path: String,
    pub(crate) description: String,
    pub(crate) status: TaskTrackingStatus,
    pub(crate) metadata: TaskStepMetadata,
    pub(crate) children: Vec<TaskTreeNode>,
}

/// Format task nodes as the compact tree shared by inline output and the task
/// panel. Root leaves retain `├`/`└` connectors; nested leaves use their status
/// glyph as the connector so the hierarchy stays readable without redundant
/// branch characters.
pub(crate) fn compact_task_tree_view(nodes: &[TaskTreeNode]) -> Vec<Value> {
    let mut lines = Vec::new();
    append_compact_task_tree_view(nodes, "  ", &mut lines);
    lines
}

fn append_compact_task_tree_view(nodes: &[TaskTreeNode], tree_prefix: &str, out: &mut Vec<Value>) {
    for (index, node) in nodes.iter().enumerate() {
        let is_last = index + 1 == nodes.len();
        let branch = if is_last { "└" } else { "├" };
        let display = if node.children.is_empty() {
            if tree_prefix == "  " {
                format!("{tree_prefix}{branch} {} {}", node.status.compact_view_symbol(), node.description)
            } else {
                format!("{tree_prefix}{} {}", node.status.compact_view_symbol(), node.description)
            }
        } else {
            format!("{tree_prefix}{branch} {}", node.description)
        };

        out.push(json!({
            "display": display,
            "index_path": node.index_path.clone(),
            "status": node.status.as_str(),
            "text": node.description.clone(),
            "files": node.metadata.files.clone(),
            "outcome": node.metadata.outcome.clone(),
            "verify": node.metadata.verify.clone(),
            "scope_review": node.metadata.scope_review.clone(),
        }));

        let next_prefix = if is_last {
            format!("{tree_prefix}  ")
        } else {
            format!("{tree_prefix}│ ")
        };
        append_compact_task_tree_view(&node.children, &next_prefix, out);
    }
}

/// Build the compact tree from a flattened checklist summary.
///
/// Standard task trackers expose numeric `index` values while planning
/// trackers expose dotted `index_path` values. Accept both forms so callers at
/// the UI boundary do not need to know which tracker produced the payload.
pub fn compact_task_tree_view_from_items(items: &[Value]) -> Vec<Value> {
    let mut ordered_paths = Vec::with_capacity(items.len());
    let mut nodes_by_path = std::collections::HashMap::with_capacity(items.len());

    for (position, item) in items.iter().enumerate() {
        let index_path = item
            .get("index_path")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| item.get("index").and_then(Value::as_u64).map(|index| index.to_string()))
            .unwrap_or_else(|| (position + 1).to_string());
        if nodes_by_path.contains_key(&index_path) {
            continue;
        }

        let status = match item.get("status").and_then(Value::as_str) {
            Some(value) => match TaskTrackingStatus::from_str(value) {
                Ok(status) => status,
                Err(_) => continue,
            },
            None => TaskTrackingStatus::Pending,
        };
        let Some(description) = item
            .get("description")
            .and_then(Value::as_str)
            .or_else(|| item.get("text").and_then(Value::as_str))
            .filter(|description| !description.trim().is_empty())
        else {
            continue;
        };
        // Descriptions copied from plan steps may still carry an inline
        // `Action -> files: [...] -> verify: [...]` suffix plus a long detail
        // tail (`"Add X – document Y, sourced from Z"`). Keep only the short
        // leading clause for the visible row and fold the rest into structured
        // metadata so the compact view never reintroduces files/verify/detail
        // text.
        let (clean_description, parsed_files, parsed_verify) = split_task_description_metadata(description);
        let raw = if clean_description.trim().is_empty() {
            description.trim()
        } else {
            clean_description.trim()
        };
        let description = short_task_description(raw);
        let mut metadata = TaskStepMetadata {
            files: string_array(item.get("files")),
            outcome: item.get("outcome").and_then(Value::as_str).map(ToOwned::to_owned),
            verify: string_or_string_array(item.get("verify")),
            scope_review: string_or_string_array(item.get("scope_review")),
        };
        if metadata.files.is_empty() {
            metadata.files = parsed_files;
        }
        if metadata.verify.is_empty() {
            metadata.verify = parsed_verify;
        }

        ordered_paths.push(index_path.clone());
        nodes_by_path.insert(
            index_path.clone(),
            TaskTreeNode {
                index_path,
                description,
                status,
                metadata,
                children: Vec::new(),
            },
        );
    }

    let mut attachment_order = ordered_paths.clone();
    attachment_order.sort_by_key(|path| std::cmp::Reverse(path.split('.').count()));
    for path in attachment_order {
        let Some(parent_path) = path.rsplit_once('.').map(|(parent, _)| parent.to_string()) else {
            continue;
        };
        let Some(child) = nodes_by_path.remove(&path) else {
            continue;
        };
        if let Some(parent) = nodes_by_path.get_mut(&parent_path) {
            parent.children.push(child);
        } else {
            nodes_by_path.insert(path, child);
        }
    }

    let roots = ordered_paths
        .into_iter()
        .filter_map(|path| nodes_by_path.remove(&path))
        .collect::<Vec<_>>();
    compact_task_tree_view(&roots)
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn string_or_string_array(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(value)) => vec![value.clone()],
        Some(Value::Array(_)) => string_array(value),
        _ => Vec::new(),
    }
}

#[derive(Default)]
pub struct TaskCounts {
    pub total: usize,
    pub completed: usize,
    pub in_progress: usize,
    pub pending: usize,
    pub blocked: usize,
}

impl TaskCounts {
    pub fn add(&mut self, status: &TaskTrackingStatus) {
        self.total += 1;
        match status {
            TaskTrackingStatus::Pending => self.pending += 1,
            TaskTrackingStatus::InProgress => self.in_progress += 1,
            TaskTrackingStatus::Completed => self.completed += 1,
            TaskTrackingStatus::Blocked => self.blocked += 1,
        }
    }

    pub fn progress_percent(&self) -> usize {
        if self.total > 0 {
            #[allow(
                clippy::cast_sign_loss,
                clippy::let_and_return,
                reason = "Intentional compatibility, platform, or test-only suppression."
            )]
            let progress = ((self.completed as f64 / self.total as f64 * 100.0).round()).max(0.0) as usize;
            progress
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests;
