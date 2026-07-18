use hashbrown::HashSet;

use std::path::Path;

use anstyle::Color;
use anyhow::Result;
use serde_json::Value;
use vtcode_commons::formatting::wrap_text_words;

use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::registry::labels::tool_action_label;
use vtcode_core::tools::tool_intent;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};

use crate::agent::runloop::tool_output::render_tree_detail;
use crate::agent::runloop::unified::tool_summary_helpers::{
    collect_param_details, command_line_for_args, describe_fetch_action, describe_grep_file, describe_list_files,
    describe_path_action, describe_shell_command, highlight_texts_for_summary, relativize_command_paths,
    relativize_to_workspace, should_render_command_line, truncate_path_middle,
};

/// Ambient context required to render tool-call summaries.
///
/// This is the single interface boundary between the tool-output pipeline and the
/// pure summarization helpers. It isolates the rendering logic from the surrounding
/// runtime (run-loop context, config, etc.) so the summarization can be unit-tested
/// and evolved independently of the agent loop.
pub(crate) struct ToolSummaryRenderContext<'a> {
    pub workspace_root: Option<&'a Path>,
}

const RUN_SUMMARY_FIRST_WIDTH: usize = 62;
const RUN_SUMMARY_CONTINUATION_WIDTH: usize = 58;

/// Infer the action string for an internal file-operation call from its arguments.
/// This is the single source of truth for action inference — all three call sites
/// (`render_file_operation_indicator`, `is_file_modification_tool`, `describe_tool_action`)
/// must use this function to stay consistent.
fn file_operation_action(args: &Value) -> &'static str {
    if let Some(action) = args.get("action").and_then(Value::as_str) {
        return match action {
            "write" | "create" => "write",
            "edit" => "edit",
            "patch" | "apply_patch" => "patch",
            "delete" => "delete",
            "move" => "move",
            _ => "read",
        };
    }
    if args.get("old_str").is_some() {
        "edit"
    } else if args.get("patch").is_some() {
        "patch"
    } else if args.get("content").is_some() {
        "write"
    } else if args.get("destination").is_some() {
        "move"
    } else {
        "read"
    }
}

