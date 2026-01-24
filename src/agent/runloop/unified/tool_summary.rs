use std::collections::HashSet;

use anyhow::Result;
use serde_json::Value;

use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::registry::labels::tool_action_label;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};

/// Pre-execution indicators for file modification operations
/// These provide visual feedback before the actual edit/write/patch is applied
pub(crate) fn render_file_operation_indicator(
    renderer: &mut AnsiRenderer,
    tool_name: &str,
    args: &Value,
) -> Result<()> {
    let palette = ColorPalette::default();

    // Only show indicators for file modification tools
    let (indicator_icon, action_verb) = match tool_name {
        name if name == tool_names::WRITE_FILE || name == tool_names::CREATE_FILE => {
            ("❋", "Writing")
        }
        name if name == tool_names::EDIT_FILE => ("❋", "Editing"),
        name if name == tool_names::APPLY_PATCH => ("❋", "Applying patch to"),
        name if name == tool_names::SEARCH_REPLACE => ("❋", "Search/replace in"),
        name if name == tool_names::DELETE_FILE => ("❋", "Deleting"),
        name if name == tool_names::UNIFIED_FILE => {
            // Determine action from unified_file parameters
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .or_else(|| {
                    if args.get("old_str").is_some() {
                        Some("edit")
                    } else if args.get("patch").is_some() {
                        Some("patch")
                    } else if args.get("content").is_some() {
                        Some("write")
                    } else {
                        None
                    }
                })
                .unwrap_or("read");

            match action {
                "write" | "create" => ("❋", "Writing"),
                "edit" => ("❋", "Editing"),
                "patch" | "apply_patch" => ("❋", "Applying patch to"),
                "delete" => ("❋", "Deleting"),
                _ => return Ok(()), // Skip indicator for read operations
            }
        }
        _ => return Ok(()), // No indicator for non-file-modification tools
    };

    // Extract file path from arguments
    let file_path = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .or_else(|| args.get("filename"))
        .and_then(Value::as_str)
        .map(|p| truncate_middle(p, 60))
        .unwrap_or_else(|| "file".to_string());

    let mut line = String::new();

    // Icon
    line.push_str(indicator_icon);
    line.push(' ');

    // Action verb in info color
    line.push_str(&render_styled(action_verb, palette.info, None));
    line.push(' ');

    // File path in primary color (dim for subtlety)
    line.push_str(&render_styled(&file_path, palette.muted, None));
    line.push_str(&render_styled("...", palette.muted, None));

    renderer.line(MessageStyle::Info, &line)?;

    Ok(())
}

/// Check if a tool is a file modification tool that should show a pre-execution indicator
pub(crate) fn is_file_modification_tool(tool_name: &str, args: &Value) -> bool {
    match tool_name {
        name if name == tool_names::WRITE_FILE
            || name == tool_names::CREATE_FILE
            || name == tool_names::EDIT_FILE
            || name == tool_names::APPLY_PATCH
            || name == tool_names::SEARCH_REPLACE
            || name == tool_names::DELETE_FILE =>
        {
            true
        }
        name if name == tool_names::UNIFIED_FILE => {
            // Check if unified_file is doing a write operation
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .or_else(|| {
                    if args.get("old_str").is_some() {
                        Some("edit")
                    } else if args.get("patch").is_some() {
                        Some("patch")
                    } else if args.get("content").is_some() {
                        Some("write")
                    } else {
                        None
                    }
                })
                .unwrap_or("read");

            matches!(
                action,
                "write" | "create" | "edit" | "patch" | "apply_patch" | "delete"
            )
        }
        _ => false,
    }
}

