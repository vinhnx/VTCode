use hashbrown::HashSet;
use std::path::Path;

use anstyle::{Color, Reset, Style as AnsiStyle};
use anyhow::Result;
use serde_json::Value;
use vtcode_commons::color_policy;
use vtcode_commons::formatting::{wrap_shell_command_lines, wrap_shell_command_with_continuations};
use vtcode_commons::ui_protocol::{CompactToolSummaryLine, CompactToolSummaryLineKind};

use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tools::registry::labels::{tool_action_label, unified_exec_action_label, write_stdin_action_label};
use vtcode_core::tools::tool_intent;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};
use vtcode_ui::tui::ui::syntax_highlight;

use crate::agent::runloop::tool_output::render_tree_detail;
use crate::agent::runloop::unified::tool_summary_helpers::{
    RAN_COMMAND_CONTINUATION_WIDTH, RAN_COMMAND_FIRST_WIDTH, collect_param_details, command_line_for_args,
    describe_code_search, describe_fetch_action, describe_grep_file, describe_list_files, describe_path_action,
    describe_shell_command, display_command_text, exec_session_param_detail, highlight_texts_for_summary,
    is_exec_session_call, mcp_tool_display_name, relativize_command_paths, relativize_to_workspace,
    should_render_command_line, truncate_path_middle,
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

/// Transcript indicator shown immediately when plan synthesis starts.
///
/// Plan synthesis can run for a long time with no tool calls (research is
/// done, the model is writing one `<proposed_plan>` block). Without a
/// transcript row the TUI looks stalled: only the footer status moves. This
/// mirrors the `❋ Applying patch to …` pre-execution indicator so the user
/// sees plan work start, and the TUI sweeps a shimmer across the row while
/// the footer status (`Drafting plan...`, `Validating plan...`, …) stays
/// live. Sends are non-blocking (`renderer.line` → unbounded inline channel)
/// and planning stays a foreground non-busy stage, so input remains enabled.
pub(crate) const PLANNING_RESEARCHING_INDICATOR: &str = "❋ Drafting plan — researching codebase...";
pub(crate) const PLANNING_VALIDATING_INDICATOR: &str = "❋ Validating plan...";
pub(crate) const PLANNING_PERSISTING_INDICATOR: &str = "❋ Persisting plan...";

pub(crate) fn render_planning_progress_indicator(renderer: &mut AnsiRenderer, text: &str) -> Result<()> {
    renderer.line(MessageStyle::Tool, text)
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
    bullet_color: Color,
) -> Result<()> {
    let data = prepare_summary_data(tool_name, args, ctx.workspace_root, stream_label);

    if renderer.is_compact_display() {
        return render_compact_tool_summary_data(renderer, &data);
    }

    render_expanded_tool_summary_data(renderer, &data, bullet_color)
}

pub(crate) fn render_expanded_tool_call_summary(
    renderer: &mut AnsiRenderer,
    tool_name: &str,
    args: &Value,
    stream_label: Option<&str>,
    ctx: &ToolSummaryRenderContext,
    bullet_color: Color,
) -> Result<()> {
    let data = prepare_summary_data(tool_name, args, ctx.workspace_root, stream_label);
    render_expanded_tool_summary_data(renderer, &data, bullet_color)
}

fn render_expanded_tool_summary_data(
    renderer: &mut AnsiRenderer,
    data: &SummaryData,
    bullet_color: Color,
) -> Result<()> {
    let theme_styles = theme::active_styles();
    let main_color = theme_styles
        .primary
        .get_fg_color()
        .unwrap_or(Color::Ansi(anstyle::AnsiColor::Green));
    let palette = ColorPalette::default();

    let mut line = String::with_capacity(128);
    line.push_str(&render_styled("•", bullet_color, None));
    line.push(' ');

    let wrapped_run_segments = render_bullet_line(&mut line, data, main_color, &palette);

    renderer.line_with_override_style(MessageStyle::Info, AnsiStyle::new(), &line)?;

    render_continuation_lines(renderer, &wrapped_run_segments, main_color, &palette)?;
    render_command_line(renderer, &data.command_line, &palette)?;
    render_details(renderer, &data.details)?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SummaryData {
    summary: String,
    summary_highlights: Vec<String>,
    command_line: Option<String>,
    details: Vec<String>,
    /// Stream label to append to the header row, already filtered of labels
    /// that a session row makes redundant.
    stream_label: Option<String>,
}

fn compact_summary_expanded_lines(data: &SummaryData) -> Vec<CompactToolSummaryLine> {
    let mut lines = Vec::new();
    if let Some(command) = data.summary.strip_prefix("Ran ") {
        let wrapped =
            wrap_shell_command_with_continuations(command, RAN_COMMAND_FIRST_WIDTH, RAN_COMMAND_CONTINUATION_WIDTH);
        let first = wrapped.first().map(String::as_str).unwrap_or("command");
        lines.push(CompactToolSummaryLine {
            kind: CompactToolSummaryLineKind::Info,
            text: format!("• Ran {first}"),
        });
        lines.extend(wrapped.iter().skip(1).map(|segment| CompactToolSummaryLine {
            kind: CompactToolSummaryLineKind::Info,
            text: format!("  │ {segment}"),
        }));
    } else {
        lines.push(CompactToolSummaryLine {
            kind: CompactToolSummaryLineKind::Info,
            text: format!(
                "• {}{}",
                data.summary,
                data.stream_label
                    .as_deref()
                    .map(|label| format!(" {label}"))
                    .unwrap_or_default()
            ),
        });
    }
    if let Some(command_line) = &data.command_line {
        lines.push(CompactToolSummaryLine {
            kind: CompactToolSummaryLineKind::Detail,
            text: format!("$ {command_line}"),
        });
    }
    lines.extend(data.details.iter().map(|detail| CompactToolSummaryLine {
        kind: CompactToolSummaryLineKind::Detail,
        text: detail.clone(),
    }));
    lines
}

/// Stream label to show beside a tool header.
///
/// `• Send command input output` reads as a stutter because the captured body
/// is already visible below the row, so the generic capture label is dropped for
/// exec-session calls. Diagnostic labels (`error`, `stderr`, `stdout`, `stdio`)
/// are kept: a failed session read must not lose its failure signal.
fn resolved_stream_label(stream_label: Option<&str>, is_exec_session: bool) -> Option<String> {
    let label = stream_label?;
    if is_exec_session && label == "output" {
        return None;
    }
    Some(label.to_string())
}

fn prepare_summary_data(
    tool_name: &str,
    args: &Value,
    workspace_root: Option<&Path>,
    stream_label: Option<&str>,
) -> SummaryData {
    let (headline, highlights) = describe_tool_action(tool_name, args, workspace_root);
    let command_line_candidate = command_line_for_args(args).map(|cmd| relativize_command_paths(&cmd, workspace_root));
    let summary_highlights = highlight_texts_for_summary(args, &highlights, workspace_root);
    let action_label = tool_action_label(tool_name, args);
    let is_run_command = action_label == "Run command";
    // Expanded `• Ran` headlines must show the command in full: the truncated
    // preview stays on compact/collapsed surfaces and `$` detail lines, while
    // the transcript headline wraps the complete command across `│` lines
    // (screenshot 2026-09-24: `| grep -v ".backup"` must survive).
    let full_run_command = is_run_command
        .then(|| display_command_text(args).map(|cmd| relativize_command_paths(&cmd, workspace_root)))
        .flatten();
    let is_exec_session = is_exec_session_call(tool_name, args);

    // Exec-session calls repeat on every poll/wait and carry plumbing the reader
    // does not need, so they collapse to one row instead of several. Command
    // launches already print the command as the headline.
    let details = if is_run_command {
        Vec::new()
    } else if is_exec_session {
        exec_session_param_detail(args).into_iter().collect()
    } else {
        collect_param_details(args, &highlights, workspace_root)
    };

    let mut summary = build_tool_summary(&action_label, &headline);
    if is_run_command {
        summary = full_run_command
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

    SummaryData {
        summary,
        summary_highlights,
        command_line,
        details,
        stream_label: resolved_stream_label(stream_label, is_exec_session),
    }
}

fn render_compact_tool_summary_data(renderer: &mut AnsiRenderer, data: &SummaryData) -> Result<()> {
    if let Some(command) = data.summary.strip_prefix("Ran ") {
        return renderer.render_compact_command_activity(command.to_string(), 0, None, None);
    }

    // A non-command result is a hard grouping boundary. Its own detail lines
    // remain visible in compact mode, so the next command starts a fresh row.
    renderer.flush_compact_command_group();
    for line in compact_summary_expanded_lines(data) {
        match line.kind {
            CompactToolSummaryLineKind::Info => {
                renderer.line_with_override_style(MessageStyle::Info, AnsiStyle::new(), &line.text)?;
            }
            CompactToolSummaryLineKind::Detail => render_tree_detail(renderer, &line.text)?,
        }
    }
    Ok(())
}

fn render_bullet_line(
    line: &mut String,
    data: &SummaryData,
    main_color: Color,
    palette: &ColorPalette,
) -> Option<Vec<String>> {
    let mut wrapped_run_segments: Option<Vec<String>> = None;
    if let Some(command) = data.summary.strip_prefix("Ran ") {
        let wrapped = wrap_shell_command_lines(command, RAN_COMMAND_FIRST_WIDTH, RAN_COMMAND_CONTINUATION_WIDTH);
        let first_segment = wrapped.first().cloned().unwrap_or_else(|| "command".to_string());
        let is_multiline = wrapped.len() > 1;
        wrapped_run_segments = Some(wrapped);
        line.push_str(&render_styled("Ran", main_color, Some("bold".to_string())));
        line.push(' ');
        line.push_str(&render_command_segment(&first_segment, main_color, palette.muted, true));
        if is_multiline {
            line.push_str(&render_styled(" \\", palette.muted, Some("dim".to_string())));
        }
    } else {
        line.push_str(&render_summary_with_highlights(
            &data.summary,
            &data.summary_highlights,
            main_color,
            palette.accent,
            palette.muted,
        ));
        if let Some(stream) = &data.stream_label {
            line.push(' ');
            line.push_str(&render_styled(stream, palette.info, None));
        }
    }

    wrapped_run_segments
}

fn render_continuation_lines(
    renderer: &mut AnsiRenderer,
    wrapped_run_segments: &Option<Vec<String>>,
    main_color: Color,
    palette: &ColorPalette,
) -> Result<()> {
    if let Some(wrapped) = wrapped_run_segments {
        let total = wrapped.len();
        for (idx, segment) in wrapped.iter().skip(1).enumerate() {
            let mut continuation = String::with_capacity(segment.len() + 32);
            continuation.push_str("  ");
            continuation.push_str(&render_styled("│", palette.muted, Some("dim".to_string())));
            continuation.push(' ');
            continuation.push_str(&render_command_segment(segment, main_color, palette.muted, false));
            // All wrapped lines except the final one end with an explicit
            // shell continuation so the multi-line header reads as one command.
            let is_last = idx + 1 >= total - 1;
            if !is_last {
                continuation.push_str(&render_styled(" \\", palette.muted, Some("dim".to_string())));
            }
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
        rendered.push_str(&render_styled(&summary[start..end], accent_color, Some("bold".to_string())));
        cursor = end;
    }
    if cursor < summary.len() {
        rendered.push_str(&render_styled(&summary[cursor..], muted_color, None));
    }

    rendered
}

/// Render a command-line segment using the bash grammar syntax highlighter.
///
/// Tries the syntect bash grammar first (the same engine used by the live PTY
/// stream). When the grammar produces distinct foreground colors the result is
/// converted to ANSI escape sequences with `args_color` merged in for tokens
/// that have no explicit color. Falls back to a simple command-name / args
/// two-color scheme when the grammar is unavailable or yields a single color.
fn render_command_segment(segment: &str, command_color: Color, args_color: Color, expect_command: bool) -> String {
    if segment.is_empty() {
        return String::new();
    }

    if !color_policy::color_output_enabled() {
        return segment.to_string();
    }

    if let Some(highlighted) = try_bash_grammar_ansi(segment, args_color) {
        return highlighted;
    }

    // Fallback: simple two-color scheme.
    if expect_command {
        render_run_command_segment(segment, command_color, args_color)
    } else {
        render_styled(segment, args_color, None)
    }
}

/// Convert a command string to ANSI-highlighted text via the syntect bash
/// grammar, returning `None` when highlighting is unavailable or produces no
/// distinct foreground colors (in which case the caller should fall back).
fn try_bash_grammar_ansi(command: &str, fallback_color: Color) -> Option<String> {
    let theme_name = syntax_highlight::get_active_syntax_theme();
    let segments = syntax_highlight::highlight_line_to_anstyle_segments(command, Some("bash"), theme_name, true)?;
    if segments.is_empty() {
        return None;
    }

    // Reject results that altered the source text (syntect should be
    // purely additive with styling, but guard against grammar quirks).
    let reconstructed: String = segments.iter().map(|(_, text)| text.as_str()).collect();
    if reconstructed != command {
        return None;
    }

    // Only use the grammar result when it produces at least two distinct
    // foreground colors among non-whitespace tokens — a single color means
    // the grammar added no semantic value, so the two-color fallback is
    // more informative.
    let non_ws: Vec<&(AnsiStyle, String)> = segments.iter().filter(|(_, t)| !t.trim().is_empty()).collect();
    if non_ws.len() > 1 {
        let colors: HashSet<Option<Color>> = non_ws.iter().map(|(style, _)| style.get_fg_color()).collect();
        if colors.len() <= 1 {
            return None;
        }
    }

    let mut output = String::with_capacity(command.len() + segments.len() * 12);
    for (style, text) in &segments {
        let mut effective = *style;
        if effective.get_fg_color().is_none() {
            effective = effective.fg_color(Some(fallback_color));
        }
        output.push_str(&effective.to_string());
        output.push_str(text);
        output.push_str(&Reset.to_string());
    }
    Some(output)
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
    let action = action_label.trim().trim_start_matches("MCP ");
    if action == "Run command" {
        if normalized.is_empty() {
            return "Ran command".to_string();
        }
        return format!("Ran {normalized}");
    }
    if normalized.is_empty() {
        return action.to_string();
    }
    if normalized == action {
        return normalized.to_string();
    }
    if normalized.starts_with(action) {
        return normalized.to_string();
    }
    if let Some(stripped) = normalized.strip_prefix("Use ")
        && same_words_ignoring_case(stripped, action)
    {
        return action.to_string();
    }
    // Symmetric case: a generic "Use X" action label must never prefix a
    // canonical headline (e.g. action "Use Code search" vs headline
    // "Search code for …"). Prefer the specific headline when it opens with
    // the same word bag as the action's.
    if let Some(stripped_action) = action.strip_prefix("Use ")
        && (headline_starts_with_word_bag(normalized, stripped_action) || normalized.starts_with(stripped_action))
    {
        return normalized.to_string();
    }
    format!("{action} {normalized}")
}

/// Whether `headline` opens with the same word bag as `action_phrase`
/// (case-insensitive, order-insensitive), so a generic "Use Code search"
/// label collapses against "Search code for …". Delegates to
/// [`same_words_ignoring_case`] so there is one bag-compare implementation.
fn headline_starts_with_word_bag(headline: &str, action_phrase: &str) -> bool {
    let width = action_phrase.split_whitespace().count();
    if width == 0 {
        return false;
    }
    let head = headline.split_whitespace().take(width).collect::<Vec<_>>().join(" ");
    same_words_ignoring_case(&head, action_phrase)
}

/// Case- and word-order-insensitive equality for short display phrases, so a
/// headline like "Use Code search" still collapses against the label
/// "Search code" instead of concatenating into "Search code Use Code search".
fn same_words_ignoring_case(a: &str, b: &str) -> bool {
    let mut left: Vec<String> = a.split_whitespace().map(|word| word.to_ascii_lowercase()).collect();
    let mut right: Vec<String> = b.split_whitespace().map(|word| word.to_ascii_lowercase()).collect();
    if left.len() != right.len() {
        return false;
    }
    left.sort_unstable();
    right.sort_unstable();
    left == right
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
    // MCP tools are named with an `mcp::`, `mcp__`, or `mcp_` prefix. A bare
    // `fetch` is the built-in web-fetch tool and must not be labeled as MCP.
    // One parser call decides both the label and the bare tool name, so the
    // two can never disagree.
    let actual_tool_name = mcp_tool_display_name(tool_name);
    let is_mcp_tool = actual_tool_name.is_some();
    let actual_tool_name = actual_tool_name.unwrap_or(tool_name);

    let with_mcp = |desc: String, used: HashSet<String>| -> (String, HashSet<String>) {
        (format!("{}{}", mcp_label(is_mcp_tool), desc), used)
    };
    let fallback =
        |label: &str| -> (String, HashSet<String>) { (format!("{}{}", mcp_label(is_mcp_tool), label), HashSet::new()) };

    match actual_tool_name {
        _ if tool_intent::is_command_run_tool_call(tool_name, args) => describe_shell_command(args)
            .map(|(desc, used)| with_mcp(desc, used))
            .unwrap_or_else(|| fallback("command")),
        actual_name if actual_name == tool_names::UNIFIED_EXEC => {
            // Single source of truth lives in `vtcode-core` labels (DRY):
            // `run` shows the command text, every other action reuses the
            // canonical session label so TUI and approval surfaces agree.
            // Check the action, not the label text, so a label rename can
            // never silently reroute the run headline (KISS).
            let is_run =
                tool_intent::command_session_action(args).is_none_or(|action| action.eq_ignore_ascii_case("run"));
            if is_run {
                describe_shell_command(args)
                    .map(|(desc, used)| with_mcp(desc, used))
                    .unwrap_or_else(|| fallback("command"))
            } else {
                with_mcp(unified_exec_action_label(args).to_string(), HashSet::new())
            }
        }
        // Session follow-ups delegate to `vtcode-core` labels (DRY). A pure
        // wait/poll must not render as a send (screenshot 2026-10-02).
        actual_name if actual_name == tool_names::WRITE_STDIN => {
            with_mcp(write_stdin_action_label(args).to_string(), HashSet::new())
        }
        actual_name if actual_name == tool_names::SEND_PTY_INPUT => {
            with_mcp("Send command input".into(), HashSet::new())
        }
        actual_name if actual_name == tool_names::READ_PTY_SESSION => {
            with_mcp("Read command session".into(), HashSet::new())
        }
        actual_name if actual_name == tool_names::LIST_FILES => describe_list_files(args, workspace_root)
            .map(|(desc, used)| with_mcp(desc, used))
            .unwrap_or_else(|| fallback("List files")),
        actual_name if actual_name == tool_names::GREP_FILE => describe_grep_file(args, workspace_root)
            .map(|(desc, used)| with_mcp(desc, used))
            .unwrap_or_else(|| fallback("Search with grep")),
        actual_name if actual_name == tool_names::CODE_SEARCH => describe_code_search(args)
            .map(|(desc, used)| with_mcp(desc, used))
            .unwrap_or_else(|| fallback("Search code")),
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
        actual_name
            if actual_name == tool_names::WEB_FETCH
                || actual_name == tool_names::FETCH_URL
                || actual_name == tool_names::FETCH
                || actual_name == tool_names::DEFUDDLE_FETCH =>
        {
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
    use vtcode_core::utils::ansi::AnsiRenderer;
    use vtcode_ui::tui::app::{InlineCommand, InlineHandle};

    use super::{
        ToolSummaryRenderContext, build_tool_summary, describe_tool_action, prepare_summary_data,
        render_tool_call_summary, run_summary_is_placeholder,
    };
    use vtcode_core::tools::registry::labels::tool_action_label;

    #[test]
    fn build_tool_summary_formats_run_command_as_ran() {
        assert_eq!(build_tool_summary("Run command", "cargo check -p vtcode"), "Ran cargo check -p vtcode");
    }

    #[test]
    fn build_tool_summary_collapses_use_prefix_with_word_order_drift() {
        assert_eq!(build_tool_summary("Search code", "Use Code search"), "Search code");
        assert_eq!(build_tool_summary("Read file", "Use read file"), "Read file");
        assert_eq!(
            build_tool_summary("Search code", "Use Code search workspace"),
            "Search code Use Code search workspace"
        );
    }

    #[test]
    fn build_tool_summary_prefers_headline_over_generic_use_action() {
        // A generic "Use X" action label must never prefix a canonical
        // headline: prefer "Search code for …" over
        // "Use Code search Search code for …".
        assert_eq!(
            build_tool_summary("Use Code search", "Search code for core agent loop"),
            "Search code for core agent loop"
        );
        assert_eq!(build_tool_summary("Use Code search", "Search code"), "Search code");
    }

    #[test]
    fn describe_tool_action_code_search_describes_query() {
        let (description, used_keys) = describe_tool_action(
            tool_names::CODE_SEARCH,
            &json!({
                "query": "core agent loop",
                "file_types": ["rs"],
                "max_results": 15
            }),
            None,
        );

        assert_eq!(description, "Search code for core agent loop");
        assert!(used_keys.contains("query"));
        assert!(!used_keys.contains("max_results"));
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
    fn prepare_summary_data_folds_exec_session_params_into_one_row() {
        let args = json!({
            "session_id": "run-2d5752f2",
            "chars": "y\n",
            "wait_timeout_seconds": 600,
            "max_output_tokens": 4000,
            "max_tokens": 4000
        });
        let data = prepare_summary_data(tool_names::UNIFIED_EXEC, &args, None, Some("output"));

        // The four plumbing rows from the screenshot collapse to one row that
        // carries only the session identity and the wait deadline.
        assert_eq!(data.summary, "Send command input");
        assert_eq!(data.details, vec!["Session run-2d5752f2 · wait up to 600s".to_string()]);
        // The generic capture label beside a session row reads as a stutter.
        assert_eq!(data.stream_label, None);
    }

    #[test]
    fn prepare_summary_data_uses_public_write_stdin_name() {
        // The legacy `write_stdin` name must collapse to the action label rather
        // than leaving a `Use write_stdin` tail after `Send command input`, and
        // its plumbing must not re-leak through the generic detail collector.
        let args = json!({
            "session_id": "run-abc",
            "chars": "y\n",
            "yield_time_ms": 1000,
            "wait_timeout_seconds": 600,
            "max_output_tokens": 4000,
            "max_tokens": 4000
        });
        let data = prepare_summary_data(tool_names::WRITE_STDIN, &args, None, Some("output"));

        assert_eq!(data.summary, "Send command input");
        assert_eq!(data.details, vec!["Session run-abc · wait up to 600s".to_string()]);
        assert_eq!(data.stream_label, None);
    }

    #[test]
    fn prepare_summary_data_labels_pure_waits_as_waits() {
        // Screenshot 2026-10-02: two pure waits (no stdin payload) rendered as
        // `Send command input`. A wait with no input must read as a wait.
        for tool in [tool_names::WRITE_STDIN, tool_names::UNIFIED_EXEC] {
            let args = json!({
                "session_id": "run-6b54fe59",
                "action": "wait",
                "wait_timeout_seconds": 60
            });
            let data = prepare_summary_data(tool, &args, None, Some("output"));
            assert_eq!(data.summary, "Wait for command session", "tool: {tool}");
            assert_eq!(data.details, vec!["Session run-6b54fe59 · wait up to 60s".to_string()]);
        }
    }

    #[test]
    fn prepare_summary_data_labels_session_poll_without_input_as_read() {
        // A session_id-only follow-up polls output; it must not claim to send.
        let args = json!({ "session_id": "run-abc" });
        let data = prepare_summary_data(tool_names::WRITE_STDIN, &args, None, Some("output"));
        assert_eq!(data.summary, "Read command session");
    }

    #[test]
    fn prepare_summary_data_keeps_failure_label_on_session_calls() {
        // Dropping every session label would hide a failed read's signal, so
        // only the redundant capture label is suppressed.
        let args = json!({ "session_id": "run-abc", "action": "wait" });
        let data = prepare_summary_data(tool_names::UNIFIED_EXEC, &args, None, Some("error"));

        assert_eq!(data.stream_label.as_deref(), Some("error"));
    }

    #[test]
    fn prepare_summary_data_keeps_stream_label_for_ordinary_tools() {
        let args = json!({ "path": "src/lib.rs", "start_line": 10, "offset": 0 });
        let data = prepare_summary_data(tool_names::READ_FILE, &args, None, Some("content"));

        assert_eq!(data.details, vec!["Start line: 10".to_string()]);
        assert_eq!(data.stream_label.as_deref(), Some("content"));
    }

    #[test]
    fn prepare_summary_data_hides_output_caps_on_ordinary_tools() {
        // The cap is plumbing for every tool, not just session follow-ups.
        let args = json!({ "query": "tool intent", "max_output_tokens": 200, "limit": 5 });
        let data = prepare_summary_data(tool_names::SEARCH_TOOLS, &args, None, None);
        assert_eq!(data.details, vec!["Limit: 5".to_string()]);
    }

    #[test]
    fn describe_tool_action_labels_pty_session_readers() {
        let args = json!({ "session_id": "run-abc" });
        for (tool, label) in [
            (tool_names::SEND_PTY_INPUT, "Send command input"),
            (tool_names::READ_PTY_SESSION, "Read command session"),
        ] {
            let (headline, _) = describe_tool_action(tool, &args, None);
            assert_eq!(headline, label);
            assert_eq!(tool_action_label(tool, &args), label);
        }
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

    #[test]
    fn try_bash_grammar_ansi_highlights_cargo_command() {
        let result = super::try_bash_grammar_ansi(
            "cargo check --locked -p vtcode",
            anstyle::Color::Ansi(anstyle::AnsiColor::White),
        );
        assert!(result.is_some(), "cargo command should get bash grammar highlighting");
        let ansi = result.unwrap();
        // ANSI escape sequences present (ESC = 0x1b)
        assert!(ansi.contains('\u{1b}'), "output should contain ANSI escape codes");
        // Plain text is preserved when ANSI codes are stripped
        let stripped = vtcode_commons::ansi::strip_ansi_codes(&ansi);
        assert_eq!(stripped, "cargo check --locked -p vtcode");
    }

    #[test]
    fn try_bash_grammar_ansi_highlights_sed_command() {
        let result = super::try_bash_grammar_ansi(
            "sed -n 178,190p src/main_helpers/bootstrap.rs",
            anstyle::Color::Ansi(anstyle::AnsiColor::White),
        );
        assert!(result.is_some(), "sed command should get bash grammar highlighting");
        let ansi = result.unwrap();
        assert!(ansi.contains('\u{1b}'));
        let stripped = vtcode_commons::ansi::strip_ansi_codes(&ansi);
        assert_eq!(stripped, "sed -n 178,190p src/main_helpers/bootstrap.rs");
    }

    #[test]
    fn try_bash_grammar_ansi_highlights_rg_command() {
        let result = super::try_bash_grammar_ansi(
            "rg -n use std::time::Instant",
            anstyle::Color::Ansi(anstyle::AnsiColor::White),
        );
        assert!(result.is_some(), "rg command should get bash grammar highlighting");
        let ansi = result.unwrap();
        assert!(ansi.contains('\u{1b}'));
        let stripped = vtcode_commons::ansi::strip_ansi_codes(&ansi);
        assert_eq!(stripped, "rg -n use std::time::Instant");
    }

    #[test]
    fn render_command_segment_falls_back_for_empty() {
        let result = super::render_command_segment(
            "",
            anstyle::Color::Ansi(anstyle::AnsiColor::Green),
            anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlack),
            true,
        );
        assert_eq!(result, "");
    }

    #[test]
    fn render_command_segment_produces_highlighted_output() {
        // Force color output on. The color policy uses a Lazy initializer that
        // reads NO_COLOR from the env; we must trigger it first, then override.
        // (nextest runs each test in its own process, so this is safe.)
        let _ = vtcode_commons::color_policy::current_color_output_policy();
        vtcode_commons::color_policy::set_color_output_policy(vtcode_commons::color_policy::ColorOutputPolicy {
            enabled: true,
            source: vtcode_commons::color_policy::ColorOutputPolicySource::CliColorAlways,
        });

        let result = super::render_command_segment(
            "cargo check --locked -p vtcode",
            anstyle::Color::Ansi(anstyle::AnsiColor::Green),
            anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlack),
            true,
        );
        // Should contain ANSI escape codes from bash grammar highlighting
        assert!(result.contains('\u{1b}'), "command segment should be syntax-highlighted");
        let stripped = vtcode_commons::ansi::strip_ansi_codes(&result);
        assert_eq!(stripped.as_ref(), "cargo check --locked -p vtcode");
    }

    #[test]
    fn inline_tool_summary_keeps_command_tokens_independently_styled() {
        let _ = vtcode_commons::color_policy::current_color_output_policy();
        vtcode_commons::color_policy::set_color_output_policy(vtcode_commons::color_policy::ColorOutputPolicy {
            enabled: true,
            source: vtcode_commons::color_policy::ColorOutputPolicySource::CliColorAlways,
        });

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
        renderer.set_tool_display_mode(vtcode_core::config::ToolDisplayMode::Expanded);
        let context = ToolSummaryRenderContext { workspace_root: None };

        render_tool_call_summary(
            &mut renderer,
            tool_names::UNIFIED_EXEC,
            &json!({"action": "run", "command": "find src/agent/runloop -maxdepth 3 -type f -name *.rs | sort"}),
            None,
            &context,
            anstyle::Color::Ansi(anstyle::AnsiColor::Green),
        )
        .expect("summary rendering should succeed");

        let commands: Vec<_> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
        let summary_segments = commands
            .iter()
            .find_map(|command| match command {
                InlineCommand::AppendLine { segments, .. }
                    if segments
                        .iter()
                        .map(|segment| segment.text.as_str())
                        .collect::<String>()
                        .contains("find") =>
                {
                    Some(segments.as_slice())
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                let texts: Vec<_> = commands
                    .iter()
                    .filter_map(|command| match command {
                        InlineCommand::AppendLine { segments, .. } => {
                            Some(segments.iter().map(|segment| segment.text.as_str()).collect::<String>())
                        }
                        _ => None,
                    })
                    .collect();
                panic!("expected inline command summary, got {texts:?}");
            });
        let command_color = summary_segments
            .iter()
            .find(|segment| segment.text.contains("find"))
            .and_then(|segment| segment.style.color);
        let option_color = summary_segments
            .iter()
            .find(|segment| segment.text.contains("maxdepth"))
            .and_then(|segment| segment.style.color);

        assert_ne!(command_color, option_color);
    }

    #[test]
    fn compact_tool_summary_keeps_each_call_visible() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
        renderer.set_tool_display_mode(vtcode_core::config::ToolDisplayMode::Compact);
        let context = ToolSummaryRenderContext { workspace_root: None };

        for max_results in [30, 100] {
            render_tool_call_summary(
                &mut renderer,
                tool_names::LIST_FILES,
                &json!({"path": ".", "max_results": max_results}),
                None,
                &context,
                anstyle::Color::Ansi(anstyle::AnsiColor::Green),
            )
            .expect("summary should render");
        }

        let text = std::iter::from_fn(|| receiver.try_recv().ok())
            .filter_map(|command| match command {
                InlineCommand::AppendLine { segments, .. } => {
                    Some(segments.into_iter().map(|s| s.text).collect::<String>())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(text.matches("• List files").count(), 2);
        assert!(text.contains("30"));
        assert!(text.contains("100"));
    }

    #[test]
    fn compact_command_summary_groups_contiguous_commands() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
        renderer.set_tool_display_mode(vtcode_core::config::ToolDisplayMode::Compact);
        let context = ToolSummaryRenderContext { workspace_root: None };

        for command in ["printf one", "printf two"] {
            render_tool_call_summary(
                &mut renderer,
                tool_names::UNIFIED_EXEC,
                &json!({"action": "run", "command": command}),
                None,
                &context,
                anstyle::Color::Ansi(anstyle::AnsiColor::Green),
            )
            .expect("summary should render");
        }

        let activities = std::iter::from_fn(|| receiver.try_recv().ok())
            .filter_map(|command| match command {
                InlineCommand::AppendCompactActivity(activity) | InlineCommand::ReplaceCompactActivity(activity) => {
                    Some(activity)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(activities.len(), 2);
        assert_eq!(activities[0].command_count, 1);
        assert_eq!(activities[0].display_text(), "• Ran printf one");
        assert_eq!(activities[1].command_count, 2);
        assert_eq!(activities[1].display_text(), "• Ran 2 commands");
    }

    #[test]
    fn compact_command_summary_groups_command_tool_aliases() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
        renderer.set_tool_display_mode(vtcode_core::config::ToolDisplayMode::Compact);
        let context = ToolSummaryRenderContext { workspace_root: None };

        for (tool_name, command) in [("exec", "printf one"), ("container.exec", "printf two")] {
            render_tool_call_summary(
                &mut renderer,
                tool_name,
                &json!({"command": command}),
                None,
                &context,
                anstyle::Color::Ansi(anstyle::AnsiColor::Green),
            )
            .expect("summary should render");
        }

        let activities = std::iter::from_fn(|| receiver.try_recv().ok())
            .filter_map(|command| match command {
                InlineCommand::AppendCompactActivity(activity) | InlineCommand::ReplaceCompactActivity(activity) => {
                    Some(activity)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(activities.len(), 2);
        assert_eq!(activities[1].command_count, 2);
        assert_eq!(activities[1].display_text(), "• Ran 2 commands");
    }

    #[test]
    fn compact_command_summary_flushes_before_non_command_results() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
        renderer.set_tool_display_mode(vtcode_core::config::ToolDisplayMode::Compact);
        let context = ToolSummaryRenderContext { workspace_root: None };

        for (tool_name, args) in [
            (tool_names::UNIFIED_EXEC, json!({"action": "run", "command": "printf one"})),
            (tool_names::LIST_FILES, json!({"path": "."})),
            (tool_names::UNIFIED_EXEC, json!({"action": "run", "command": "printf two"})),
        ] {
            render_tool_call_summary(
                &mut renderer,
                tool_name,
                &args,
                None,
                &context,
                anstyle::Color::Ansi(anstyle::AnsiColor::Green),
            )
            .expect("summary should render");
        }

        let activities = std::iter::from_fn(|| receiver.try_recv().ok())
            .filter_map(|command| match command {
                InlineCommand::AppendCompactActivity(activity) | InlineCommand::ReplaceCompactActivity(activity) => {
                    Some(activity)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(activities.len(), 2);
        assert!(activities.iter().all(|activity| activity.command_count == 1));
    }

    #[test]
    fn planning_indicators_carry_spinner_marker() {
        // The TUI transcript spinner matches indicator rows by their `❋`
        // text prefix (`transcript.rs::FILE_OPERATION_INDICATORS`). If an
        // indicator loses the marker, the planning row goes static while the
        // long synthesis runs — the exact stall this feature fixes.
        for indicator in [
            super::PLANNING_RESEARCHING_INDICATOR,
            super::PLANNING_VALIDATING_INDICATOR,
            super::PLANNING_PERSISTING_INDICATOR,
        ] {
            assert!(indicator.contains('❋'), "missing spinner marker in {indicator:?}");
            assert!(
                indicator.to_ascii_lowercase().contains("plan"),
                "indicator must name the plan phase: {indicator:?}"
            );
        }
    }

    #[test]
    fn render_planning_progress_indicator_emits_transcript_line() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());

        super::render_planning_progress_indicator(&mut renderer, super::PLANNING_RESEARCHING_INDICATOR)
            .expect("planning indicator should render");

        let text = std::iter::from_fn(|| receiver.try_recv().ok())
            .filter_map(|command| match command {
                InlineCommand::AppendLine { segments, .. } => {
                    Some(segments.into_iter().map(|s| s.text).collect::<String>())
                }
                InlineCommand::Inline { segment, .. } => Some(segment.text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Drafting plan"), "got: {text:?}");
    }

    #[test]
    fn prepare_summary_data_keeps_run_headline_in_full_without_truncation() {
        // Screenshot 2026-09-24: the expanded `• Ran` headline must carry the
        // complete pipeline so TUI wrapping (not `…`) owns the overflow.
        // `display_command_text` normalizes quoting (double to single), so
        // assert token completeness rather than byte equality. Exact
        // screenshot bytes: `||` in the pattern (3 pattern pipes + 3 shell
        // pipes = 6) and the backslash-escaped `\.backup` must survive.
        let command = "grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs | grep -v node_modules | grep -v package-lock | grep -v \"\\.backup\"";
        let data =
            prepare_summary_data(tool_names::UNIFIED_EXEC, &json!({"action": "run", "command": command}), None, None);
        assert!(!data.summary.contains('…'), "got: {:?}", data.summary);
        for fragment in [
            "grep",
            "-rn",
            "@vinhnx/vtcode|npm install -g||npx @vinhnx",
            "docs",
            "node_modules",
            "package-lock",
            "\\.backup",
        ] {
            assert!(data.summary.contains(fragment), "missing {fragment:?} in {:?}", data.summary);
        }
        assert_eq!(
            data.summary.matches('|').count(),
            6,
            "pattern pipes + shell pipes must survive: {:?}",
            data.summary
        );
    }
}