/// Pre-execution indicators for file modification operations
/// These provide visual feedback before the actual edit/write/patch is applied
pub(crate) fn render_file_operation_indicator(
    renderer: &mut AnsiRenderer,
    tool_name: &str,
    args: &Value,
    ctx: &ToolSummaryRenderContext,
) -> Result<()> {
    let palette = ColorPalette::default();

    // Only show indicators for file modification tools
    let (indicator_icon, action_verb) = match tool_name {
        name if name == tool_names::WRITE_FILE || name == tool_names::CREATE_FILE => ("❋", "Writing"),
        name if name == tool_names::EDIT_FILE => ("❋", "Editing"),
        name if name == tool_names::APPLY_PATCH => ("❋", "Applying patch to"),
        name if name == tool_names::SEARCH_REPLACE => ("❋", "Search/replace in"),
        name if name == tool_names::DELETE_FILE => ("❋", "Deleting"),
        name if name == tool_names::UNIFIED_FILE => {
            let action = file_operation_action(args);

            match action {
                "write" | "create" => ("❋", "Writing"),
                "edit" => ("❋", "Editing"),
                "patch" | "apply_patch" => ("❋", "Applying patch to"),
                "delete" => ("❋", "Deleting"),
                "move" => ("❋", "Moving"),
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
        .map(|p| {
            let rel = relativize_to_workspace(p, ctx.workspace_root);
            truncate_path_middle(&rel, 60)
        })
        .or_else(|| {
            // For apply_patch, extract the first file path from patch content
            extract_first_patch_file_path(args).map(|p| truncate_path_middle(&p, 60))
        })
        .unwrap_or_else(|| "file".to_string());

    let mut line = String::with_capacity(64);

    // Icon
    line.push_str(indicator_icon);
    line.push(' ');

    // Action verb in info color
    line.push_str(&render_styled(action_verb, palette.info, None));
    line.push(' ');

    // File path in primary color (dim for subtlety)
    line.push_str(&render_styled(&file_path, palette.muted, None));
    line.push_str(&render_styled("...", palette.muted, None));

    renderer.line(MessageStyle::Tool, &line)?;

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
        name if name == tool_names::UNIFIED_FILE || name == "file_operation" => {
            let action = file_operation_action(args);

            matches!(action, "write" | "create" | "edit" | "patch" | "apply_patch" | "delete" | "move")
        }
        _ => false,
    }
}

pub(crate) fn render_tool_call_summary(
    renderer: &mut AnsiRenderer,
    tool_name: &str,
    args: &Value,
    stream_label: Option<&str>,
    ctx: &ToolSummaryRenderContext,
) -> Result<()> {
    let data = prepare_summary_data(tool_name, args, ctx.workspace_root);

    let theme_styles = theme::active_styles();
    let main_color = theme_styles
        .primary
        .get_fg_color()
        .unwrap_or(Color::Ansi(anstyle::AnsiColor::Green));
    let palette = ColorPalette::default();

    let mut line = String::with_capacity(128);
    line.push_str(&render_styled("•", palette.muted, Some("dim".to_string())));
    line.push(' ');

    let wrapped_run_segments = render_bullet_line(&mut line, &data, stream_label, main_color, &palette);

    renderer.line(MessageStyle::Info, &line)?;

    render_continuation_lines(renderer, &wrapped_run_segments, &palette)?;
    render_command_line(renderer, &data.command_line, &palette)?;
    render_details(renderer, &data.details)?;

    Ok(())
}

struct SummaryData {
    summary: String,
    summary_highlights: Vec<String>,
    command_line: Option<String>,
    details: Vec<String>,
}

fn prepare_summary_data(tool_name: &str, args: &Value, workspace_root: Option<&Path>) -> SummaryData {
    let (headline, highlights) = describe_tool_action(tool_name, args, workspace_root);
    let command_line_candidate = command_line_for_args(args).map(|cmd| relativize_command_paths(&cmd, workspace_root));
    let summary_highlights = highlight_texts_for_summary(args, &highlights, workspace_root);
    let action_label = tool_action_label(tool_name, args);
    let is_run_command = action_label == "Run command";

    let details = if is_run_command {
        Vec::new()
    } else {
        collect_param_details(args, &highlights, workspace_root)
    };

    let mut summary = build_tool_summary(&action_label, &headline);
    if is_run_command {
        summary = command_line_candidate
            .as_ref()
            .map(|command| format!("Ran {command}"))
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

    SummaryData { summary, summary_highlights, command_line, details }
}

fn render_bullet_line(
    line: &mut String,
    data: &SummaryData,
    stream_label: Option<&str>,
    main_color: Color,
    palette: &ColorPalette,
) -> Option<Vec<String>> {
    let mut wrapped_run_segments: Option<Vec<String>> = None;
    if let Some(command) = data.summary.strip_prefix("Ran ") {
        let wrapped = wrap_text_words(command, RUN_SUMMARY_FIRST_WIDTH, RUN_SUMMARY_CONTINUATION_WIDTH);
        let first_segment = wrapped.first().cloned().unwrap_or_else(|| "command".to_string());
        wrapped_run_segments = Some(wrapped);
        line.push_str(&render_styled("Ran", palette.accent, Some("bold".to_string())));
        line.push(' ');
        line.push_str(&render_run_command_segment(&first_segment, main_color, palette.muted));
    } else {
        line.push_str(&render_summary_with_highlights(
            &data.summary,
            &data.summary_highlights,
            main_color,
            palette.accent,
            palette.muted,
        ));
    }

    let effective_stream = if data.summary.starts_with("Ran ") {
        None
    } else {
        stream_label
    };

    if let Some(stream) = effective_stream {
        line.push(' ');
        line.push_str(&render_styled(stream, palette.info, None));
    }

    wrapped_run_segments
}

fn render_continuation_lines(
    renderer: &mut AnsiRenderer,
    wrapped_run_segments: &Option<Vec<String>>,
    palette: &ColorPalette,
) -> Result<()> {
    if let Some(wrapped) = wrapped_run_segments {
        for segment in wrapped.iter().skip(1) {
            let mut continuation = String::with_capacity(segment.len() + 32);
            continuation.push_str("  ");
            continuation.push_str(&render_styled("│", palette.muted, Some("dim".to_string())));
            continuation.push(' ');
            continuation.push_str(&render_styled(segment, palette.muted, None));
            renderer.line(MessageStyle::Info, &continuation)?;
        }
    }
    Ok(())
}

fn render_command_line(
    renderer: &mut AnsiRenderer,
    command_line: &Option<String>,
    palette: &ColorPalette,
) -> Result<()> {
    if let Some(command_line) = command_line {
        let mut styled = String::with_capacity(64);
        crate::agent::runloop::tool_output::push_tree_prefix(&mut styled, palette);
        styled.push_str(&render_styled("$", palette.accent, None));
        styled.push(' ');
        styled.push_str(&render_styled(command_line, palette.muted, None));
        renderer.line(MessageStyle::Info, &styled)?;
    }
    Ok(())
}

fn render_details(renderer: &mut AnsiRenderer, details: &[String]) -> Result<()> {
    for detail in details {
        render_tree_detail(renderer, detail)?;
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

    let mut rendered = String::with_capacity(summary.len() * 3);
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

    let mut rendered = String::with_capacity(segment.len() * 2);
    rendered.push_str(&render_styled(command, command_color, None));
    if !args.is_empty() {
        rendered.push_str(&render_styled(&format!(" {args}"), args_color, None));
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
    let normalized = headline.trim().trim_start_matches("MCP ");
    if action_label == "Run command" {
        if normalized.is_empty() {
            return "Ran command".to_string();
        }
        return format!("Ran {normalized}");
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
    format!("{action_label} {normalized}")
}

fn run_summary_is_placeholder(summary: &str) -> bool {
    let Some(command) = summary.strip_prefix("Ran ") else {
        return false;
    };
    let normalized = command.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "command" | "bash" | "run pty cmd" | "unified exec" | "use unified exec" | "use run pty cmd" | "use bash"
    )
}

pub(crate) fn stream_label_from_output(output: &Value, command_success: bool) -> Option<&'static str> {
    let has_output = output.get("output").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    let has_stdout = output.get("stdout").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    let has_stderr = output.get("stderr").and_then(Value::as_str).is_some_and(|s| !s.is_empty());
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

/// Returns the `"MCP "` prefix for MCP tools, or an empty string otherwise.
fn mcp_label(is_mcp_tool: bool) -> &'static str {
    if is_mcp_tool { "MCP " } else { "" }
}

pub(crate) fn describe_tool_action(
    tool_name: &str,
    args: &Value,
    workspace_root: Option<&Path>,
) -> (String, HashSet<String>) {
    // Check if this is an MCP tool based on the original naming convention
    // MCP tools are named with an `mcp::`, `mcp__`, or `mcp_` prefix. A bare
    // `fetch` is the built-in web-fetch tool and must not be labeled as MCP.
    let is_mcp_tool = tool_name.starts_with("mcp::") || tool_name.starts_with("mcp_");

    // For the actual matching, we need to use the tool name without the "mcp_" prefix
    let actual_tool_name = if tool_name.starts_with("mcp__") {
        tool_name.split("__").last().unwrap_or(tool_name)
    } else if let Some(stripped) = tool_name.strip_prefix("mcp_") {
        stripped
    } else if tool_name.starts_with("mcp::") {
        // For tools in mcp::provider::name format, extract just the tool name
        tool_name.split("::").last().unwrap_or(tool_name)
    } else {
        tool_name
    };

    let with_mcp = |desc: String, used: HashSet<String>| -> (String, HashSet<String>) {
        (format!("{}{}", mcp_label(is_mcp_tool), desc), used)
    };
    let fallback =
        |label: &str| -> (String, HashSet<String>) { (format!("{}{}", mcp_label(is_mcp_tool), label), HashSet::new()) };

    match actual_tool_name {
        actual_name if tool_intent::is_command_run_tool(actual_name) => describe_shell_command(args)
            .map(|(desc, used)| with_mcp(desc, used))
            .unwrap_or_else(|| fallback("command")),
        actual_name if actual_name == tool_names::UNIFIED_EXEC => {
            match tool_intent::command_session_action(args).unwrap_or("run") {
                "run" => describe_shell_command(args)
                    .map(|(desc, used)| with_mcp(desc, used))
                    .unwrap_or_else(|| fallback("command")),
                "write" => with_mcp("Send command input".into(), HashSet::new()),
                "poll" => with_mcp("Read command session".into(), HashSet::new()),
                "continue" => with_mcp("Continue command session".into(), HashSet::new()),
                "inspect" => with_mcp("Inspect command output".into(), HashSet::new()),
                "list" => with_mcp("List command sessions".into(), HashSet::new()),
                "close" => with_mcp("Close command session".into(), HashSet::new()),
                "code" => with_mcp("Run code".into(), HashSet::new()),
                _ => with_mcp("exec_command".into(), HashSet::new()),
            }
        }
        actual_name if actual_name == tool_names::LIST_FILES => describe_list_files(args, workspace_root)
            .map(|(desc, used)| with_mcp(desc, used))
            .unwrap_or_else(|| fallback("List files")),
        actual_name if actual_name == tool_names::GREP_FILE => describe_grep_file(args, workspace_root)
            .map(|(desc, used)| with_mcp(desc, used))
            .unwrap_or_else(|| fallback("Search with grep")),
        actual_name if actual_name == tool_names::READ_FILE => {
            describe_path_action(args, "Read file", &["path"], workspace_root)
                .map(|(desc, used)| with_mcp(desc, used))
                .unwrap_or_else(|| fallback("Read file"))
        }
        actual_name if actual_name == tool_names::WRITE_FILE => {
            describe_path_action(args, "Write file", &["path"], workspace_root)
                .map(|(desc, used)| with_mcp(desc, used))
                .unwrap_or_else(|| fallback("Write file"))
        }
        actual_name if actual_name == tool_names::EDIT_FILE => {
            describe_path_action(args, "Edit file", &["path"], workspace_root)
                .map(|(desc, used)| with_mcp(desc, used))
                .unwrap_or_else(|| fallback("Edit file"))
        }
        actual_name if actual_name == tool_names::CREATE_FILE => {
            describe_path_action(args, "Create file", &["path"], workspace_root)
                .map(|(desc, used)| with_mcp(desc, used))
                .unwrap_or_else(|| fallback("Create file"))
        }
        actual_name if actual_name == tool_names::UNIFIED_FILE => {
            let action = file_operation_action(args);

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

            describe_path_action(args, verb, keys, workspace_root)
                .map(|(desc, used)| with_mcp(desc, used))
                .unwrap_or_else(|| with_mcp(verb.into(), HashSet::new()))
        }
        actual_name if actual_name == tool_names::DELETE_FILE => {
            describe_path_action(args, "Delete file", &["path"], workspace_root)
                .map(|(desc, used)| with_mcp(desc, used))
                .unwrap_or_else(|| fallback("Delete file"))
        }
        actual_name if actual_name == tool_names::APPLY_PATCH => {
            let prefix = mcp_label(is_mcp_tool);
            match extract_first_patch_file_path(args) {
                Some(path) => (format!("{prefix}Apply patch to {path}"), HashSet::from(["path".to_string()])),
                None => with_mcp("Apply workspace patch".into(), HashSet::new()),
            }
        }
        "fetch" | tool_names::WEB_FETCH => {
            let (desc, used) = describe_fetch_action(args);
            with_mcp(desc, used)
        }
        _ => with_mcp(format!("Use {}", humanize_tool_name(actual_tool_name)), HashSet::new()),
    }
}

pub(crate) fn humanize_tool_name(name: &str) -> String {
    crate::agent::runloop::unified::tool_summary_helpers::humanize_tool_name(name)
}

/// Extract the first file path from a patch's content.
///
/// Patch format uses lines like `*** Update File: path`, `*** Add File: path`,
/// or `*** Delete File: path`. This function parses the patch text from `input`
/// or `patch` args and returns the first file path found.
fn extract_first_patch_file_path(args: &Value) -> Option<String> {
    let patch_text = args.get("input").or_else(|| args.get("patch")).and_then(Value::as_str)?;

    for line in patch_text.lines() {
        let trimmed = line.trim();
        for prefix in &["*** Update File: ", "*** Add File: ", "*** Delete File: "] {
            if let Some(path) = trimmed.strip_prefix(prefix) {
                let path = path.trim();
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vtcode_commons::formatting::wrap_text_words;
    use vtcode_core::config::constants::tools as tool_names;

    use super::{build_tool_summary, describe_tool_action, run_summary_is_placeholder};

    #[test]
    fn build_tool_summary_formats_run_command_as_ran() {
        assert_eq!(build_tool_summary("Run command", "cargo check -p vtcode"), "Ran cargo check -p vtcode");
    }

    #[test]
    fn describe_tool_action_handles_unified_exec_run_command() {
        let (description, used_keys) = describe_tool_action(
            tool_names::UNIFIED_EXEC,
            &json!({
                "action": "run",
                "command": "cargo check -p vtcode"
            }),
            None,
        );

        assert_eq!(description, "cargo check -p vtcode");
        assert!(used_keys.contains("command"));
    }

    #[test]
    fn describe_tool_action_annotates_skill_doc_reads_with_skill_name() {
        let (description, used_keys) = describe_tool_action(
            tool_names::READ_FILE,
            &json!({
                "path": "/tmp/pr-babysitter/SKILL.md"
            }),
            None,
        );

        assert_eq!(description, "Read file /tmp/pr-babysitter/SKILL.md (pr-babysitter skill)");
        assert!(used_keys.contains("path"));
    }

    #[test]
    fn describe_tool_action_annotates_unified_file_skill_doc_reads_with_skill_name() {
        let (description, used_keys) = describe_tool_action(
            tool_names::UNIFIED_FILE,
            &json!({
                "action": "read",
                "path": "skills/code-review-skill/SKILL.md"
            }),
            None,
        );

        assert_eq!(description, "Read file skills/code-review-skill/SKILL.md (code-review-skill skill)");
        assert!(used_keys.contains("path"));
    }

    #[test]
    fn run_summary_placeholder_detection_catches_generic_labels() {
        assert!(run_summary_is_placeholder("Ran Use Unified exec"));
        assert!(run_summary_is_placeholder("Ran command"));
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

    #[test]
    fn extract_first_patch_file_path_from_update() {
        let args = json!({
            "input": "*** Begin Patch\n*** Update File: src/main.rs\n@@ -1,3 +1,4 @@\n+use std::io;\n*** End Patch"
        });
        assert_eq!(super::extract_first_patch_file_path(&args), Some("src/main.rs".to_string()));
    }

    #[test]
    fn extract_first_patch_file_path_from_add() {
        let args = json!({
            "patch": "*** Begin Patch\n*** Add File: new_file.txt\n+Hello\n*** End Patch"
        });
        assert_eq!(super::extract_first_patch_file_path(&args), Some("new_file.txt".to_string()));
    }

    #[test]
    fn extract_first_patch_file_path_from_delete() {
        let args = json!({
            "input": "*** Begin Patch\n*** Delete File: old.txt\n*** End Patch"
        });
        assert_eq!(super::extract_first_patch_file_path(&args), Some("old.txt".to_string()));
    }

    #[test]
    fn extract_first_patch_file_path_returns_first_of_multiple() {
        let args = json!({
            "input": "*** Begin Patch\n*** Update File: a.rs\n+line\n*** Update File: b.rs\n+line\n*** End Patch"
        });
        assert_eq!(super::extract_first_patch_file_path(&args), Some("a.rs".to_string()));
    }

    #[test]
    fn extract_first_patch_file_path_returns_none_for_missing() {
        let args = json!({"input": "not a valid patch"});
        assert_eq!(super::extract_first_patch_file_path(&args), None);
    }

    #[test]
    fn describe_tool_action_apply_patch_shows_file_path() {
        let (description, used_keys) = describe_tool_action(
            tool_names::APPLY_PATCH,
            &json!({
                "input": "*** Begin Patch\n*** Update File: src/lib.rs\n+line\n*** End Patch"
            }),
            None,
        );
        assert_eq!(description, "Apply patch to src/lib.rs");
        assert!(used_keys.contains("path"));
    }

    #[test]
    fn describe_tool_action_apply_patch_falls_back_without_path() {
        let (description, used_keys) =
            describe_tool_action(tool_names::APPLY_PATCH, &json!({"input": "not a patch"}), None);
        assert_eq!(description, "Apply workspace patch");
        assert!(used_keys.is_empty());
    }
}
