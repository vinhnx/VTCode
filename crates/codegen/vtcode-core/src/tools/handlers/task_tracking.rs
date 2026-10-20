use std::str::FromStr;

use anyhow::{Result, bail};
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod persistence;
pub(crate) use persistence::{
    TrackerArtifact, persist_tracker_artifacts, read_tracker_artifact, write_tracker_artifact,
};

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

pub(crate) const TASK_ITEMS_DESCRIPTION: &str = "Full checklist replacement for create or bulk update, not indexed updates. Send plain descriptions with [x]/[~]/[!]/[ ] status prefixes or actual objects with description and status; never JSON-encoded strings. Bulk updates preserve omitted metadata for unique unchanged descriptions. Explicit metadata replaces it; use empty arrays or an empty outcome to clear fields. Duplicate descriptions with existing metadata require explicit files, outcome, and verify; include scope_review when existing scope checks would otherwise be ambiguous.";

/// Shared status values for every task-tracker schema (standard + planning,
/// top-level and nested item objects). Single source so a future status
/// addition cannot drift between the planning on/off variants.
pub(crate) const TASK_STATUS_VALUES: [&str; 4] = ["pending", "in_progress", "completed", "blocked"];

/// Base `status` type schema without a description (callers add their own).
pub(crate) fn task_status_schema() -> Value {
    json!({
        "type": "string",
        "enum": TASK_STATUS_VALUES
    })
}

/// Base `files` type schema without a description.
pub(crate) fn task_files_schema() -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" }
    })
}

/// Base `verify` type schema without a description (string or string list).
pub(crate) fn task_verify_schema() -> Value {
    json!({
        "anyOf": [
            { "type": "string" },
            {
                "type": "array",
                "items": { "type": "string" }
            }
        ]
    })
}

fn with_description(mut schema: Value, description: &str) -> Value {
    if let Value::Object(map) = &mut schema {
        map.insert("description".to_string(), Value::String(description.to_string()));
    }
    schema
}

/// Shared `allOf` conditional blocks for both tracker parameter schemas.
///
/// The create, update, no-indices, and add blocks are identical across
/// trackers; the update `anyOf` arms are order-normalized (validation is
/// order-independent and no test indexes arm positions). The `index == 0`
/// completion rule is standard-tracker-only (planning indices start at 1):
/// `include_index_zero_rule` inserts it at position 2, preserving the
/// `allOf[2]` contract asserted by the standard schema tests.
pub(crate) fn tracker_schema_allof(include_index_zero_rule: bool) -> Vec<Value> {
    let mut allof = vec![
        json!({
            "if": {
                "properties": { "action": { "const": "create" } },
                "required": ["action"]
            },
            "then": {
                "required": ["items"]
            }
        }),
        json!({
            "if": {
                "properties": { "action": { "const": "update" } },
                "required": ["action"]
            },
            "then": {
                "anyOf": [
                    {
                        "required": ["index", "status"],
                        "not": {
                            "anyOf": [
                                { "required": ["items"] },
                                { "required": ["index_path"] }
                            ]
                        }
                    },
                    {
                        "required": ["index_path", "status"],
                        "not": {
                            "anyOf": [
                                { "required": ["items"] },
                                { "required": ["index"] }
                            ]
                        }
                    },
                    {
                        "required": ["items"],
                        "not": {
                            "anyOf": [
                                { "required": ["index"] },
                                { "required": ["index_path"] },
                                { "required": ["status"] }
                            ]
                        }
                    }
                ]
            }
        }),
        json!({
            "if": {
                "properties": { "action": { "enum": ["create", "list", "add"] } },
                "required": ["action"]
            },
            "then": {
                "not": {
                    "anyOf": [
                        { "required": ["index"] },
                        { "required": ["index_path"] }
                    ]
                }
            }
        }),
        json!({
            "if": {
                "properties": { "action": { "const": "add" } },
                "required": ["action"]
            },
            "then": {
                "required": ["description"]
            }
        }),
    ];
    if include_index_zero_rule {
        allof.insert(
            2,
            json!({
                "if": {
                    "properties": { "index": { "const": 0 } },
                    "required": ["index"]
                },
                "then": {
                    "properties": {
                        "action": { "const": "update" },
                        "status": { "const": "completed" }
                    },
                    "required": ["action", "status"]
                }
            }),
        );
    }
    allof
}

