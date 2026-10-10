//! Token-efficient tool result reducers.
//!
//! When tool results are too large for the context window, reducers truncate
//! them to keep only high-signal information. This follows the context
//! engineering principle: "return only summaries or a small number of results
//! to the model."

use std::borrow::Cow;

use serde_json::Value;

use crate::config::constants::tools;
use crate::tools::tool_intent;

/// Reduce a tool result to be more token-efficient.
///
/// Dispatches to the appropriate reducer based on the tool name.
pub fn reduce_tool_result(tool_name: &str, result: Value) -> Value {
    let canonical_tool_name = tool_intent::canonical_command_session_tool_name(tool_name).unwrap_or(tool_name);
    match canonical_tool_name {
        tools::READ_FILE => reduce_read_file_result(result),
        tools::UNIFIED_EXEC => reduce_command_result(result),
        _ => result,
    }
}

/// Strip TUI-only display fields from a tool result before it enters model
/// context.
///
/// The `view` field produced by `task_tracker` (and its planning-workflow
/// variant) is rendered only by the TUI — branch symbols, status icons, and
/// per-item display lines that duplicate the structured `checklist` already
/// present in the same payload. Sending it to the model wastes tokens on every
/// tracker call, and the waste grows as items accumulate `outcome`/`verify`
/// metadata (observed growing from ~5 KB to ~12 KB per call in session logs,
/// with `view` accounting for ~3 KB of each).
///
/// The TUI reads `view` from the original tool-output `Value` via the
/// pipeline-output / event path, not from the model-facing string this is
/// applied to, so removing it here is display-safe.
///
/// Returns a borrowed value when no stripping is needed (non-tracker tools or
/// results without a `view` field) so the common path pays zero allocation.
pub fn strip_tui_display_fields<'a>(tool_name: &str, value: &'a Value) -> Cow<'a, Value> {
    let canonical = tool_intent::canonical_command_session_tool_name(tool_name).unwrap_or(tool_name);
    if canonical != tools::TASK_TRACKER && canonical != tools::MATRIX {
        return Cow::Borrowed(value);
    }
    let Some(obj) = value.as_object() else {
        return Cow::Borrowed(value);
    };
    if !obj.contains_key("view") {
        return Cow::Borrowed(value);
    }
    let mut stripped = obj.clone();
    stripped.remove("view");
    Cow::Owned(Value::Object(stripped))
}

/// Project an indexed tracker update without repeating unchanged task details.
/// Full results remain available to persistence, events, and explicit list calls.
pub fn project_model_tool_result<'a>(tool_name: &str, args: &Value, value: &'a Value) -> Cow<'a, Value> {
    let mut projected = strip_tui_display_fields(tool_name, value);
    if tool_name != tools::TASK_TRACKER
        || args.get("action").and_then(Value::as_str) != Some("update")
        || args.get("items").is_some()
    {
        return projected;
    }
    let index_path = args
        .get("index_path")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| args.get("index").and_then(Value::as_u64).map(|index| index.to_string()));
    let Some(index_path) = index_path else {
        return projected;
    };
    let Some(changed_item) = value
        .get("checklist")
        .and_then(|checklist| checklist.get("items"))
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("index_path")
                    .and_then(Value::as_str)
                    .is_some_and(|path| path == index_path)
                    || item
                        .get("index")
                        .and_then(Value::as_u64)
                        .is_some_and(|index| index.to_string() == index_path)
            })
        })
        .cloned()
    else {
        return projected;
    };
    if let Some(object) = projected.to_mut().as_object_mut() {
        if let Some(checklist) = object.get_mut("checklist").and_then(Value::as_object_mut) {
            checklist.remove("items");
        }
        object.insert("changed_item".to_owned(), changed_item);
    }
    projected
}

/// Hard byte cap on model-visible content. Line truncation alone does not
/// bound output with very long lines (minified bundles, generated data), so
/// the provider-visible preview contract needs a byte budget as well.
const MAX_RESULT_BYTES: usize = 32 * 1024;

fn truncate_utf8_bytes(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    (vtcode_commons::formatting::truncate_utf8_prefix(text, max_bytes).to_string(), true)
}

