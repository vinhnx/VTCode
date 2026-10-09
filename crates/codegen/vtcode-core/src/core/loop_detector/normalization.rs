//! Pure helper functions for loop detection: tool classification, argument
//! normalization, and read-target extraction.
//!
//! These functions have no dependency on [`LoopDetector`](super::LoopDetector)
//! state, which keeps them independently unit-testable and keeps the detector
//! facade focused on state + budget bookkeeping. The root module re-exports
//! them via `pub(crate) use self::normalization::*;` so existing call sites and
//! tests keep working unchanged.

use crate::config::constants::tools;
use crate::tools::tool_intent;

use super::LEGACY_LIST_FILES;

/// Return the base tool name, stripping a `::action` suffix (e.g.
/// `unified_file::read` -> `unified_file`).
#[inline]
pub(super) fn base_tool_name(tool_name: &str) -> &str {
    tool_name.split_once("::").map(|(base, _)| base).unwrap_or(tool_name)
}

pub(super) use tool_intent::is_command_session_tool as is_command_tool_name;

/// Returns `true` when the tool is a file-mutating tool. Write/edit tools are
/// excluded from identical-call enforcement because their normalized hash nulls
/// the content payload, so distinct edits to the same path would collide.
#[inline]
pub(super) fn is_write_tool_name(tool_name: &str) -> bool {
    let base_name = base_tool_name(tool_name);
    matches!(
        base_name,
        tools::WRITE_FILE
            | tools::CREATE_FILE
            | tools::EDIT_FILE
            | tools::APPLY_PATCH
            | tools::DELETE_FILE
            | tools::MOVE_FILE
            | tools::COPY_FILE
            | tools::SEARCH_REPLACE
    ) || (base_name == tools::UNIFIED_FILE && !tool_name.ends_with("::read"))
}

/// Canonicalize shell commands for loop detection.
///
/// Collapses semantically equivalent verification and read commands so the
/// identical-call detector catches patterns like:
/// - `command -v ast-grep` / `which ast-grep` / `ast-grep --help` → `__verify__:ast-grep`
/// - `cat file.txt` / `head file.txt` → `__read__:file.txt`
///
/// Returns `None` if the command is not a recognized pattern (pass through unchanged).
pub(super) fn canonicalize_command_for_detection(command: &str) -> Option<String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Split into tokens, respecting basic quoting
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    // `command -v <tool>` or `which <tool>`
    if (tokens[0] == "command" && tokens.len() >= 3 && tokens[1] == "-v") || (tokens[0] == "which" && tokens.len() >= 2)
    {
        let tool = if tokens[0] == "command" { tokens[2] } else { tokens[1] };
        // Strip path prefix (e.g., /usr/bin/ast-grep → ast-grep)
        let basename = tool.rsplit('/').next().unwrap_or(tool);
        return Some(format!("__verify__:{basename}"));
    }

    // `<tool> --help`, `<tool> -h`, `<tool> --version`, `<tool> -V`
    // Also handles `env VAR=val <tool> --help` by skipping env prefix.
    if tokens.len() >= 2 {
        // SAFETY: tokens.len() >= 2 is checked above, so .last() is guaranteed Some
        let last = tokens.last().expect("tokens.len() >= 2 checked above");
        if matches!(*last, "--help" | "-h" | "--version" | "-V" | "version") {
            // Skip leading `env` and its VAR=val flags to find the actual tool
            let tool_token = if tokens[0] == "env" {
                tokens.iter().skip(1).find(|t| !t.contains('=')).unwrap_or(&tokens[0])
            } else {
                &tokens[0]
            };
            let basename = tool_token.rsplit('/').next().unwrap_or(tool_token);
            if !basename.is_empty() {
                return Some(format!("__verify__:{basename}"));
            }
        }
    }

    // `cat <path>`, `head <path>`, `tail <path>` (simple single-file forms)
    if tokens.len() == 2 && matches!(tokens[0], "cat" | "head" | "tail") {
        let path = tokens[1].trim_matches(|c| c == '\'' || c == '"');
        return Some(format!("__read__:{path}"));
    }

    None
}