/// Shared nested item-object schema (description + optional status/files/
/// outcome/verify). Used inside the `items` array of both tracker schemas.
pub(crate) fn task_item_object_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "description": { "type": "string" },
            "status": task_status_schema(),
            "files": task_files_schema(),
            "outcome": { "type": "string" },
            "verify": task_verify_schema(),
            "scope_review": with_description(task_verify_schema(), "Workspace scope checks; do not satisfy functional verification.")
        },
        "required": ["description"]
    })
}

/// Shared `items` array schema with a caller-supplied description.
pub(crate) fn task_items_array_schema(items_description: &str) -> Value {
    with_description(
        json!({
            "type": "array",
            "items": {
                "anyOf": [
                    { "type": "string" },
                    task_item_object_schema()
                ]
            }
        }),
        items_description,
    )
}

/// Top-level `status` property with its own description.
pub(crate) fn task_status_property_schema(description: &str) -> Value {
    with_description(task_status_schema(), description)
}

/// Top-level `files` property with its own description.
pub(crate) fn task_files_property_schema(description: &str) -> Value {
    with_description(task_files_schema(), description)
}

/// Top-level `verify` property with its own description.
pub(crate) fn task_verify_property_schema(description: &str) -> Value {
    with_description(task_verify_schema(), description)
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
mod tests {
    use super::*;

    #[test]
    fn scope_review_rows_round_trip_without_functional_verification_credit() {
        let mut metadata = TaskStepMetadata::default();
        let mut in_verify_block = false;
        for row in [
            "  files: README.md",
            "  scope-review: `git status --porcelain`",
            "  scope-review: git diff --stat HEAD -- README.md",
        ] {
            assert!(apply_metadata_line(&mut metadata, row, &mut in_verify_block));
        }
        assert_eq!(metadata.scope_review, vec!["git status --porcelain", "git diff --stat HEAD -- README.md"]);
        assert!(metadata.verify.is_empty());
        let mut markdown = String::new();
        append_task_step_metadata(&mut markdown, "", &metadata);
        assert_eq!(
            markdown,
            "  files: README.md\n  scope-review: git status --porcelain\n  scope-review: git diff --stat HEAD -- README.md\n"
        );
        let old: TaskStepMetadata = serde_json::from_value(json!({"verify": ["cargo check"]})).unwrap();
        assert!(old.scope_review.is_empty());
        assert_eq!(old.verify, vec!["cargo check"]);
        let restored: TaskStepMetadata = serde_json::from_value(serde_json::to_value(&metadata).unwrap()).unwrap();
        assert_eq!(restored, metadata);
        let previous = [TaskMetadataRef { description: "Review scope", metadata: &metadata }];
        let updated = parse_task_items(&[TaskItemInput::Text("[x] Review scope".to_string())], &previous).unwrap();
        assert_eq!(updated[0].metadata.scope_review, metadata.scope_review);
        assert!(updated[0].metadata.verify.is_empty());
        let cleared = parse_task_items(
            &[TaskItemInput::Structured(TaskItemInputObject {
                description: "Review scope".to_string(),
                scope_review: Some(Vec::new()),
                ..Default::default()
            })],
            &previous,
        )
        .unwrap();
        assert!(cleared[0].metadata.scope_review.is_empty());
    }

    #[test]
    fn compact_tree_retains_scope_metadata_in_a_distinct_field() {
        let rows = compact_task_tree_view_from_items(&[json!({
            "description": "Review scope", "scope_review": ["git status --porcelain"],
            "verify": ["rg -n 'Overview' README.md"],
        })]);
        assert_eq!(rows[0]["scope_review"], json!(["git status --porcelain"]));
        assert_eq!(rows[0]["verify"], json!(["rg -n 'Overview' README.md"]));
    }

    #[test]
    fn parse_files_metadata_keeps_quoted_commas_as_single_item() {
        let items = parse_files_metadata("'src/a.rs, b.rs', src/c.rs");
        assert_eq!(items, vec!["'src/a.rs, b.rs'".to_string(), "src/c.rs".to_string()]);
    }

    #[test]
    fn apply_metadata_line_drives_verify_block_state_machine() {
        let mut metadata = TaskStepMetadata::default();
        let mut in_verify_block = false;

        // Non-metadata lines are rejected without touching state.
        assert!(!apply_metadata_line(&mut metadata, "just a description", &mut in_verify_block));
        assert!(!in_verify_block);
        assert!(metadata.verify.is_empty());

        // files:/outcome: lines are consumed directly (persisted bracket-free).
        assert!(apply_metadata_line(&mut metadata, "  files: a.rs, b.rs", &mut in_verify_block));
        assert!(apply_metadata_line(&mut metadata, "  outcome: did the thing", &mut in_verify_block));
        assert_eq!(metadata.files, vec!["a.rs".to_string(), "b.rs".to_string()]);
        assert_eq!(metadata.outcome.as_deref(), Some("did the thing"));

        // Bare `verify:` opens a block; mixed bullet markers accumulate.
        assert!(apply_metadata_line(&mut metadata, "  verify:", &mut in_verify_block));
        assert!(in_verify_block);
        assert!(apply_metadata_line(&mut metadata, "    - cargo test", &mut in_verify_block));
        assert!(apply_metadata_line(&mut metadata, "    * cargo clippy", &mut in_verify_block));
        assert_eq!(metadata.verify, vec!["cargo test".to_string(), "cargo clippy".to_string()]);

        // A non-bullet line closes the block and is itself rejected.
        assert!(!apply_metadata_line(&mut metadata, "  other: x", &mut in_verify_block));
        assert!(!in_verify_block);

        // Inline `verify:` replaces the accumulated block.
        assert!(apply_metadata_line(&mut metadata, "  verify: make check", &mut in_verify_block));
        assert_eq!(metadata.verify, vec!["make check".to_string()]);
    }

    #[test]
    fn verify_ingestion_strips_code_span_decoration() {
        // Regression for session-vtcode-20261019T075619Z: models emit verify
        // in the taught code-span form, and a stored code span is (a) never
        // valid shell and (b) forbidden to type back at preflight, which
        // deadlocked the completion-verification gate. Ingestion must store
        // the executable command in every input shape.
        let mut metadata = TaskStepMetadata::default();
        let mut in_verify_block = false;
        assert!(apply_metadata_line(
            &mut metadata,
            "  verify: `./scripts/update-contributors-readme.sh --dry-run`",
            &mut in_verify_block
        ));
        assert_eq!(metadata.verify, vec!["./scripts/update-contributors-readme.sh --dry-run".to_string()]);
        // Multi-line block entries unwrap too.
        assert!(apply_metadata_line(&mut metadata, "  verify:", &mut in_verify_block));
        assert!(apply_metadata_line(&mut metadata, "    - `git show --stat HEAD`", &mut in_verify_block));
        assert_eq!(metadata.verify, vec!["git show --stat HEAD".to_string()]);
        // Structured API input (create/update items) unwraps as well.
        let mut structured = TaskStepMetadata::default();
        apply_metadata_input(
            &mut structured,
            Some(&["README.md".to_string()]),
            None,
            Some(&["`cargo check --locked`".to_string()]),
        );
        assert_eq!(structured.verify, vec!["cargo check --locked".to_string()]);
        // Nested/unbalanced spans are left untouched (fail-safe).
        let mut nested = TaskStepMetadata::default();
        apply_metadata_input(&mut nested, None, None, Some(&["`grep -n '`x`' f`".to_string()]));
        assert_eq!(nested.verify, vec!["`grep -n '`x`' f`".to_string()]);
    }

    #[test]
    fn parse_marked_status_prefix_rejects_unmarked_text() {
        let parsed = parse_marked_status_prefix("plain text without marker");
        assert!(parsed.is_none());
    }

    #[test]
    fn parse_status_prefix_defaults_to_pending_for_unmarked_text() {
        let (status, description) = parse_status_prefix("plain text without marker");
        assert_eq!(status, TaskTrackingStatus::Pending);
        assert_eq!(description, "plain text without marker");
    }

    #[test]
    fn parse_status_prefix_supports_both_in_progress_markers() {
        let (status_tilde, text_tilde) = parse_status_prefix("[~] do thing");
        let (status_slash, text_slash) = parse_status_prefix("[/] do thing");
        assert_eq!(status_tilde, TaskTrackingStatus::InProgress);
        assert_eq!(status_slash, TaskTrackingStatus::InProgress);
        assert_eq!(text_tilde, "do thing");
        assert_eq!(text_slash, "do thing");
    }

    #[test]
    fn append_notes_joins_with_single_newline() {
        let merged = append_notes(Some("left".to_string()), Some("right"));
        assert_eq!(merged, Some("left\nright".to_string()));
    }

    #[test]
    fn append_notes_section_ignores_blank_notes() {
        let mut markdown = "# Title\n".to_string();
        append_notes_section(&mut markdown, Some("   "));
        assert_eq!(markdown, "# Title\n");
    }

    #[test]
    fn bulk_sync_validation_preserves_text_and_actual_structured_items() {
        let valid = vec![
            TaskItemInput::Text(r#"[x] Document {"status":"completed"}"#.to_string()),
            TaskItemInput::Text(r#"{"example":"literal JSON"}"#.to_string()),
            TaskItemInput::Structured(TaskItemInputObject {
                description: "Original step".to_string(),
                status: Some("completed".to_string()),
                ..Default::default()
            }),
        ];
        validate_task_item_inputs(&valid).unwrap();
        for text in [
            r#"{"index":1,"status":"completed"}"#,
            r#"{"description":"Encoded step","status":"completed"}"#,
        ] {
            for prefix in ["", "[x] ", "[X] ", "[~] ", "[/] ", "[!] ", "[ ] "] {
                let item = TaskItemInput::Text(format!("  {prefix}{text}  "));
                assert!(validate_task_item_inputs(&[item]).is_err(), "prefix {prefix:?}");
            }
        }
    }

    #[test]
    fn is_bulk_sync_update_requires_items_and_missing_single_item_fields() {
        let items = vec!["Step".to_string()];
        let no_items: Option<&[String]> = None;
        assert!(is_bulk_sync_update(Some(&items), None, None, None));
        assert!(!is_bulk_sync_update(no_items, Some(1), None, Some("completed")));
    }

    #[test]
    fn is_bulk_sync_update_rejects_mixed_single_item_fields() {
        let items = vec!["Step".to_string()];
        let error = validate_update_shape(Some(&items), Some(1), None, Some("completed"))
            .expect_err("mixed bulk and single update fields must fail closed");

        assert!(error.to_string().contains("cannot combine 'items'"));
    }

    #[test]
    fn task_tracker_update_rejects_both_index_forms() {
        let error = validate_update_shape::<String>(None, Some(1), Some("1"), Some("completed"))
            .expect_err("an update must select exactly one index form");

        assert!(error.to_string().contains("cannot combine 'index' and 'index_path'"));
    }

    #[test]
    fn task_counts_tracks_progress() {
        let mut counts = TaskCounts::default();
        counts.add(&TaskTrackingStatus::Completed);
        counts.add(&TaskTrackingStatus::Pending);
        counts.add(&TaskTrackingStatus::Blocked);
        counts.add(&TaskTrackingStatus::InProgress);
        assert_eq!(counts.total, 4);
        assert_eq!(counts.completed, 1);
        assert_eq!(counts.pending, 1);
        assert_eq!(counts.blocked, 1);
        assert_eq!(counts.in_progress, 1);
        assert_eq!(counts.progress_percent(), 25);
    }

    #[test]
    fn compact_task_tree_view_formats_flat_leaf_statuses_exactly() {
        let nodes = [
            TaskTreeNode {
                index_path: "1".to_string(),
                description: "Pending".to_string(),
                status: TaskTrackingStatus::Pending,
                metadata: TaskStepMetadata::default(),
                children: Vec::new(),
            },
            TaskTreeNode {
                index_path: "2".to_string(),
                description: "In progress".to_string(),
                status: TaskTrackingStatus::InProgress,
                metadata: TaskStepMetadata::default(),
                children: Vec::new(),
            },
            TaskTreeNode {
                index_path: "3".to_string(),
                description: "Completed".to_string(),
                status: TaskTrackingStatus::Completed,
                metadata: TaskStepMetadata::default(),
                children: Vec::new(),
            },
            TaskTreeNode {
                index_path: "4".to_string(),
                description: "Blocked".to_string(),
                status: TaskTrackingStatus::Blocked,
                metadata: TaskStepMetadata::default(),
                children: Vec::new(),
            },
        ];

        let displays = compact_task_tree_view(&nodes)
            .into_iter()
            .filter_map(|line| line.get("display").and_then(Value::as_str).map(ToOwned::to_owned))
            .collect::<Vec<_>>();
        assert_eq!(
            displays,
            vec![
                "  ├ □ Pending".to_string(),
                "  ├ [-] In progress".to_string(),
                "  ├ [x] Completed".to_string(),
                "  └ [!] Blocked".to_string(),
            ]
        );
    }

    #[test]
    fn compact_task_tree_view_omits_parent_status_but_keeps_metadata_fields() {
        let nodes = [TaskTreeNode {
            index_path: "1".to_string(),
            description: "Parent".to_string(),
            status: TaskTrackingStatus::InProgress,
            metadata: TaskStepMetadata {
                files: vec!["Cargo.toml".to_string()],
                outcome: Some("Ready".to_string()),
                verify: vec!["cargo check".to_string()],
                ..Default::default()
            },
            children: vec![TaskTreeNode {
                index_path: "1.1".to_string(),
                description: "Child".to_string(),
                status: TaskTrackingStatus::Completed,
                metadata: TaskStepMetadata::default(),
                children: Vec::new(),
            }],
        }];

        let lines = compact_task_tree_view(&nodes);
        assert_eq!(lines[0]["display"], "  └ Parent");
        assert_eq!(lines[1]["display"], "    [x] Child");
        assert_eq!(lines[0]["files"], json!(["Cargo.toml"]));
        assert_eq!(lines[0]["outcome"], "Ready");
        assert_eq!(lines[0]["verify"], json!(["cargo check"]));
    }

    #[test]
    fn split_task_description_metadata_strips_files_and_verify_suffix() {
        let (clean, files, verify) =
            split_task_description_metadata("Emit summary -> files: [src/a.rs, src/b.rs] -> verify: [cargo check]");
        assert_eq!(clean, "Emit summary");
        assert_eq!(files, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
        assert_eq!(verify, vec!["cargo check".to_string()]);
    }

    #[test]
    fn split_task_description_metadata_keeps_inline_code_arrows_intact() {
        let (clean, files, verify) =
            split_task_description_metadata("Use `a -> files: b` in code -> files: [src/a.rs]");
        assert_eq!(clean, "Use `a -> files: b` in code");
        assert_eq!(files, vec!["src/a.rs".to_string()]);
        assert!(verify.is_empty());
    }

    #[test]
    fn split_task_description_metadata_without_markers_returns_trimmed_description() {
        let (clean, files, verify) = split_task_description_metadata("  Just an action  ");
        assert_eq!(clean, "Just an action");
        assert!(files.is_empty());
        assert!(verify.is_empty());
    }

    #[test]
    fn split_task_description_metadata_preserves_quoted_commas_in_files_and_verify() {
        let (clean, files, verify) = split_task_description_metadata(
            "Inspect range -> files: ['src/a.rs, b.rs', src/c.rs] -> verify: [sed -n '/^## A/,/^## B/p']",
        );
        assert_eq!(clean, "Inspect range");
        assert_eq!(files, vec!["'src/a.rs, b.rs'".to_string(), "src/c.rs".to_string()]);
        assert_eq!(verify, vec!["sed -n '/^## A/,/^## B/p'".to_string()]);
    }

    #[test]
    fn compact_task_tree_view_from_items_strips_inline_metadata_into_fields() {
        let items = vec![json!({
            "index_path": "1",
            "description": "Update parser -> files: [src/a.rs] -> verify: [cargo check]",
            "status": "pending",
        })];
        let rows = compact_task_tree_view_from_items(&items);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["display"], "  └ □ Update parser");
        assert_eq!(rows[0]["text"], "Update parser");
        assert_eq!(rows[0]["files"], json!(["src/a.rs"]));
        assert_eq!(rows[0]["verify"], json!(["cargo check"]));
    }

    #[test]
    fn short_task_description_keeps_leading_clause_only() {
        assert_eq!(
            short_task_description(
                "Add vtcode exec resume to the Commands section – document the cross-turn contract, sourced from ExecSubcommand::Resume"
            ),
            "Add vtcode exec resume to the Commands section"
        );
        assert_eq!(
            short_task_description("Update the Everyday recipes block — add a headless example"),
            "Update the Everyday recipes block"
        );
        assert_eq!(
            short_task_description("Emit summary -> files: [src/a.rs] -> verify: [cargo check]"),
            "Emit summary"
        );
        assert_eq!(short_task_description("Verify with cargo check"), "Verify with cargo check");
        assert_eq!(short_task_description(""), "");
    }

    #[test]
    fn compact_task_tree_view_from_items_shortens_detail_tail() {
        let items = vec![json!({
            "index_path": "1",
            "description": "Add vtcode exec resume to the Commands section – document the cross-turn contract in the command table",
            "status": "pending",
        })];
        let rows = compact_task_tree_view_from_items(&items);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["display"], "  └ □ Add vtcode exec resume to the Commands section");
        assert_eq!(rows[0]["text"], "Add vtcode exec resume to the Commands section");
    }

    #[test]
    fn shared_schema_helpers_stay_actionable_and_drift_free() {
        assert_eq!(TASK_STATUS_VALUES, ["pending", "in_progress", "completed", "blocked"]);

        let status = task_status_schema();
        assert_eq!(status["type"], "string");
        assert_eq!(status["enum"], json!(TASK_STATUS_VALUES));
        assert!(status.get("description").is_none());

        let with_desc = task_status_property_schema("New status.");
        assert_eq!(with_desc["description"], "New status.");
        assert_eq!(with_desc["enum"], json!(TASK_STATUS_VALUES));

        // Asymmetric: plain string item vs structured item with metadata.
        let items_schema = task_items_array_schema("Items desc.");
        let validator = jsonschema::validator_for(&items_schema).expect("items schema is valid");
        assert!(validator.validate(&json!(["plain step"])).is_ok());
        assert!(
            validator
                .validate(&json!([{
                    "description": "Structured step",
                    "status": "in_progress",
                    "files": ["src/a.rs"],
                    "outcome": "Done",
                    "verify": ["cargo check"],
                }]))
                .is_ok()
        );
        // Boundary: verify also accepts a bare string; missing description fails.
        assert!(validator.validate(&json!([{"status": "completed"}])).is_err());
        assert!(validator.validate(&json!([{"description": "Bad", "status": "done"}])).is_err());

        assert_eq!(task_files_property_schema("F.")["description"], "F.");
        assert_eq!(task_verify_property_schema("V.")["description"], "V.");
    }

    #[test]
    fn shared_status_values_match_runtime_parser() {
        use std::str::FromStr;
        for status in TASK_STATUS_VALUES {
            assert!(TaskTrackingStatus::from_str(status).is_ok(), "schema status {status:?} must parse");
        }
        assert!(TaskTrackingStatus::from_str("done").is_err());
    }

    /// Drift guard: `TaskTrackingStatus` reaches the wire through three
    /// independent paths — the derived serde `snake_case` form, `as_str()`, and
    /// the hand-written `FromStr`. Nothing in the type system ties them
    /// together, so this locks the as_str <-> serde <-> FromStr triangle for
    /// every variant and pins the schema enum list to the same source. A
    /// one-sided edit to any path fails here instead of silently producing a
    /// value the runtime cannot parse back.
    #[test]
    fn task_status_as_str_serde_and_from_str_stay_in_lockstep() {
        use std::str::FromStr;
        let variants = [
            TaskTrackingStatus::Pending,
            TaskTrackingStatus::InProgress,
            TaskTrackingStatus::Completed,
            TaskTrackingStatus::Blocked,
        ];
        let strings: Vec<&str> = variants.iter().map(TaskTrackingStatus::as_str).collect();
        assert_eq!(strings, TASK_STATUS_VALUES);

        for variant in variants {
            let expected = variant.as_str();
            assert_eq!(
                serde_json::to_value(variant).expect("serializes"),
                json!(expected),
                "serde form drifted for {variant:?}"
            );
            assert_eq!(
                TaskTrackingStatus::from_str(expected).expect("as_str output must parse back"),
                variant,
                "FromStr drift for {variant:?}"
            );
        }
    }

    /// Cross-crate drift guard. `vtcode-commons::ui_protocol::TaskItemStatus`
    /// (the TUI-facing row status) is a distinct type from
    /// `TaskTrackingStatus` but must carry the *same* canonical strings — its
    /// doc comment promises they match, and the TUI parses runtime strings
    /// through it (`raw.parse::<TaskItemStatus>()`). The type system does not
    /// connect the two enums, so pin them token-for-token here.
    #[test]
    fn task_item_status_matches_tracking_status_strings() {
        use std::str::FromStr;
        use vtcode_commons::ui_protocol::TaskItemStatus;

        for variant in [
            TaskTrackingStatus::Pending,
            TaskTrackingStatus::InProgress,
            TaskTrackingStatus::Completed,
            TaskTrackingStatus::Blocked,
        ] {
            let canonical = variant.as_str();
            let display = TaskItemStatus::from_str(canonical)
                .unwrap_or_else(|_| panic!("TaskItemStatus must parse runtime status {canonical:?}"));
            assert_eq!(display.as_str(), canonical, "TaskItemStatus/TaskTrackingStatus string drift for {variant:?}");
        }
    }
}
