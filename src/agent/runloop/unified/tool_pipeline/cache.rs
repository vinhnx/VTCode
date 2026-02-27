use std::path::Path;

use serde_json::Value;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::result_cache::ToolCacheKey;

/// Determine if a tool is cacheable based on tool type and arguments.
pub(super) fn is_tool_cacheable(tool_name: &str, args: &Value) -> bool {
    // Always cache these read-only tools (original set)
    if matches!(
        tool_name,
        "read_file" | "list_files" | "grep_search" | "find_files"
    ) {
        return true;
    }

    // Cache search tools with stable arguments
    if matches!(tool_name, "search_tools" | "get_errors" | "agent_info") {
        // These tools typically have stable results within a session
        return true;
    }

    // Cache path-scoped git diff command calls to avoid redundant reruns.
    if extract_git_diff_cache_target(tool_name, args).is_some() {
        return true;
    }

    false
}

/// Enhanced cache key creation that includes workspace context in the target path
/// This prevents cache collisions between different workspaces
pub(super) fn create_enhanced_cache_key(
    tool_name: &str,
    args: &Value,
    cache_target: &str,
    workspace: &str,
) -> ToolCacheKey {
    // For file-based tools, include workspace in the target path to ensure uniqueness
    // For non-file tools, use a workspace-specific target path
    let enhanced_target = if cache_target.starts_with('/') || cache_target.contains(':') {
        // Absolute path or special path - keep as is
        cache_target.to_string()
    } else {
        // Relative path - prefix with workspace to ensure uniqueness
        format!("{}/{}", workspace, cache_target)
    };

    ToolCacheKey::from_json(tool_name, args, &enhanced_target)
}

pub(super) fn cache_target_path(tool_name: &str, args: &Value) -> String {
    if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
        return path.to_string();
    }
    if let Some(root) = args.get("root").and_then(|v| v.as_str()) {
        return root.to_string();
    }
    if let Some(target) = args.get("target_path").and_then(|v| v.as_str()) {
        return target.to_string();
    }
    if let Some(dir) = args.get("dir").and_then(|v| v.as_str()) {
        return dir.to_string();
    }
    if let Some(diff_target) = extract_git_diff_cache_target(tool_name, args) {
        return diff_target;
    }

    tool_name.to_string()
}

fn extract_git_diff_cache_target(tool_name: &str, args: &Value) -> Option<String> {
    let parts = command_parts_for_cache(tool_name, args)?;
    if contains_shell_operator(&parts) {
        return None;
    }
    if !is_git_diff_command(&parts) {
        return None;
    }
    extract_git_diff_path_target(&parts)
}

fn command_parts_for_cache(tool_name: &str, args: &Value) -> Option<Vec<String>> {
    match tool_name {
        tools::RUN_PTY_CMD | tools::SHELL => {
            let mut parts = command_value_to_parts(args.get("command")?)?;
            append_args(&mut parts, args.get("args"));
            if parts.is_empty() { None } else { Some(parts) }
        }
        tools::UNIFIED_EXEC | tools::EXEC_PTY_CMD | tools::EXEC => {
            let action = args.get("action").and_then(Value::as_str).unwrap_or("run");
            if action != "run" {
                return None;
            }
            let command_value = args
                .get("command")
                .or_else(|| args.get("cmd"))
                .or_else(|| args.get("raw_command"))?;
            let mut parts = command_value_to_parts(command_value)?;
            append_args(&mut parts, args.get("args"));
            if parts.is_empty() { None } else { Some(parts) }
        }
        _ => None,
    }
}

fn command_value_to_parts(value: &Value) -> Option<Vec<String>> {
    if let Some(command) = value.as_str() {
        let parts = shell_words::split(command)
            .ok()?
            .into_iter()
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if parts.is_empty() { None } else { Some(parts) }
    } else if let Some(parts) = value.as_array() {
        let collected = parts
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if collected.is_empty() {
            None
        } else {
            Some(collected)
        }
    } else {
        None
    }
}

