use crate::exec::events::{ItemCompletedEvent, ThreadEvent, ThreadItem, ThreadItemDetails};
use crate::tools::file_ops::canonical_diff_previews;
use serde_json::Value;

/// Attribute only a successful file tool's recorded result, never current Git state.
pub fn file_change_completed_event(
    call_id: &str,
    tool_name: &str,
    args: &Value,
    output: &Value,
) -> Option<ThreadEvent> {
    use crate::exec::events::{FileChangeItem, FileUpdateChange, PatchApplyStatus, PatchChangeKind};
    let file_tool = matches!(
        tool_name,
        "apply_patch" | "write_file" | "edit_file" | "create_file" | "delete_file" | "move_file" | "copy_file" | "file"
    ) || tool_name == crate::config::constants::tools::UNIFIED_FILE;
    if !file_tool
        || !crate::tools::tool_intent::classify_tool_intent(tool_name, args).mutating
        || output.get("success").and_then(Value::as_bool) == Some(false)
        || output.get("skipped").and_then(Value::as_bool) == Some(true)
        || output.get("conflict").and_then(Value::as_bool) == Some(true)
        || crate::tools::file_ops::diff_output_has_effective_change(output) == Some(false)
    {
        return None;
    }
    let previews = canonical_diff_previews(output);
    let mut changes = Vec::new();
    let mut diffs = Vec::new();
    let mut additions = 0u64;
    let mut deletions = 0u64;
    let mut counts_available = true;
    let mut diff_incomplete = previews.is_empty();
    for preview in &previews {
        if preview.get("is_empty").and_then(Value::as_bool) == Some(true)
            && !matches!(preview.get("operation").and_then(Value::as_str), Some("created" | "deleted"))
        {
            continue;
        }
        let Some(path) = preview.get("path").and_then(Value::as_str) else {
            diff_incomplete = true;
            continue;
        };
        let kind = match preview.get("operation").and_then(Value::as_str) {
            Some("created" | "added") => PatchChangeKind::Add,
            Some("deleted" | "removed") => PatchChangeKind::Delete,
            _ => PatchChangeKind::Update,
        };
        changes.push(FileUpdateChange {
            path: path.to_owned(),
            kind,
            terminators_normalized: preview.get("normalized_terminators").and_then(Value::as_bool),
        });
        diff_incomplete |= preview.get("truncated").and_then(Value::as_bool) == Some(true)
            || preview.get("skipped").and_then(Value::as_bool) == Some(true)
            || preview.get("content").and_then(Value::as_str).is_none();
        if let Some(content) = preview.get("content").and_then(Value::as_str) {
            diffs.push(content.to_owned());
        }
        if let (Some(a), Some(d)) = (
            preview
                .get("additions")
                .or_else(|| preview.get("summary").and_then(|s| s.get("additions")))
                .and_then(Value::as_u64),
            preview
                .get("deletions")
                .or_else(|| preview.get("summary").and_then(|s| s.get("deletions")))
                .and_then(Value::as_u64),
        ) {
            additions = additions.saturating_add(a);
            deletions = deletions.saturating_add(d);
        } else {
            counts_available = false;
        }
    }
    if changes.is_empty()
        && let Some(paths) = output.get("modified_files").and_then(Value::as_array)
    {
        counts_available = false;
        for path in paths.iter().filter_map(Value::as_str) {
            changes.push(FileUpdateChange {
                path: path.to_owned(),
                kind: PatchChangeKind::Update,
                terminators_normalized: None,
            });
        }
    }
    if changes.is_empty() && diff_incomplete && output.get("success").and_then(Value::as_bool) == Some(true) {
        // Successful file operations without a diff still need a durable
        // refusal marker: otherwise Undo can silently reverse only the
        // other, recorded changes in the same turn.
        for path in ["path", "from", "to"]
            .into_iter()
            .filter_map(|key| output.get(key).and_then(Value::as_str))
        {
            if !path.is_empty() && !changes.iter().any(|change| change.path == path) {
                changes.push(FileUpdateChange {
                    path: path.to_owned(),
                    kind: PatchChangeKind::Update,
                    terminators_normalized: None,
                });
            }
        }
        diff_incomplete = true;
    } else if changes.is_empty() {
        return None;
    }
    Some(ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: format!("{call_id}:changes"),
            context: None,
            details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                diff_incomplete: diff_incomplete.then_some(true),
                changes,
                status: PatchApplyStatus::Completed,
                unified_diff: (!diffs.is_empty()).then(|| diffs.join("\n")),
                additions: counts_available.then_some(additions),
                deletions: counts_available.then_some(deletions),
            })),
        },
    }))
}