pub(crate) fn render_tool_call_summary(
    renderer: &mut AnsiRenderer,
    tool_name: &str,
    args: &Value,
    stream_label: Option<&str>,
) -> Result<()> {
    let (headline, highlights) = describe_tool_action(tool_name, args);
    let details = collect_param_details(args, &highlights);
    let palette = ColorPalette::default();
    let action_label = tool_action_label(tool_name, args);
    let summary = build_tool_summary(&action_label, &headline);

    let mut line = String::new();
    line.push_str(&render_styled(&summary, palette.primary, None));

    // Details in dim gray if present - these are the call parameters
    if !details.is_empty() {
        line.push_str(" .. ");
        line.push_str(&render_styled(
            &details.join(" .. "),
            palette.muted,
            None,
        ));
    }

    if let Some(stream) = stream_label {
        line.push(' ');
        line.push_str(&render_styled(stream, palette.info, None));
    }

    renderer.line(MessageStyle::Info, &line)?;

    Ok(())
}

fn build_tool_summary(action_label: &str, headline: &str) -> String {
    let normalized = headline.trim().trim_start_matches("MCP ").trim();
    if normalized.is_empty() {
        return action_label.to_string();
    }
    if normalized == action_label {
        return normalized.to_string();
    }
    if normalized.starts_with(action_label) {
        return normalized.to_string();
    }
    if let Some(stripped) = normalized.strip_prefix("Use ") {
        if stripped == action_label {
            return action_label.to_string();
        }
    }
    format!("{} {}", action_label, normalized)
}

pub(crate) fn stream_label_from_output(
    output: &Value,
    command_success: bool,
) -> Option<&'static str> {
    let has_output = output
        .get("output")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty());
    let has_stdout = output
        .get("stdout")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty());
    let has_stderr = output
        .get("stderr")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty());
    let has_error = output.get("error").is_some() || output.get("error_type").is_some();

    if has_output {
        return Some("output");
    }
    match (has_stdout, has_stderr) {
        (true, true) => Some("stdio"),
        (true, false) => Some("stdout"),
        (false, true) => Some("stderr"),
        (false, false) => {
            if has_error || !command_success {
                Some("error")
            } else {
                None
            }
        }
    }
}