fn reduce_read_file_result(result: Value) -> Value {
    const MAX_FILE_LINES: usize = 2000;

    let Some(obj) = result.as_object() else {
        return result;
    };
    let Some(content) = obj.get("content").and_then(Value::as_str) else {
        return result;
    };

    let (line_truncated, content) = match truncate_lines(content, MAX_FILE_LINES) {
        Some((truncated, _)) => (true, truncated),
        None => (false, content.to_string()),
    };
    let (content, byte_truncated) = truncate_utf8_bytes(&content, MAX_RESULT_BYTES);
    let is_truncated = line_truncated || byte_truncated;
    if !is_truncated {
        // Keep the full payload: read results carry continuation fields
        // (`has_more`, `next_read_args`, `spool_path`, …) that a whitelist
        // rebuild would drop and break paging.
        return result;
    }

    let mut reduced = obj.clone();
    reduced.insert("content".to_string(), Value::String(content));
    reduced.insert("is_truncated".to_string(), Value::Bool(true));
    // next_read_args.offset still points past the *original* chunk. Without
    // this flag the model would assume the page is complete and skip the
    // byte-capped tail.
    if reduced.contains_key("next_read_args") || reduced.get("has_more").and_then(Value::as_bool) == Some(true) {
        reduced.insert("chunk_tail_omitted".to_string(), Value::Bool(true));
    }
    if line_truncated {
        reduced.insert("note".to_string(), Value::String("File content truncated for context economy.".to_string()));
    }

    Value::Object(reduced)
}

fn reduce_command_result(result: Value) -> Value {
    const MAX_FILE_LINES: usize = 2000;

    let Some(obj) = result.as_object() else {
        return result;
    };
    let stream_key = if obj.get("stdout").and_then(Value::as_str).is_some() {
        "stdout"
    } else {
        "output"
    };
    let Some(stream) = obj.get(stream_key).and_then(Value::as_str) else {
        return result;
    };

    let (line_truncated, lines_count, stream) = match truncate_lines(stream, MAX_FILE_LINES) {
        Some((truncated, lines_count)) => (true, lines_count, truncated),
        None => (false, 0, stream.to_string()),
    };
    let (stream, byte_truncated) = truncate_utf8_bytes(&stream, MAX_RESULT_BYTES);
    if !line_truncated && !byte_truncated {
        return result;
    }

    let mut reduced = obj.clone();
    reduced.insert(stream_key.to_string(), Value::String(stream));
    reduced.insert("is_truncated".to_string(), Value::Bool(true));
    if line_truncated {
        reduced.insert("original_lines".to_string(), Value::Number(serde_json::Number::from(lines_count as u64)));
    }
    reduced.insert("note".to_string(), Value::String("Command output truncated for context economy.".to_string()));
    Value::Object(reduced)
}