/// Compute a normalized hash of tool arguments for loop detection without
/// cloning the entire JSON object. Skips expensive large values (like `content`)
/// that are irrelevant to loop detection, and normalizes key aliases in-place
/// via the hash computation.
///
/// This eliminates the O(size_of_args) deep-clone that `normalize_args_for_detection`
/// performs, replacing it with an O(relevant_keys) walk.
pub(super) fn hash_normalized_args(tool_name: &str, args: &serde_json::Value) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let base_name = base_tool_name(tool_name);
    let mut hasher = DefaultHasher::new();

    if base_name == tools::CODE_SEARCH {
        hash_normalized_code_search_args(args, &mut hasher);
        return hasher.finish();
    }

    if let Some(obj) = args.as_object() {
        let is_read_tool =
            base_name == tools::READ_FILE || (base_name == tools::UNIFIED_FILE && tool_name.ends_with("::read"));
        let is_write_tool = base_name == tools::WRITE_FILE
            || base_name == tools::CREATE_FILE
            || base_name == tools::EDIT_FILE
            || base_name == tools::APPLY_PATCH
            || (base_name == tools::UNIFIED_FILE && !tool_name.ends_with("::read"));
        let is_list_tool = base_name == LEGACY_LIST_FILES;
        let is_command_tool = is_command_tool_name(base_name);

        // Keys to skip entirely
        let irrelevant_keys: &[&str] = &["page", "per_page", "encoding", "action"];

        // For read tools: alias keys that should map to canonical names
        let read_aliases: &[(&str, &str)] = &[
            ("file_path", "path"),
            ("filepath", "path"),
            ("target_path", "path"),
            ("file", "path"),
            ("offset_lines", "offset"),
            ("line_start", "offset"),
            ("offset_bytes", "offset"),
            ("start_line", "offset"),
            ("max_lines", "limit"),
            ("chunk_lines", "limit"),
            ("limit_lines", "limit"),
            ("page_size_lines", "limit"),
        ];

        // For list tools: normalize root path markers
        fn is_root_marker(v: &serde_json::Value) -> bool {
            if let Some(s) = v.as_str() {
                let trimmed = s.trim();
                trimmed.is_empty() || trimmed.trim_matches(|c: char| c == '.' || c == '/').is_empty()
            } else {
                false
            }
        }

        // Collect normalized entries
        let mut entries: Vec<(String, serde_json::Value)> = Vec::with_capacity(obj.len());

        for (key, value) in obj {
            if irrelevant_keys.contains(&key.as_str()) {
                continue;
            }

            // For write tools, skip large content fields
            if is_write_tool && matches!(key.as_str(), "content" | "new_content" | "diff") {
                entries.push((key.clone(), serde_json::Value::Null));
                continue;
            }

            // For read tools: normalize alias keys to canonical names
            if is_read_tool {
                if let Some((_, canonical)) = read_aliases.iter().find(|(alias, _)| *alias == key.as_str()) {
                    let canonical = canonical.to_string();
                    // Only insert if canonical key not already present
                    if !entries.iter().any(|(k, _)| *k == canonical) {
                        entries.push((canonical, value.clone()));
                    }
                    continue;
                }
            }

            entries.push((key.clone(), value.clone()));
        }

        // For list tools: normalize root path
        if is_list_tool {
            if let Some(pos) = entries.iter().position(|(k, _)| k == "path") {
                if is_root_marker(&entries[pos].1) {
                    entries[pos].1 = serde_json::Value::Null; // sentinel for __ROOT__
                }
            } else {
                entries.push(("path".to_string(), serde_json::Value::Null));
            }
        }

        // For read tools: handle line_end/end_line → offset + limit computation
        if is_read_tool {
            // Remove line_end/end_line and compute limit from offset + end_line
            let line_end_val = entries
                .iter()
                .position(|(k, _)| k == "line_end" || k == "end_line")
                .and_then(|pos| {
                    let val = entries.remove(pos).1;
                    val.as_u64()
                });

            if let Some(end) = line_end_val {
                if !entries.iter().any(|(k, _)| k == "limit") {
                    let start = entries
                        .iter()
                        .find(|(k, _)| k == "offset")
                        .and_then(|(_, v)| v.as_u64())
                        .unwrap_or(1);
                    let limit = end.saturating_sub(start).saturating_add(1);
                    entries.push(("limit".to_string(), serde_json::Value::Number(limit.into())));
                }
            }

            // Ensure canonical offset and limit are present
            if !entries.iter().any(|(k, _)| k == "offset") {
                entries.push(("offset".to_string(), serde_json::Value::Number(1.into())));
            }
            if !entries.iter().any(|(k, _)| k == "limit") {
                entries.push(("limit".to_string(), serde_json::Value::Null));
            }
        }

        // For command tools: canonicalize verification commands
        // (exec_command uses the `cmd` key; pty/dispatcher tools use `command`).
        if is_command_tool {
            if let Some(pos) = entries.iter().position(|(k, _)| k == "cmd" || k == "command") {
                if let serde_json::Value::String(cmd) = &entries[pos].1 {
                    if let Some(canonical) = canonicalize_command_for_detection(cmd) {
                        entries[pos].1 = serde_json::Value::String(canonical);
                    }
                }
            }
        }

        // Sort for consistent hashing (HashMap iteration order is random)
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        for (key, value) in &entries {
            key.hash(&mut hasher);
            value.hash(&mut hasher);
        }
    } else {
        args.hash(&mut hasher);
    }

    hasher.finish()
}

