use std::collections::HashSet;

use anstyle::Color;
use anyhow::Result;
use serde_json::Value;

use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::registry::labels::tool_action_label;
use vtcode_core::tools::tool_intent;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};

use crate::agent::runloop::unified::tool_summary_helpers::{
    collect_param_details, command_line_for_args, describe_fetch_action, describe_grep_file,
    describe_list_files, describe_path_action, describe_shell_command, highlight_texts_for_summary,
    should_render_command_line, truncate_middle,
};

const RUN_SUMMARY_FIRST_WIDTH: usize = 62;
const RUN_SUMMARY_CONTINUATION_WIDTH: usize = 58;

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
    let command_line_candidate = command_line_for_args(args);
    let summary_highlights = highlight_texts_for_summary(args, &highlights);
    let palette = ColorPalette::default();
    let action_label = tool_action_label(tool_name, args);
    let is_run_command = action_label == "Run command (PTY)";
    let details = if is_run_command {
        Vec::new()
    } else {
        collect_param_details(args, &highlights)
    };
    let mut summary = build_tool_summary(&action_label, &headline);
    if is_run_command {
        summary = command_line_candidate
            .as_ref()
            .map(|command| format!("Ran {}", command))
            .unwrap_or(summary);
    }
    if is_run_command && run_summary_is_placeholder(&summary) {
        summary = "Ran command".to_string();
    }
    let command_line = if is_run_command {
        None
    } else {
        command_line_candidate.filter(|_| should_render_command_line(&highlights))
    };

    // Get current theme's primary color
    let theme_styles = theme::active_styles();
    let main_color = theme_styles
        .primary
        .get_fg_color()
        .unwrap_or(Color::Ansi(anstyle::AnsiColor::Green));

    let mut line = String::new();
    line.push_str(&render_styled("•", palette.muted, Some("dim".to_string())));
    line.push(' ');
    let mut wrapped_run_segments: Option<Vec<String>> = None;
    if let Some(command) = summary.strip_prefix("Ran ") {
        let wrapped = wrap_text_words(
            command,
            RUN_SUMMARY_FIRST_WIDTH,
            RUN_SUMMARY_CONTINUATION_WIDTH,
        );
        let first_segment = wrapped
            .first()
            .cloned()
            .unwrap_or_else(|| "command".to_string());
        wrapped_run_segments = Some(wrapped);
        line.push_str(&render_styled(
            "Ran",
            palette.accent,
            Some("bold".to_string()),
        ));
        line.push(' ');
        line.push_str(&render_run_command_segment(
            &first_segment,
            main_color,
            palette.muted,
        ));
    } else {
        line.push_str(&render_summary_with_highlights(
            &summary,
            &summary_highlights,
            main_color,
            palette.accent,
            palette.muted,
        ));
    }

    let stream_label = if summary.starts_with("Ran ") {
        None
    } else {
        stream_label
    };

    if let Some(stream) = stream_label {
        line.push(' ');
        line.push_str(&render_styled(stream, palette.info, None));
    }

    renderer.line(MessageStyle::Info, &line)?;
    if let Some(wrapped) = wrapped_run_segments {
        for segment in wrapped.into_iter().skip(1) {
            let mut continuation = String::new();
            continuation.push_str("  ");
            continuation.push_str(&render_styled("│", palette.muted, Some("dim".to_string())));
            continuation.push(' ');
            continuation.push_str(&render_styled(&segment, palette.muted, None));
            renderer.line(MessageStyle::Info, &continuation)?;
        }
    }

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

fn render_run_command_segment(segment: &str, command_color: Color, args_color: Color) -> String {
    let (command, args) = split_command_and_args(segment);
    if command.is_empty() {
        return render_styled(segment, args_color, None);
    }

    let mut rendered = String::new();
    rendered.push_str(&render_styled(command, command_color, None));
    if !args.is_empty() {
        rendered.push_str(&render_styled(&format!(" {}", args), args_color, None));
    }
    rendered
}

fn split_command_and_args(text: &str) -> (&str, &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return ("", "");
    }

    for (idx, ch) in trimmed.char_indices() {
        if ch.is_whitespace() {
            let command = &trimmed[..idx];
            let args = trimmed[idx..].trim_start();
            return (command, args);
        }
    }

    (trimmed, "")
}

