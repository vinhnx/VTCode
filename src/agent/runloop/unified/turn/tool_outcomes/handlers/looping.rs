use serde_json::{Value, json};
use std::time::Duration;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::tools::tool_intent;

use crate::agent::runloop::unified::tool_reads::{is_read_file_style_call, read_file_path_arg};

pub(super) use crate::agent::runloop::unified::tool_reads::spool_chunk_read_path;

fn compact_loop_key_part(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

fn compact_loop_text(value: &str, max_chars: usize) -> String {
    compact_loop_key_part(
        &value.split_whitespace().collect::<Vec<_>>().join(" "),
        max_chars,
    )
}

fn normalize_shell_command_text(value: &str, max_chars: usize) -> String {
    compact_loop_text(
        &value
            .chars()
            .filter(|ch| !matches!(ch, '\'' | '"'))
            .collect::<String>(),
        max_chars,
    )
}

fn normalized_shell_command_arg(args: &Value, max_chars: usize) -> Option<String> {
    vtcode_core::tools::command_args::command_text(args)
        .ok()
        .flatten()
        .map(|command| normalize_shell_command_text(&command, max_chars))
        .filter(|command| !command.is_empty())
}

fn unified_search_globs_arg(args: &Value) -> Option<String> {
    let globs = args.get("globs")?;
    match globs {
        Value::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(compact_loop_text(trimmed, 120))
            }
        }
        Value::Array(items) => {
            let joined = items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join(",");
            if joined.is_empty() {
                None
            } else {
                Some(compact_loop_text(&joined, 120))
            }
        }
        _ => None,
    }
}

fn read_file_has_offset_arg(args: &Value) -> bool {
    ["offset", "offset_lines", "offset_bytes"]
        .iter()
        .any(|key| args.get(*key).is_some())
}

fn read_file_offset_value(args: &Value) -> Option<usize> {
    ["offset", "offset_lines", "offset_bytes"]
        .iter()
        .filter_map(|key| args.get(*key))
        .find_map(|value| {
            value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .or_else(|| value.as_str().and_then(|s| s.parse::<usize>().ok()))
        })
}

fn read_file_has_limit_arg(args: &Value) -> bool {
    ["limit", "page_size_lines", "max_lines", "chunk_lines"]
        .iter()
        .any(|key| args.get(*key).is_some())
}

pub(super) fn shell_run_signature(canonical_tool_name: &str, args: &Value) -> Option<String> {
    if !tool_intent::is_command_run_tool_call(canonical_tool_name, args) {
        return None;
    }

    let command = normalized_shell_command_arg(args, 200)?;
    Some(format!("{}::{}", tool_names::UNIFIED_EXEC, command))
}

pub(super) fn maybe_apply_spool_read_offset_hint(
    tool_registry: &mut ToolRegistry,
    canonical_tool_name: &str,
    args: &Value,
) -> Value {
    if !is_read_file_style_call(canonical_tool_name, args) {
        return args.clone();
    }

    let Some(path) = spool_chunk_read_path(canonical_tool_name, args) else {
        return args.clone();
    };

    let Some((next_offset, chunk_limit)) =
        tool_registry.find_recent_read_file_spool_progress(path, Duration::from_secs(180))
    else {
        return args.clone();
    };

    let requested_offset = read_file_offset_value(args);
    let should_advance_offset = match requested_offset {
        Some(existing) => existing < next_offset,
        None => true,
    };
    let should_fill_offset = !read_file_has_offset_arg(args);

    let mut adjusted = args.clone();
    let keep_existing_limit = read_file_has_limit_arg(&adjusted);
    if let Some(obj) = adjusted.as_object_mut() {
        if should_fill_offset || should_advance_offset {
            obj.insert("offset".to_string(), json!(next_offset));
        }
        if !keep_existing_limit {
            obj.insert("limit".to_string(), json!(chunk_limit));
        }
        if should_fill_offset || should_advance_offset || !keep_existing_limit {
            tracing::debug!(
                tool = canonical_tool_name,
                path = path,
                requested_offset = requested_offset.unwrap_or(0),
                next_offset,
                chunk_limit,
                "Applied spool read continuation hint to avoid repeated identical chunk reads"
            );
        }
    }
    adjusted
}

pub(super) fn task_tracker_create_signature(tool_name: &str, args: &Value) -> Option<String> {
    if tool_name != tool_names::TASK_TRACKER {
        return None;
    }

    let action = args.get("action").and_then(Value::as_str)?;
    if action != "create" {
        return None;
    }

    #[derive(serde::Serialize)]
    struct TaskTrackerCreateSignature<'a> {
        title: Option<&'a Value>,
        items: Option<&'a Value>,
        notes: Option<&'a Value>,
    }

    let payload = TaskTrackerCreateSignature {
        title: args.get("title"),
        items: args.get("items"),
        notes: args.get("notes"),
    };
    let payload_str = serde_json::to_string(&payload).ok()?;
    let mut signature = String::with_capacity("task_tracker::create::".len() + payload_str.len());
    signature.push_str("task_tracker::create::");
    signature.push_str(&payload_str);

    Some(signature)
}
pub(crate) fn low_signal_family_key(canonical_tool_name: &str, args: &Value) -> Option<String> {
    match canonical_tool_name {
        tool_names::READ_FILE => read_file_path_arg(args).map(|path| {
            format!(
                "{canonical_tool_name}::{}",
                compact_loop_key_part(path, 120)
            )
        }),
        tool_names::UNIFIED_FILE => {
            let action = tool_intent::unified_file_action(args).unwrap_or("read");
            if !action.eq_ignore_ascii_case("read") {
                return None;
            }
            read_file_path_arg(args).map(|path| {
                format!(
                    "{canonical_tool_name}::read::{}",
                    compact_loop_key_part(path, 120)
                )
            })
        }
        tool_names::UNIFIED_EXEC => normalized_shell_command_arg(args, 160)
            .map(|command| format!("{canonical_tool_name}::run::{command}")),
        tool_names::UNIFIED_SEARCH => {
            let normalized = tool_intent::normalize_unified_search_args(args);
            let mut key = canonical_tool_name.to_string();
            if let Some(globs) = unified_search_globs_arg(&normalized) {
                key.push_str("::globs=");
                key.push_str(&globs);
            } else {
                let path = normalized
                    .get("path")
                    .and_then(Value::as_str)
                    .map(|value| compact_loop_key_part(value, 120))
                    .unwrap_or_else(|| ".".to_string());
                key.push_str("::");
                key.push_str(&path);
            }
            Some(key)
        }
        _ => None,
    }
}