fn hash_normalized_code_search_args(args: &serde_json::Value, hasher: &mut impl std::hash::Hasher) {
    use std::hash::Hash;

    crate::tools::normalised_code_search_loop_identity(args)
        .unwrap_or_else(|| args.to_string())
        .hash(hasher);
}

/// Normalize tool arguments for consistent loop detection (kept for backward
/// compatibility with callers that need the actual normalized Value).
///
/// Prefer [`hash_normalized_args`] in hot paths to avoid the deep-clone cost.
#[cfg(test)]
pub(super) fn normalize_args_for_detection(tool_name: &str, args: &serde_json::Value) -> serde_json::Value {
    let base_name = base_tool_name(tool_name);
    if let Some(obj) = args.as_object() {
        let mut normalized = obj.clone();

        // Remove pagination params that shouldn't affect loop detection
        normalized.remove("page");
        normalized.remove("per_page");

        // For list_files: normalize root path variations
        if base_name == LEGACY_LIST_FILES {
            if let Some(path) = normalized.get("path").and_then(|v| v.as_str()) {
                let trimmed = path.trim();
                let only_root_markers = trimmed.trim_matches(|c| c == '.' || c == '/').is_empty();
                if trimmed.is_empty() || only_root_markers {
                    normalized.insert("path".into(), serde_json::json!("__ROOT__"));
                }
            } else {
                normalized.insert("path".into(), serde_json::json!("__ROOT__"));
            }
        }

        // For read-file tools: normalize parameter aliases so cycling through
        // offset_lines/line_start, max_lines/chunk_lines/limit_lines/limit, encoding, action
        // all hash to the same canonical form.
        let is_read_tool =
            base_name == tools::READ_FILE || (base_name == tools::UNIFIED_FILE && tool_name.ends_with("::read"));
        if is_read_tool {
            // Normalize path aliases to "path"
            for alias in ["file_path", "filepath", "target_path", "file"] {
                if let Some(val) = normalized.remove(alias)
                    && !normalized.contains_key("path")
                {
                    normalized.insert("path".into(), val);
                }
            }

            // Normalize offset aliases to "offset"
            // line_start=N → offset=N, offset_lines=N → offset=N, start_line=N → offset=N
            for alias in ["offset_lines", "line_start", "offset_bytes", "start_line"] {
                if let Some(val) = normalized.remove(alias)
                    && !normalized.contains_key("offset")
                {
                    normalized.insert("offset".into(), val);
                }
            }

            // Normalize limit aliases to "limit"
            // max_lines, chunk_lines, limit_lines, page_size_lines, line_end, end_line → limit
            // For line_end/end_line: compute limit from offset + end_line
            if let Some(line_end) = normalized.remove("line_end").or_else(|| normalized.remove("end_line")) {
                // start_line/end_line or line_start/line_end → offset + limit
                if !normalized.contains_key("limit") {
                    let start = normalized.get("offset").and_then(|v| v.as_u64()).unwrap_or(1);
                    let end = line_end.as_u64().unwrap_or(start);
                    let limit = end.saturating_sub(start).saturating_add(1);
                    normalized.insert("limit".into(), serde_json::json!(limit));
                }
            }
            for alias in ["max_lines", "chunk_lines", "limit_lines", "page_size_lines"] {
                if let Some(val) = normalized.remove(alias) {
                    normalized.entry(String::from("limit")).or_insert(val);
                }
            }

            // Canonicalize omitted offsets to the first line.
            normalized.entry(String::from("offset")).or_insert(serde_json::json!(1));

            // Remove noise params that don't change semantic intent
            normalized.remove("encoding");
            normalized.remove("action");
        }

        // For command tools: canonicalize verification and read commands
        // so `command -v ast-grep` / `which ast-grep` / `ast-grep --help`
        // all hash to the same `__verify__:ast-grep` token.
        if is_command_tool_name(base_name) {
            if let Some(cmd) = normalized.get("command").and_then(|v| v.as_str()) {
                if let Some(canonical) = canonicalize_command_for_detection(cmd) {
                    normalized.insert("command".into(), serde_json::Value::String(canonical));
                }
            }
        }

        serde_json::Value::Object(normalized)
    } else {
        args.clone()
    }
}

