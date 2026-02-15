use std::collections::HashSet;

use anstyle::Color;
use anyhow::Result;
use serde_json::Value;

use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::registry::labels::tool_action_label;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};

use crate::agent::runloop::unified::tool_summary_helpers::{
    collect_param_details, command_line_for_args, describe_fetch_action, describe_grep_file,
    describe_list_files, describe_path_action, describe_shell_command, highlight_texts_for_summary,
    should_render_command_line, truncate_middle,
};

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
    let command_line =
        command_line_for_args(args).filter(|_| should_render_command_line(&highlights));
    let details = collect_param_details(args, &highlights);
    let summary_highlights = highlight_texts_for_summary(args, &highlights);
    let palette = ColorPalette::default();
    let action_label = tool_action_label(tool_name, args);
    let summary = build_tool_summary(&action_label, &headline);

    // Get current theme's primary color
    let theme_styles = theme::active_styles();
    let main_color = theme_styles
        .primary
        .get_fg_color()
        .unwrap_or(Color::Ansi(anstyle::AnsiColor::Green));

    let mut line = String::new();
    line.push_str(&render_styled("•", palette.muted, Some("dim".to_string())));
    line.push(' ');
    line.push_str(&render_summary_with_highlights(
        &summary,
        &summary_highlights,
        main_color,
        palette.accent,
        palette.muted,
    ));

    if let Some(stream) = stream_label {
        line.push(' ');
        line.push_str(&render_styled(stream, palette.info, None));
    }

    renderer.line(MessageStyle::Info, &line)?;

    if let Some(command_line) = command_line {
        let mut styled = String::new();
        styled.push_str("  ");
        styled.push_str(&render_styled("└", palette.muted, Some("dim".to_string())));
        styled.push(' ');
        styled.push_str(&render_styled("$", palette.accent, None));
        styled.push(' ');
        styled.push_str(&render_styled(&command_line, palette.muted, None));
        renderer.line(MessageStyle::Info, &styled)?;
    }

    // Details in gray if present - these are the call parameters
    for detail in details {
        let mut styled = String::new();
        styled.push_str("  ");
        styled.push_str(&render_styled("└", palette.muted, Some("dim".to_string())));
        styled.push(' ');
        styled.push_str(&render_styled(&detail, palette.muted, None));
        renderer.line(MessageStyle::Info, &styled)?;
    }

    Ok(())
}

fn render_summary_with_highlights(
    summary: &str,
    highlights: &[String],
    main_color: Color,
    accent_color: Color,
    muted_color: Color,
) -> String {
    if highlights.is_empty() {
        return render_styled(summary, main_color, None);
    }

    let mut ranges: Vec<(usize, usize)> = highlights
        .iter()
        .filter_map(|text| {
            if text.is_empty() {
                return None;
            }
            summary.find(text).map(|start| (start, start + text.len()))
        })
        .collect();

    if ranges.is_empty() {
        return render_styled(summary, main_color, None);
    }

    ranges.sort_by_key(|(start, _)| *start);

    let mut rendered = String::new();
    let mut cursor = 0usize;
    for (start, end) in ranges {
        if start < cursor || start >= summary.len() || end > summary.len() {
            continue;
        }
        if cursor < start {
            rendered.push_str(&render_styled(&summary[cursor..start], muted_color, None));
        }
        rendered.push_str(&render_styled(&summary[start..end], accent_color, None));
        cursor = end;
    }
    if cursor < summary.len() {
        rendered.push_str(&render_styled(&summary[cursor..], muted_color, None));
    }

    rendered
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
    if let Some(stripped) = normalized.strip_prefix("Use ")
        && stripped == action_label
    {
        return action_label.to_string();
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
    crate::agent::runloop::unified::tool_summary_helpers::humanize_tool_name(name)
}