fn build_tool_summary(action_label: &str, headline: &str) -> String {
    let normalized = headline.trim().trim_start_matches("MCP ").trim();
    if action_label == "Run command (PTY)" {
        if normalized.is_empty() {
            return "Ran command".to_string();
        }
        return format!("Ran {}", normalized);
    }
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

fn run_summary_is_placeholder(summary: &str) -> bool {
    let Some(command) = summary.strip_prefix("Ran ") else {
        return false;
    };
    let normalized = command.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "command"
            | "bash"
            | "run pty cmd"
            | "unified exec"
            | "use unified exec"
            | "use run pty cmd"
            | "use bash"
    )
}

fn wrap_text_words(text: &str, first_width: usize, continuation_width: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut remaining = trimmed;
    let mut width = first_width.max(1);
    while char_count(remaining) > width {
        let split = split_at_word_boundary(remaining, width);
        let (head, tail) = remaining.split_at(split);
        let head = head.trim();
        if head.is_empty() {
            break;
        }
        result.push(head.to_string());
        remaining = tail.trim_start();
        if remaining.is_empty() {
            break;
        }
        width = continuation_width.max(1);
    }
    if !remaining.is_empty() {
        result.push(remaining.to_string());
    }
    result
}

fn split_at_word_boundary(input: &str, width: usize) -> usize {
    let capped = byte_index_for_char_count(input, width);
    let candidate = &input[..capped];
    if let Some(boundary) = candidate.rfind(char::is_whitespace) {
        boundary
    } else {
        capped
    }
}

fn byte_index_for_char_count(input: &str, chars: usize) -> usize {
    if chars == 0 {
        return 0;
    }
    let mut seen = 0usize;
    for (idx, ch) in input.char_indices() {
        seen += 1;
        if seen == chars {
            return idx + ch.len_utf8();
        }
    }
    input.len()
}

fn char_count(input: &str) -> usize {
    input.chars().count()
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
        actual_name if actual_name == tool_names::UNIFIED_EXEC => {
            match tool_intent::unified_exec_action(args).unwrap_or("run") {
                "run" => describe_shell_command(args)
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
                "write" => (
                    format!("{}Send PTY input", if is_mcp_tool { "MCP " } else { "" }),
                    HashSet::new(),
                ),
                "poll" => (
                    format!("{}Read PTY session", if is_mcp_tool { "MCP " } else { "" }),
                    HashSet::new(),
                ),
                "list" => (
                    format!("{}List PTY sessions", if is_mcp_tool { "MCP " } else { "" }),
                    HashSet::new(),
                ),
                "close" => (
                    format!("{}Close PTY session", if is_mcp_tool { "MCP " } else { "" }),
                    HashSet::new(),
                ),
                "code" => (
                    format!("{}Run code", if is_mcp_tool { "MCP " } else { "" }),
                    HashSet::new(),
                ),
                _ => (
                    format!("{}Use Unified exec", if is_mcp_tool { "MCP " } else { "" }),
                    HashSet::new(),
                ),
            }
        }
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

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vtcode_core::config::constants::tools as tool_names;

    use super::{
        build_tool_summary, describe_tool_action, run_summary_is_placeholder, wrap_text_words,
    };

    #[test]
    fn build_tool_summary_formats_run_command_as_ran() {
        assert_eq!(
            build_tool_summary("Run command (PTY)", "cargo check -p vtcode"),
            "Ran cargo check -p vtcode"
        );
    }

    #[test]
    fn describe_tool_action_handles_unified_exec_run_command() {
        let (description, used_keys) = describe_tool_action(
            tool_names::UNIFIED_EXEC,
            &json!({
                "action": "run",
                "command": "cargo check -p vtcode"
            }),
        );

        assert_eq!(description, "cargo check -p vtcode");
        assert!(used_keys.contains("command"));
    }

    #[test]
    fn run_summary_placeholder_detection_catches_generic_labels() {
        assert!(run_summary_is_placeholder("Ran Use Unified exec"));
        assert!(run_summary_is_placeholder("Ran bash"));
        assert!(!run_summary_is_placeholder("Ran cargo check -p vtcode"));
    }

    #[test]
    fn wrap_text_words_wraps_long_command_summary() {
        let text = "cargo test -p vtcode run_command_preview_ build_tool_summary_formats_run_command_as_ran";
        let wrapped = wrap_text_words(text, 62, 58);
        assert_eq!(wrapped.len(), 2);
        assert_eq!(wrapped[0], "cargo test -p vtcode run_command_preview_");
        assert_eq!(wrapped[1], "build_tool_summary_formats_run_command_as_ran");
    }
}