/// Extract the file path a read-only tool call is targeting, if any.
///
/// Used to track navigation streaks and repetitive-read-target detection:
/// consecutive reads of *different* files are exploration, while the same
/// target being read repeatedly is a loop.
pub(super) fn read_target_for_tool_call(tool_name: &str, args: &serde_json::Value) -> Option<String> {
    let base_name = base_tool_name(tool_name);

    // For command tools: extract file target from simple read commands
    // (`cat file`, `head file`, `tail file`) and normalized `__read__:path` commands.
    if is_command_tool_name(base_name) {
        // exec_command uses the `cmd` key; pty/dispatcher tools use `command`.
        if let Some(cmd) = args.get("cmd").or_else(|| args.get("command")).and_then(|v| v.as_str()) {
            // Check normalized form first
            if let Some(path) = cmd.strip_prefix("__read__:") {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            // Extract from simple cat/head/tail commands (before normalization)
            let tokens: Vec<&str> = cmd.split_whitespace().collect();
            if tokens.len() == 2 && matches!(tokens[0], "cat" | "head" | "tail") {
                let path = tokens[1].trim_matches(|c| c == '\'' || c == '"');
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
        return None;
    }

    let read_tool = base_name == tools::READ_FILE
        || base_name == tools::CODE_SEARCH
        || (base_name == tools::UNIFIED_FILE && is_file_operation_read(tool_name, args));
    if !read_tool {
        return None;
    }

    let obj = args.as_object()?;
    let keys: &[&str] = &["path", "file_path", "filepath", "target_path", "file"];
    for key in keys {
        if let Some(path) = obj.get(*key).and_then(|v| v.as_str()) {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Returns `true` when `(tool_name, args)` represent a `file_operation` read
/// invocation — either via the legacy `::read` suffix or the modern
/// `action: "read"` argument.
fn is_file_operation_read(tool_name: &str, args: &serde_json::Value) -> bool {
    tool_name.ends_with("::read") || matches!(args.get("action").and_then(|v| v.as_str()), Some("read"))
}
