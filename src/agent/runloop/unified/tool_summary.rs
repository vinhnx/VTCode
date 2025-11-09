use std::collections::HashSet;

use anyhow::Result;
use serde_json::Value;

use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};

pub(crate) fn render_tool_call_summary_with_status(
    renderer: &mut AnsiRenderer,
    tool_name: &str,
    args: &Value,
    status_icon: &str,
    exit_code: Option<i64>,
) -> Result<()> {
    let (headline, highlights) = describe_tool_action(tool_name, args);
    let details = collect_highlight_details(args, &highlights);
    let palette = ColorPalette::default();

    let mut line = String::new();

    // Status icon with color based on exit code
    let status_color = match exit_code {
        Some(0) => palette.success,
        Some(_) => palette.error,
        None => palette.info,
    };
    line.push_str(&render_styled(status_icon, status_color, None));
    line.push(' ');

    // Check if this is an MCP tool for special decoration
    let is_mcp = tool_name.starts_with("mcp::") || tool_name == "fetch";

    if is_mcp {
        // For MCP tools, use special bracket style with magenta
        line.push_str(&render_styled(
            &format!("[{}]", tool_name),
            anstyle::Color::Ansi(anstyle::AnsiColor::Magenta),
            None,
        ));
    } else {
        // Tool name in brackets with cyan color for normal tools
        line.push_str(&render_styled(
            &format!("[{}]", tool_name),
            palette.info,
            None,
        ));
    }
    line.push(' ');

    // Headline in bright white
    line.push_str(&render_styled(
        &headline,
        anstyle::Color::Ansi(anstyle::AnsiColor::White),
        None,
    ));

    // Details in dim gray if present - these are the call parameters
    if !details.is_empty() {
        line.push_str(" ");
        line.push_str(&render_styled(&format!("· {}", details.join(" · ")), palette.muted, None));
    } else {
        // Even if no specific highlights were extracted, show all parameters if available
        if let Some(map) = args.as_object() {
            let all_params: Vec<String> = map
                .iter()
                .filter_map(|(key, value)| match value {
                    Value::String(s) if !s.is_empty() => {
                        Some(format!("{}: {}", humanize_key(key), truncate_middle(s, 40))) // Shorter truncation
                    }
                    Value::Bool(true) => Some(humanize_key(key)),
                    Value::Array(items) => {
                        let strings: Vec<String> = items
                            .iter()
                            .filter_map(|item| item.as_str().map(|s| s.to_string()))
                            .collect();
                        if !strings.is_empty() {
                            Some(format!(
                                "{}: {}",
                                humanize_key(key),
                                summarize_list(&strings, 1, 30) // Shorter list
                            ))
                        } else {
                            None
                        }
                    }
                    Value::Number(num) => Some(format!("{}: {}", humanize_key(key), num)),
                    _ => None,
                })
                .collect();

            if !all_params.is_empty() {
                line.push_str(" ");
                line.push_str(&render_styled(
                    &format!("· {}", all_params.join(" · ")),
                    palette.muted,
                    None,
                ));
            }
        }
    }

    // Exit code at the end if available (colored based on success)
    if let Some(code) = exit_code {
        let code_color = if code == 0 { palette.success } else { palette.error };
        line.push_str(" ");
        line.push_str(&render_styled(&format!("(exit: {})", code), code_color, None));
    }

    renderer.line(MessageStyle::Info, &line)?;

    Ok(())
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
        actual_name if actual_name == tool_names::RUN_COMMAND => describe_shell_command(args)
            .map(|(desc, used)| {
                (
                    format!("{}{}", if is_mcp_tool { "MCP " } else { "" }, desc),
                    used,
                )
            })
            .unwrap_or_else(|| {
                (
                    format!("{}Run shell command", if is_mcp_tool { "MCP " } else { "" }),
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
        actual_name if actual_name == tool_names::UPDATE_PLAN => (
            format!("{}Update task plan", if is_mcp_tool { "MCP " } else { "" }),
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
    // Parameters will be shown in the details section by render_tool_call_summary_with_status
    ("Use Fetch".to_string(), HashSet::new())
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
        return Some((summary.to_string(), used));
    }

    if let Some(cmd) = args
        .get("bash_command")
        .and_then(|value| value.as_str())
        .filter(|s| !s.is_empty())
    {
        used.insert("bash_command".to_string());
        let summary = truncate_middle(cmd, 70);
        return Some((summary.to_string(), used));
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
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_len {
        return text.to_string();
    }
    if max_len <= 1 {
        return "…".to_string();
    }
    let head_len = max_len / 2;
    let tail_len = max_len.saturating_sub(head_len + 1);
    let mut result: String = chars.iter().take(head_len).collect();
    result.push('…');
    if tail_len > 0 {
        let tail: String = chars
            .iter()
            .rev()
            .take(tail_len)
            .cloned()
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        result.push_str(&tail);
    }
    result
}

fn collect_highlight_details(args: &Value, keys: &HashSet<String>) -> Vec<String> {
    let mut details = Vec::new();
    let Some(map) = args.as_object() else {
        return details;
    };
    for key in keys {
        if let Some(value) = map.get(key) {
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
}