pub(crate) fn describe_tool_action(tool_name: &str, args: &Value) -> (String, HashSet<String>) {
    // Check if this is an MCP tool based on the original naming convention
    let is_mcp_tool =
        tool_name.starts_with("mcp::") || tool_name.starts_with("mcp_") || tool_name == "fetch";

    // For the actual matching, we need to use the tool name without the "mcp_" prefix
    let actual_tool_name = if let Some(stripped) = tool_name.strip_prefix("mcp_") {
        stripped
    } else if tool_name.starts_with("mcp::") {
        // For tools in mcp::provider::name format, extract just the tool name
        tool_name.split("::").last().unwrap_or(tool_name)
    } else {
        tool_name
    };

    match actual_tool_name {
        actual_name if actual_name == tool_names::RUN_PTY_CMD => describe_shell_command(args)
            .map(|(desc, used)| {
                (
                    format!("{}{}", if is_mcp_tool { "MCP " } else { "" }, desc),
                    used,
                )
            })
            .unwrap_or_else(|| {
                (
                    format!("{}bash", if is_mcp_tool { "MCP " } else { "" }),
                    HashSet::new(),
                )
            }),
        actual_name if actual_name == tool_names::LIST_FILES => describe_list_files(args)
            .map(|(desc, used)| {
                (
                    format!("{}{}", if is_mcp_tool { "MCP " } else { "" }, desc),
                    used,
                )
            })
            .unwrap_or_else(|| {
                (
                    format!("{}List files", if is_mcp_tool { "MCP " } else { "" }),
                    HashSet::new(),
                )
            }),
        actual_name if actual_name == tool_names::GREP_FILE => describe_grep_file(args)
            .map(|(desc, used)| {
                (
                    format!("{}{}", if is_mcp_tool { "MCP " } else { "" }, desc),
                    used,
                )
            })
            .unwrap_or_else(|| {
                (
                    format!("{}Search with grep", if is_mcp_tool { "MCP " } else { "" }),
                    HashSet::new(),
                )
            }),
        actual_name if actual_name == tool_names::READ_FILE => {
            describe_path_action(args, "Read file", &["path"])
                .map(|(desc, used)| {
                    (
                        format!("{}{}", if is_mcp_tool { "MCP " } else { "" }, desc),
                        used,
                    )
                })
                .unwrap_or_else(|| {
                    (
                        format!("{}Read file", if is_mcp_tool { "MCP " } else { "" }),
                        HashSet::new(),
                    )
                })
        }
        actual_name if actual_name == tool_names::WRITE_FILE => {
            describe_path_action(args, "Write file", &["path"])
                .map(|(desc, used)| {
                    (
                        format!("{}{}", if is_mcp_tool { "MCP " } else { "" }, desc),
                        used,
                    )
                })
                .unwrap_or_else(|| {
                    (
                        format!("{}Write file", if is_mcp_tool { "MCP " } else { "" }),
                        HashSet::new(),
                    )
                })
        }
        actual_name if actual_name == tool_names::EDIT_FILE => {
            describe_path_action(args, "Edit file", &["path"])
                .map(|(desc, used)| {
                    (
                        format!("{}{}", if is_mcp_tool { "MCP " } else { "" }, desc),
                        used,
                    )
                })
                .unwrap_or_else(|| {
                    (
                        format!("{}Edit file", if is_mcp_tool { "MCP " } else { "" }),
                        HashSet::new(),
                    )
                })
        }
        actual_name if actual_name == tool_names::CREATE_FILE => {
            describe_path_action(args, "Create file", &["path"])
                .map(|(desc, used)| {
                    (
                        format!("{}{}", if is_mcp_tool { "MCP " } else { "" }, desc),
                        used,
                    )
                })
                .unwrap_or_else(|| {
                    (
                        format!("{}Create file", if is_mcp_tool { "MCP " } else { "" }),
                        HashSet::new(),
                    )
                })
        }
        actual_name if actual_name == tool_names::UNIFIED_FILE => {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .or_else(|| {
                    if args.get("old_str").is_some() {
                        Some("edit")
                    } else if args.get("patch").is_some() {
                        Some("patch")
                    } else if args.get("content").is_some() {
                        Some("write")
                    } else if args.get("destination").is_some() {
                        Some("move")
                    } else {
                        Some("read")
                    }
                })
                .unwrap_or("read");

            let (verb, keys): (&str, &[&str]) = match action {
                "read" => ("Read file", &["path", "file_path", "target_path"]),
                "write" => ("Write file", &["path", "file_path", "target_path"]),
                "edit" => ("Edit file", &["path", "file_path", "target_path"]),
                "patch" => ("Apply patch", &["path", "file_path", "target_path"]),
                "delete" => ("Delete file", &["path", "file_path", "target_path"]),
                "move" => ("Move file", &["path", "file_path", "target_path"]),
                "copy" => ("Copy file", &["path", "file_path", "target_path"]),
                _ => ("File operation", &["path", "file_path", "target_path"]),
            };

            describe_path_action(args, verb, keys)
                .map(|(desc, used)| {
                    (
                        format!("{}{}", if is_mcp_tool { "MCP " } else { "" }, desc),
                        used,
                    )
                })
                .unwrap_or_else(|| {
                    (
                        format!("{}{}", if is_mcp_tool { "MCP " } else { "" }, verb),
                        HashSet::new(),
                    )
                })
        }
        actual_name if actual_name == tool_names::DELETE_FILE => {
            describe_path_action(args, "Delete file", &["path"])
                .map(|(desc, used)| {
                    (
                        format!("{}{}", if is_mcp_tool { "MCP " } else { "" }, desc),
                        used,
                    )
                })
                .unwrap_or_else(|| {
                    (
                        format!("{}Delete file", if is_mcp_tool { "MCP " } else { "" }),
                        HashSet::new(),
                    )
                })
        }
        actual_name if actual_name == tool_names::APPLY_PATCH => (
            format!(
                "{}Apply workspace patch",
                if is_mcp_tool { "MCP " } else { "" }
            ),
            HashSet::new(),
        ),
        "fetch" | "web_fetch" => {
            let (desc, used) = describe_fetch_action(args);
            (
                format!("{}{}", if is_mcp_tool { "MCP " } else { "" }, desc),
                used,
            )
        }
        _ => (
            format!(
                "{}Use {}",
                if is_mcp_tool { "MCP " } else { "" },
                humanize_tool_name(actual_tool_name)
            ),
            HashSet::new(),
        ),
    }
}