pub fn truncate_lines(text: &str, max_lines: usize) -> Option<(String, usize)> {
    if max_lines == 0 {
        return Some((String::new(), text.lines().count()));
    }

    let mut lines = text.lines();
    let mut total = 0usize;
    let mut out = String::new();
    while let Some(line) = lines.next() {
        total += 1;
        if total <= max_lines {
            if total > 1 {
                out.push('\n');
            }
            out.push_str(line);
            continue;
        }
        total += lines.count();
        return Some((out, total));
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn tracker_projection_keeps_only_the_changed_hierarchical_item() {
        let original = serde_json::json!({
            "status": "updated",
            "checklist": {"total": 2, "completed": 1, "items": [
                {"index_path": "1", "description": "Parent", "status": "pending"},
                {"index_path": "1.1", "description": "Child", "status": "completed", "verify": ["cargo check"]}
            ]},
            "view": {"lines": ["Parent", "Child"]}
        });
        let projected = project_model_tool_result(
            "task_tracker",
            &serde_json::json!({"action": "update", "index_path": "1.1"}),
            &original,
        );
        assert_eq!(projected["changed_item"]["description"], "Child");
        assert_eq!(projected["changed_item"]["verify"], serde_json::json!(["cargo check"]));
        assert_eq!(projected["checklist"]["total"], 2);
        assert!(projected["checklist"].get("items").is_none());
        assert!(projected.get("view").is_none());
        assert_eq!(original["checklist"]["items"].as_array().unwrap().len(), 2);
        assert!(original.get("view").is_some());
        assert!(projected.to_string().len() < original.to_string().len());
        for args in [
            serde_json::json!({"action": "list"}),
            serde_json::json!({"action": "create"}),
            serde_json::json!({"action": "update", "items": ["Replacement"]}),
            serde_json::json!({"action": "update", "index_path": "9"}),
        ] {
            let projected = project_model_tool_result("task_tracker", &args, &original);
            assert_eq!(projected["checklist"]["items"], original["checklist"]["items"]);
            assert!(projected.get("changed_item").is_none());
        }
    }

    #[test]
    fn tracker_projection_supports_standard_indices() {
        let result = serde_json::json!({"checklist": {"items": [
            {"index": 1, "description": "First"}, {"index": 2, "description": "Second"}
        ]}});
        let projected =
            project_model_tool_result("task_tracker", &serde_json::json!({"action": "update", "index": 2}), &result);
        assert_eq!(projected["changed_item"]["description"], "Second");
        assert_eq!(project_model_tool_result("exec_command", &serde_json::json!({}), &result).as_ref(), &result);
    }
    use super::*;
    use serde_json::json;

    #[test]
    fn strip_view_removes_view_from_task_tracker_result() {
        let result = json!({
            "status": "updated",
            "message": "Item 1 status changed: pending → completed",
            "checklist": { "title": "Demo", "total": 2, "completed": 1, "items": [] },
            "view": { "title": "Demo", "lines": [{ "display": "└ [x] step one" }] },
        });
        let stripped = strip_tui_display_fields("task_tracker", &result);
        assert!(stripped.get("view").is_none(), "view should be removed");
        assert!(stripped.get("checklist").is_some(), "checklist must remain for the model");
        assert_eq!(stripped["status"], "updated");
    }

    #[test]
    fn strip_view_borrows_non_tracker_tools_unchanged() {
        let result = json!({ "status": "ok", "output": "hello" });
        let stripped = strip_tui_display_fields("exec_command", &result);
        assert!(matches!(stripped, Cow::Borrowed(_)), "non-tracker tools should borrow without allocation");
        assert_eq!(stripped.as_ref(), &result);
    }

    #[test]
    fn strip_view_borrows_tracker_result_without_view_field() {
        let result = json!({ "status": "empty", "message": "No active checklist." });
        let stripped = strip_tui_display_fields("task_tracker", &result);
        assert!(
            matches!(stripped, Cow::Borrowed(_)),
            "tracker results without `view` should borrow without allocation"
        );
        assert_eq!(stripped.as_ref(), &result);
    }

    #[test]
    fn strip_view_preserves_checklist_items_and_metadata() {
        let result = json!({
            "status": "ok",
            "checklist": {
                "title": "Plan",
                "total": 2,
                "completed": 0,
                "items": [
                    { "index": 1, "description": "Step A", "status": "pending", "files": ["a.rs"], "outcome": null, "verify": ["cargo check"] },
                    { "index": 2, "description": "Step B", "status": "pending" },
                ],
            },
            "view": { "title": "Plan", "lines": [{ "display": "├ [ ] Step A" }] },
        });
        let stripped = strip_tui_display_fields("task_tracker", &result);
        let items = stripped["checklist"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["verify"][0], "cargo check");
        assert_eq!(items[1]["description"], "Step B");
        assert!(stripped.get("view").is_none());
    }

    #[test]
    fn strip_view_handles_non_object_result() {
        let result = json!("not an object");
        let stripped = strip_tui_display_fields("task_tracker", &result);
        assert_eq!(stripped.as_ref(), &result);
    }

    #[test]
    fn reduce_read_file_preserves_continuation_fields_when_not_truncated() {
        let result = json!({
            "success": true,
            "content": "short body",
            "path": "src/lib.rs",
            "has_more": true,
            "next_read_args": { "path": "src/lib.rs", "offset": 80 },
            "spool_path": ".vtcode/context/tool_outputs/x",
            "spooled_bytes": 999,
        });
        let reduced = reduce_tool_result("read_file", result.clone());
        assert_eq!(reduced, result, "untruncated read results must stay intact for paging");
    }

    #[test]
    fn reduce_read_file_truncates_large_content_but_keeps_next_read_args() {
        let content = "a\n".repeat(2_500);
        let result = json!({
            "success": true,
            "content": content,
            "path": "src/lib.rs",
            "has_more": true,
            "next_read_args": { "path": "src/lib.rs", "offset": 2500 },
        });
        let reduced = reduce_tool_result("read_file", result);
        assert_eq!(reduced["is_truncated"], json!(true));
        assert!(reduced["content"].as_str().unwrap().lines().count() < 2_500, "content must be line-capped");
        assert_eq!(reduced["has_more"], json!(true), "paging fields survive truncation");
        assert!(reduced.get("next_read_args").is_some(), "next_read_args must survive truncation");
        assert_eq!(
            reduced["chunk_tail_omitted"],
            json!(true),
            "byte-capped page must warn that the chunk tail is not in content"
        );
    }
}
