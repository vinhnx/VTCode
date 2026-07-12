//! Token-efficient tool result reducers.
//!
//! When tool results are too large for the context window, reducers truncate
//! them to keep only high-signal information. This follows the context
//! engineering principle: "return only summaries or a small number of results
//! to the model."

use std::collections::HashSet;

use serde_json::Value;

use crate::config::constants::tools;
use crate::tools::tool_intent;

/// Reduce a tool result to be more token-efficient.
///
/// Dispatches to the appropriate reducer based on the tool name.
pub fn reduce_tool_result(tool_name: &str, result: Value) -> Value {
    let canonical_tool_name =
        tool_intent::canonical_unified_exec_tool_name(tool_name).unwrap_or(tool_name);
    match canonical_tool_name {
        tools::UNIFIED_SEARCH => reduce_search_result(result),
        tools::READ_FILE => reduce_read_file_result(result),
        tools::UNIFIED_EXEC => reduce_command_result(result),
        _ => result,
    }
}

fn reduce_search_result(result: Value) -> Value {
    const MAX_GREP_RESULTS: usize = 5;
    const MAX_LIST_FILES: usize = 50;

    let Some(obj) = result.as_object() else {
        return result;
    };

    if let Some(matches) = obj.get("matches").and_then(Value::as_array) {
        let mut deduped = Vec::with_capacity(matches.len());
        let mut seen = HashSet::new();
        for entry in matches {
            let path = entry
                .get("path")
                .or_else(|| entry.get("file"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let line = entry
                .get("line")
                .or_else(|| entry.get("line_number"))
                .and_then(Value::as_i64);
            if path.is_none() && line.is_none() {
                deduped.push(entry.clone());
                continue;
            }
            if seen.insert((path, line)) {
                deduped.push(entry.clone());
            }
        }
        let total = deduped.len();
        if total > MAX_GREP_RESULTS {
            return serde_json::json!({
                "matches": deduped.into_iter().take(MAX_GREP_RESULTS).collect::<Vec<_>>(),
                "overflow": format!("[+{} more matches]", total - MAX_GREP_RESULTS),
                "total": total,
                "note": "Showing top 5 unique matches (by path/line)"
            });
        }
        if total != matches.len() {
            return serde_json::json!({
                "matches": deduped,
                "total": total,
                "note": "unique grep matches (collapsed by path/line)"
            });
        }
        return serde_json::json!({
            "matches": deduped,
            "total": total,
            "note": "grep results normalized"
        });
    }

    let Some(files) = obj
        .get("files")
        .or_else(|| obj.get("items"))
        .and_then(Value::as_array)
    else {
        return result;
    };
    if files.len() <= MAX_LIST_FILES {
        return result;
    }

    serde_json::json!({
        "total_files": files.len(),
        "sample": files.iter().take(5).cloned().collect::<Vec<_>>(),
        "note": format!("Showing 5 of {} files. Use unified_search for specific patterns.", files.len())
    })
}

fn reduce_read_file_result(result: Value) -> Value {
    const MAX_FILE_LINES: usize = 2000;

    let Some(obj) = result.as_object() else {
        return result;
    };
    let Some(content) = obj.get("content").and_then(Value::as_str) else {
        return result;
    };

    let (content, is_truncated) = truncate_lines(content, MAX_FILE_LINES)
        .map(|(truncated, _)| (truncated, true))
        .unwrap_or_else(|| (content.to_string(), false));

    let mut reduced = serde_json::Map::new();
    reduced.insert("success".to_string(), Value::Bool(true));
    reduced.insert(
        "status".to_string(),
        obj.get("status")
            .cloned()
            .unwrap_or_else(|| Value::String("success".to_string())),
    );
    if let Some(message) = obj.get("message") {
        reduced.insert("message".to_string(), message.clone());
    }
    reduced.insert("content".to_string(), Value::String(content));
    if let Some(path) = obj.get("path").or_else(|| obj.get("file")) {
        reduced.insert("path".to_string(), path.clone());
    }
    if let Some(metadata) = obj.get("metadata") {
        reduced.insert("metadata".to_string(), metadata.clone());
    }
    if is_truncated {
        reduced.insert("is_truncated".to_string(), Value::Bool(true));
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
    let Some((truncated, lines_count)) = truncate_lines(stream, MAX_FILE_LINES) else {
        return result;
    };

    let mut reduced = obj.clone();
    reduced.insert(stream_key.to_string(), Value::String(truncated));
    reduced.insert("is_truncated".to_string(), Value::Bool(true));
    reduced.insert(
        "original_lines".to_string(),
        Value::Number(serde_json::Number::from(lines_count as u64)),
    );
    reduced.insert(
        "note".to_string(),
        Value::String("Command output truncated for context economy.".to_string()),
    );
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