pub(crate) fn humanize_tool_name(name: &str) -> String {
    humanize_key(name)
}

fn describe_fetch_action(_args: &Value) -> (String, HashSet<String>) {
    // Return simple description without parameters to avoid duplication
    // Parameters will be shown in the details section by render_tool_call_summary
    ("Use Fetch".into(), HashSet::new())
}

fn describe_shell_command(args: &Value) -> Option<(String, HashSet<String>)> {
    let mut used = HashSet::new();
    if let Some(parts) = args
        .get("command")
        .and_then(|value| value.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .filter(|parts: &Vec<String>| !parts.is_empty())
    {
        used.insert("command".to_string());
        let joined = parts.join(" ");
        let summary = truncate_middle(&joined, 70);
        return Some((summary, used));
    }

    if let Some(cmd) = args
        .get("bash_command")
        .and_then(|value| value.as_str())
        .filter(|s| !s.is_empty())
    {
        used.insert("bash_command".to_string());
        let summary = truncate_middle(cmd, 70);
        return Some((summary, used));
    }

    None
}

fn describe_list_files(args: &Value) -> Option<(String, HashSet<String>)> {
    if let Some(path) = lookup_string(args, "path") {
        let mut used = HashSet::new();
        used.insert("path".to_string());
        let location = if path == "." {
            "workspace root".to_string()
        } else {
            truncate_middle(&path, 60)
        };
        return Some((format!("List files in {}", location), used));
    }
    if let Some(pattern) = lookup_string(args, "name_pattern") {
        let mut used = HashSet::new();
        used.insert("name_pattern".to_string());
        return Some((
            format!("Find files named {}", truncate_middle(&pattern, 40)),
            used,
        ));
    }
    if let Some(pattern) = lookup_string(args, "content_pattern") {
        let mut used = HashSet::new();
        used.insert("content_pattern".to_string());
        return Some((
            format!("Search files for {}", truncate_middle(&pattern, 40)),
            used,
        ));
    }
    None
}

fn describe_grep_file(args: &Value) -> Option<(String, HashSet<String>)> {
    let pattern = lookup_string(args, "pattern");
    let path = lookup_string(args, "path");
    match (pattern, path) {
        (Some(pat), Some(path)) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            used.insert("path".to_string());
            Some((
                format!(
                    "Grep {} in {}",
                    truncate_middle(&pat, 40),
                    truncate_middle(&path, 40)
                ),
                used,
            ))
        }
        (Some(pat), None) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            Some((format!("Grep {}", truncate_middle(&pat, 40)), used))
        }
        _ => None,
    }
}

fn describe_path_action(
    args: &Value,
    verb: &str,
    keys: &[&str],
) -> Option<(String, HashSet<String>)> {
    for key in keys {
        if let Some(value) = lookup_string(args, key) {
            let mut used = HashSet::new();
            used.insert((*key).to_string());
            let summary = truncate_middle(&value, 60);
            return Some((format!("{} {}", verb, summary), used));
        }
    }
    None
}

