use std::str::FromStr;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskStepMetadata {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verify: Vec<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
            other => bail!(
                "Invalid status '{}'. Use: pending, in_progress, completed, blocked",
                other
            ),
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
    parse_marked_status_prefix(value)
        .unwrap_or((TaskTrackingStatus::Pending, value.trim_start().to_string()))
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

pub fn is_bulk_sync_update<T>(
    items: Option<&[T]>,
    index: Option<usize>,
    index_path: Option<&str>,
    status: Option<&str>,
) -> bool {
    items.is_some() && ((index.is_none() && index_path.is_none()) || status.is_none())
}

pub fn deserialize_optional_string_list<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<String>>, D::Error>
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
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
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
    TaskStepMetadata {
        files: normalize_string_items(files),
        outcome: normalize_optional_text(outcome),
        verify: normalize_string_items(verify),
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
            (self.completed as f64 / self.total as f64 * 100.0).round() as usize
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn is_bulk_sync_update_requires_items_and_missing_single_item_fields() {
        let items = vec!["Step".to_string()];
        assert!(is_bulk_sync_update(Some(&items), None, None, None));
        assert!(!is_bulk_sync_update(
            Some(&items),
            Some(1),
            None,
            Some("completed")
        ));
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
}
