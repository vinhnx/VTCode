use serde_json::{Value, json};
use std::time::Duration;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::tools::tool_intent;

fn compact_loop_key_part(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

fn compact_loop_text(value: &str, max_chars: usize) -> String {
    compact_loop_key_part(
        &value.split_whitespace().collect::<Vec<_>>().join(" "),
        max_chars,
    )
}

fn unified_search_text_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| compact_loop_text(value, 120))
}

fn unified_search_pattern_arg(args: &Value) -> Option<String> {
    ["pattern", "keyword", "query"]
        .iter()
        .find_map(|key| unified_search_text_arg(args, key))
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

fn patch_source_arg(args: &Value) -> Option<&str> {
    args.as_str()
        .or_else(|| args.get("input").and_then(|v| v.as_str()))
        .or_else(|| args.get("patch").and_then(|v| v.as_str()))
}

fn extract_patch_target_path(patch_source: &str) -> Option<&str> {
    const PATCH_FILE_PREFIXES: [&str; 4] = [
        "*** Update File: ",
        "*** Add File: ",
        "*** Delete File: ",
        "*** Move to: ",
    ];

    patch_source.lines().find_map(|line| {
        PATCH_FILE_PREFIXES
            .iter()
            .find_map(|prefix| line.strip_prefix(prefix))
            .map(str::trim)
            .filter(|path| !path.is_empty())
    })
}

fn patch_signature(patch_source: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in patch_source.as_bytes().iter().take(2048) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("len{}-fnv{:016x}", patch_source.len(), hash)
}

fn read_file_path_arg(args: &Value) -> Option<&str> {
    let obj = args.as_object()?;
    for key in ["path", "file_path", "filepath", "target_path"] {
        if let Some(path) = obj.get(key).and_then(|v| v.as_str()) {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn unified_file_destination_arg(args: &Value) -> Option<&str> {
    let destination = args.get("destination").and_then(|v| v.as_str())?;
    let trimmed = destination.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
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

fn read_file_limit_value(args: &Value) -> Option<usize> {
    ["limit", "page_size_lines", "max_lines", "chunk_lines"]
        .iter()
        .filter_map(|key| args.get(*key))
        .find_map(|value| {
            value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .or_else(|| value.as_str().and_then(|s| s.parse::<usize>().ok()))
        })
}

pub(super) fn shell_run_signature(canonical_tool_name: &str, args: &Value) -> Option<String> {
    if !tool_intent::is_command_run_tool_call(canonical_tool_name, args) {
        return None;
    }

    let command = vtcode_core::tools::command_args::command_text(args)
        .ok()
        .flatten()?;
    Some(format!(
        "{}::{}",
        tool_names::UNIFIED_EXEC,
        compact_loop_key_part(&command, 200)
    ))
}

fn looks_like_tool_output_spool_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.contains(".vtcode/context/tool_outputs/")
}

fn is_read_file_style_call(canonical_tool_name: &str, args: &Value) -> bool {
    match canonical_tool_name {
        tool_names::READ_FILE => true,
        tool_names::UNIFIED_FILE => tool_intent::unified_file_action(args)
            .unwrap_or("read")
            .eq_ignore_ascii_case("read"),
        _ => false,
    }
}

pub(super) fn maybe_apply_spool_read_offset_hint(
    tool_registry: &mut ToolRegistry,
    canonical_tool_name: &str,
    args: &Value,
) -> Value {
    if !is_read_file_style_call(canonical_tool_name, args) {
        return args.clone();
    }

    let Some(path) = read_file_path_arg(args) else {
        return args.clone();
    };
    if !looks_like_tool_output_spool_path(path) {
        return args.clone();
    }

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

    Some("task_tracker::create".to_string())
}

pub(super) fn spool_chunk_read_path<'a>(
    canonical_tool_name: &str,
    args: &'a Value,
) -> Option<&'a str> {
    if !is_read_file_style_call(canonical_tool_name, args) {
        return None;
    }
    let path = read_file_path_arg(args)?;
    if looks_like_tool_output_spool_path(path) {
        Some(path)
    } else {
        None
    }
}

pub(super) fn loop_detection_tool_key(canonical_tool_name: &str, args: &Value) -> String {
    match canonical_tool_name {
        tool_names::READ_FILE => {
            let offset = args
                .get("offset")
                .or_else(|| args.get("offset_lines"))
                .or_else(|| args.get("offset_bytes"))
                .and_then(|v| {
                    v.as_u64()
                        .and_then(|n| usize::try_from(n).ok())
                        .or_else(|| v.as_str().and_then(|s| s.parse::<usize>().ok()))
                })
                .unwrap_or(1);
            let limit = read_file_limit_value(args).unwrap_or(0);
            if let Some(path) = read_file_path_arg(args) {
                return format!(
                    "{canonical_tool_name}::{}::offset={offset}::limit={limit}",
                    compact_loop_key_part(path, 120)
                );
            }
            format!("{canonical_tool_name}::offset={offset}::limit={limit}")
        }
        tool_names::UNIFIED_FILE => {
            let action = tool_intent::unified_file_action(args).unwrap_or("read");
            let action = action.to_ascii_lowercase();
            if action == "read"
                && let Some(path) = args
                    .get("path")
                    .or_else(|| args.get("file_path"))
                    .or_else(|| args.get("filepath"))
                    .or_else(|| args.get("target_path"))
                    .and_then(|v| v.as_str())
            {
                let offset = args
                    .get("offset")
                    .or_else(|| args.get("offset_lines"))
                    .or_else(|| args.get("offset_bytes"))
                    .and_then(|v| {
                        v.as_u64()
                            .and_then(|n| usize::try_from(n).ok())
                            .or_else(|| v.as_str().and_then(|s| s.parse::<usize>().ok()))
                    })
                    .unwrap_or(1);
                let limit = read_file_limit_value(args).unwrap_or(0);
                return format!(
                    "{canonical_tool_name}::{action}::{}::offset={offset}::limit={limit}",
                    compact_loop_key_part(path, 120),
                );
            }
            if action == "patch"
                && let Some(patch_source) = patch_source_arg(args)
            {
                let target = extract_patch_target_path(patch_source)
                    .map(|path| compact_loop_key_part(path, 120))
                    .unwrap_or_else(|| "<unknown>".to_string());
                return format!(
                    "{canonical_tool_name}::{action}::{target}::{}",
                    patch_signature(patch_source)
                );
            }
            if matches!(
                action.as_str(),
                "edit" | "write" | "delete" | "move" | "copy"
            ) {
                let source = read_file_path_arg(args).map(|path| compact_loop_key_part(path, 120));
                let destination =
                    unified_file_destination_arg(args).map(|path| compact_loop_key_part(path, 120));
                return match (source, destination) {
                    (Some(src), Some(dest)) => {
                        format!("{canonical_tool_name}::{action}::{src}->{dest}")
                    }
                    (Some(src), None) => format!("{canonical_tool_name}::{action}::{src}"),
                    (None, Some(dest)) => {
                        format!("{canonical_tool_name}::{action}::destination={dest}")
                    }
                    (None, None) => format!("{canonical_tool_name}::{action}"),
                };
            }
            format!("{canonical_tool_name}::{action}")
        }
        tool_names::APPLY_PATCH => {
            if let Some(patch_source) = patch_source_arg(args) {
                let target = extract_patch_target_path(patch_source)
                    .map(|path| compact_loop_key_part(path, 120))
                    .unwrap_or_else(|| "<unknown>".to_string());
                return format!(
                    "{canonical_tool_name}::{target}::{}",
                    patch_signature(patch_source)
                );
            }
            canonical_tool_name.to_string()
        }
        tool_names::UNIFIED_EXEC => {
            let action = tool_intent::unified_exec_action(args).unwrap_or("run");
            let action = action.to_ascii_lowercase();
            if matches!(action.as_str(), "poll" | "continue" | "close" | "inspect")
                && let Some(session_id) = args.get("session_id").and_then(|v| v.as_str())
            {
                if action == "continue"
                    && let Some(input) = args
                        .get("input")
                        .or_else(|| args.get("chars"))
                        .or_else(|| args.get("text"))
                        .and_then(|v| v.as_str())
                {
                    return format!(
                        "{canonical_tool_name}::{action}::{}::{}",
                        compact_loop_key_part(session_id, 80),
                        compact_loop_key_part(input, 40)
                    );
                }
                return format!(
                    "{canonical_tool_name}::{action}::{}",
                    compact_loop_key_part(session_id, 80)
                );
            }
            if action == "inspect"
                && let Some(spool_path) = args.get("spool_path").and_then(|v| v.as_str())
            {
                return format!(
                    "{canonical_tool_name}::{action}::{}",
                    compact_loop_key_part(spool_path, 120)
                );
            }
            if action == "run"
                && let Some(command) = args
                    .get("command")
                    .or_else(|| args.get("cmd"))
                    .or_else(|| args.get("raw_command"))
                    .and_then(|v| v.as_str())
            {
                return format!(
                    "{canonical_tool_name}::{action}::{}",
                    compact_loop_key_part(command, 120)
                );
            }
            format!("{canonical_tool_name}::{action}")
        }
        tool_names::UNIFIED_SEARCH => {
            let normalized = tool_intent::normalize_unified_search_args(args);
            let action = tool_intent::unified_search_action(&normalized).unwrap_or("grep");
            let action = action.to_ascii_lowercase();
            let path = normalized
                .get("path")
                .and_then(Value::as_str)
                .map(|value| compact_loop_key_part(value, 120))
                .unwrap_or_else(|| ".".to_string());
            match action.as_str() {
                "list" => {
                    let mode = normalized
                        .get("mode")
                        .and_then(Value::as_str)
                        .map(|value| compact_loop_key_part(value, 32))
                        .unwrap_or_else(|| "list".to_string());
                    format!("{canonical_tool_name}::{action}::{path}::mode={mode}")
                }
                "structural" => {
                    let pattern = unified_search_pattern_arg(&normalized)
                        .unwrap_or_else(|| "<missing-pattern>".to_string());
                    let mut key = format!("{canonical_tool_name}::{action}::{path}::{pattern}");
                    if let Some(lang) = unified_search_text_arg(&normalized, "lang") {
                        key.push_str("::lang=");
                        key.push_str(&compact_loop_key_part(&lang, 32));
                    }
                    if let Some(selector) = unified_search_text_arg(&normalized, "selector") {
                        key.push_str("::selector=");
                        key.push_str(&compact_loop_key_part(&selector, 64));
                    }
                    if let Some(strictness) = unified_search_text_arg(&normalized, "strictness") {
                        key.push_str("::strictness=");
                        key.push_str(&compact_loop_key_part(&strictness, 32));
                    }
                    if let Some(debug_query) = unified_search_text_arg(&normalized, "debug_query") {
                        key.push_str("::debug_query=");
                        key.push_str(&compact_loop_key_part(&debug_query, 32));
                    }
                    if let Some(globs) = unified_search_globs_arg(&normalized) {
                        key.push_str("::globs=");
                        key.push_str(&globs);
                    }
                    key
                }
                _ => {
                    let pattern = unified_search_pattern_arg(&normalized)
                        .unwrap_or_else(|| "<missing-pattern>".to_string());
                    let mut key = format!("{canonical_tool_name}::{action}::{path}::{pattern}");
                    if let Some(globs) = unified_search_globs_arg(&normalized) {
                        key.push_str("::globs=");
                        key.push_str(&globs);
                    }
                    key
                }
            }
        }
        _ => canonical_tool_name.to_string(),
    }
}