fn lookup_string(args: &Value, key: &str) -> Option<String> {
    args.as_object()
        .and_then(|map| map.get(key))
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn humanize_key(key: &str) -> String {
    let replaced = key.replace('_', " ");
    if replaced.is_empty() {
        return replaced;
    }
    let mut chars = replaced.chars();
    let first = chars.next().unwrap();
    let mut result = first.to_uppercase().collect::<String>();
    result.push_str(&chars.collect::<String>());
    result
}

fn truncate_middle(text: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let sanitized: String = text
        .chars()
        .map(|c| {
            if matches!(c, '\n' | '\r' | '\t') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let char_count = sanitized.chars().count();
    if char_count <= max_len {
        return sanitized;
    }
    if max_len <= 1 {
        return "…".to_string();
    }
    let head_len = max_len / 2;
    let tail_len = max_len.saturating_sub(head_len + 1);

    // Collect only head and tail characters to avoid allocating the full char vector.
    let head: String = sanitized.chars().take(head_len).collect();
    let mut result = String::with_capacity(head.len() + tail_len + 1);
    result.push_str(&head);
    result.push('…');
    if tail_len > 0 {
        // Collect tail in reverse then reverse to restore order.
        let mut tail_rev: Vec<char> = sanitized.chars().rev().take(tail_len).collect();
        tail_rev.reverse();
        let tail: String = tail_rev.into_iter().collect();
        result.push_str(&tail);
    }
    result
}

fn collect_param_details(args: &Value, keys: &HashSet<String>) -> Vec<String> {
    let mut details = Vec::new();
    let Some(map) = args.as_object() else {
        return details;
    };
    let include_all = keys.is_empty();
    for (key, value) in map {
        if !include_all && keys.contains(key) {
            continue;
        }
        match value {
            Value::String(s) if !s.is_empty() => {
                details.push(format!("{}: {}", humanize_key(key), truncate_middle(s, 60)))
            }
            Value::Bool(true) => {
                details.push(humanize_key(key));
            }
            Value::Array(items) => {
                let strings: Vec<String> = items
                    .iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect();
                if !strings.is_empty() {
                    details.push(format!(
                        "{}: {}",
                        humanize_key(key),
                        summarize_list(&strings, 2, 60)
                    ));
                }
            }
            Value::Number(num) => {
                details.push(format!("{}: {}", humanize_key(key), num));
            }
            _ => {}
        }
    }
    details
}

fn summarize_list(items: &[String], max_items: usize, max_len: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    let shown: Vec<String> = items
        .iter()
        .take(max_items)
        .map(|s| truncate_middle(s, max_len))
        .collect();
    if items.len() > max_items {
        format!("{} +{} more", shown.join(", "), items.len() - max_items)
    } else {
        shown.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_describe_shell_command_new_format() {
        let args = json!({
            "command": ["bash", "-lc", "ls -R"]
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert_eq!(description, "bash -lc ls -R");
    }

    #[test]
    fn test_describe_shell_command_bash_command_format() {
        let args = json!({
            "bash_command": "pwd"
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert_eq!(description, "pwd");
    }

    #[test]
    fn test_describe_shell_command_truncation() {
        let long_command = "a".repeat(100);
        let args = json!({
            "command": [long_command]
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert!(description.contains("…")); // Should be truncated
    }

    #[test]
    fn test_is_file_modification_tool_write_file() {
        let args = json!({"path": "/tmp/test.txt", "content": "hello"});
        assert!(is_file_modification_tool("write_file", &args));
    }

    #[test]
    fn test_is_file_modification_tool_edit_file() {
        let args = json!({"path": "/tmp/test.txt", "old_str": "foo", "new_str": "bar"});
        assert!(is_file_modification_tool("edit_file", &args));
    }

    #[test]
    fn test_is_file_modification_tool_apply_patch() {
        let args = json!({"path": "/tmp/test.txt", "patch": "diff content"});
        assert!(is_file_modification_tool("apply_patch", &args));
    }

    #[test]
    fn test_is_file_modification_tool_unified_file_write() {
        let args = json!({"path": "/tmp/test.txt", "content": "hello"});
        assert!(is_file_modification_tool("unified_file", &args));
    }

    #[test]
    fn test_is_file_modification_tool_unified_file_edit() {
        let args = json!({"path": "/tmp/test.txt", "old_str": "foo", "new_str": "bar"});
        assert!(is_file_modification_tool("unified_file", &args));
    }

    #[test]
    fn test_is_file_modification_tool_unified_file_read() {
        let args = json!({"path": "/tmp/test.txt", "action": "read"});
        assert!(!is_file_modification_tool("unified_file", &args));
    }

    #[test]
    fn test_is_file_modification_tool_read_file() {
        let args = json!({"path": "/tmp/test.txt"});
        assert!(!is_file_modification_tool("read_file", &args));
    }

    #[test]
    fn test_is_file_modification_tool_grep_file() {
        let args = json!({"pattern": "test", "path": "/tmp"});
        assert!(!is_file_modification_tool("grep_file", &args));
    }
}