fn append_args(parts: &mut Vec<String>, args_value: Option<&Value>) {
    let Some(args_array) = args_value.and_then(Value::as_array) else {
        return;
    };

    for arg in args_array {
        if let Some(segment) = arg
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            parts.push(segment.to_string());
        }
    }
}

fn is_git_diff_command(parts: &[String]) -> bool {
    let Some(first) = parts.first() else {
        return false;
    };
    let basename = Path::new(first)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(first.as_str())
        .to_ascii_lowercase();
    if basename != "git" && basename != "git.exe" {
        return false;
    }

    parts.iter().skip(1).any(|part| part == "diff")
}

fn extract_git_diff_path_target(parts: &[String]) -> Option<String> {
    let diff_index = parts.iter().position(|part| part == "diff")?;
    if diff_index + 1 >= parts.len() {
        return None;
    }

    let mut saw_separator = false;
    let mut targets = Vec::new();

    for part in parts.iter().skip(diff_index + 1) {
        if part == "--" {
            saw_separator = true;
            continue;
        }

        if !saw_separator {
            if part.starts_with('-') {
                continue;
            }
            if !is_path_like(part) {
                continue;
            }
        }

        targets.push(part.clone());
    }

    if targets.is_empty() {
        None
    } else {
        Some(targets.join(" "))
    }
}

fn is_path_like(candidate: &str) -> bool {
    candidate.contains('/') || candidate.contains('\\') || candidate.starts_with("./")
}

fn contains_shell_operator(parts: &[String]) -> bool {
    parts.iter().any(|part| {
        matches!(
            part.as_str(),
            "|" | "||" | "&" | "&&" | ";" | ">" | ">>" | "<"
        ) || part.contains('|')
            || part.contains(';')
            || part.contains("&&")
            || part.contains("||")
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vtcode_core::config::constants::tools;

    use super::{cache_target_path, is_tool_cacheable};

    #[test]
    fn caches_path_scoped_git_diff_run_pty() {
        let args = json!({
            "command": "git diff vtcode-tui/src/core_tui/session/diff_preview.rs"
        });

        assert!(is_tool_cacheable(tools::RUN_PTY_CMD, &args));
        assert_eq!(
            cache_target_path(tools::RUN_PTY_CMD, &args),
            "vtcode-tui/src/core_tui/session/diff_preview.rs"
        );
    }

    #[test]
    fn does_not_cache_git_diff_without_path() {
        let args = json!({ "command": "git diff" });

        assert!(!is_tool_cacheable(tools::RUN_PTY_CMD, &args));
        assert_eq!(
            cache_target_path(tools::RUN_PTY_CMD, &args),
            tools::RUN_PTY_CMD
        );
    }

    #[test]
    fn caches_unified_exec_run_with_git_diff_path() {
        let args = json!({
            "action": "run",
            "command": ["git", "diff", "src/main.rs"]
        });

        assert!(is_tool_cacheable(tools::UNIFIED_EXEC, &args));
        assert_eq!(cache_target_path(tools::UNIFIED_EXEC, &args), "src/main.rs");
    }

    #[test]
    fn does_not_cache_non_run_unified_exec_action() {
        let args = json!({
            "action": "poll",
            "session_id": "run-123"
        });

        assert!(!is_tool_cacheable(tools::UNIFIED_EXEC, &args));
    }

    #[test]
    fn does_not_cache_compound_shell_command_with_diff() {
        let args = json!({
            "command": "git diff src/main.rs && echo done"
        });

        assert!(!is_tool_cacheable(tools::RUN_PTY_CMD, &args));
    }

    #[test]
    fn caches_quoted_path_with_spaces() {
        let args = json!({
            "command": "git diff \"dir with space/file.rs\""
        });

        assert!(is_tool_cacheable(tools::RUN_PTY_CMD, &args));
        assert_eq!(
            cache_target_path(tools::RUN_PTY_CMD, &args),
            "dir with space/file.rs"
        );
    }
}
