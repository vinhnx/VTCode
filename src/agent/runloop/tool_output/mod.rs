mod commands;
mod commands_processing;
mod files;
pub(crate) mod large_output;
#[cfg(test)]
mod large_output_tests;
mod mcp;
mod panels;
mod streams;
pub(crate) mod styles;
mod tracker;
use tracker::render_tracker_view;
#[cfg(test)]
pub(crate) use tracker::tracker_tree_body_lines;
pub(crate) use tracker::{
    TRACKER_CURRENT_PREFIX, TrackerLine, humanize_tracker_title, is_tracker_current_row, tracker_panel_metadata,
    tracker_panel_rows, tracker_transcript_lines,
};
#[cfg(test)]
use tracker::{
    TRACKER_ROW_DESCRIPTION_MAX_BYTES, TRACKER_TRANSCRIPT_MAX_ROWS, tracker_current_tree_row, tracker_progress_lines,
    tracker_row_text, tracker_summary_lines,
};

// Re-export stream utilities
use anyhow::Result;
use commands::render_terminal_command_panel;
use files::{
    format_diff_content_lines_with_numbers, render_apply_patch_diff_preview, render_list_dir_output,
    render_read_file_output, render_write_file_preview,
};
use mcp::{render_context7_output, render_generic_output, render_sequential_output, resolve_renderer_profile};
use serde_json::Value;
use streams::render_stream_section;
pub(crate) use streams::{render_code_fence_blocks, resolve_stdout_tail_limit};
use styles::{GitStyles, LsStyles};
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::mcp::McpRendererProfile;
use vtcode_core::tools::continuation::{
    NEXT_CONTINUE_PROMPT, NEXT_READ_PROMPT, PtyContinuationArgs, ReadChunkContinuationArgs,
};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::style_helpers::{ColorPalette, render_styled};

pub(crate) fn spooled_output_hint(path: &str) -> String {
    format!(
        "Large output was spooled to \"{path}\". Use exec_command with shell tools such as cat, sed, or rg to inspect details."
    )
}

/// Render a detail line with tree prefix styling (└ prefix).
/// Used to unify output details with the tree structure used by other tools.
pub(crate) fn render_tree_detail(renderer: &mut AnsiRenderer, detail: &str) -> Result<()> {
    let palette = ColorPalette::default();
    let mut styled = String::new();
    push_tree_prefix(&mut styled, &palette);
    styled.push_str(&render_styled(detail, palette.muted, None));
    renderer.line(MessageStyle::Info, &styled)?;
    Ok(())
}

/// Push the shared `  └ ` tree-detail prefix (two-space indent, dim `└`, trailing
/// space) into `styled`. Centralized so the prefix style can change in one place
/// across both detail lines and command-line renderings.
pub(crate) fn push_tree_prefix(styled: &mut String, palette: &ColorPalette) {
    styled.push_str("  ");
    styled.push_str(&render_styled("└", palette.muted, Some("dim".to_string())));
    styled.push(' ');
}

fn tool_recovery_hint(val: &Value) -> Option<&'static str> {
    if !val.get("loop_detected").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    if val.get("spool_path").and_then(Value::as_str).is_some() {
        return Some("Loop detected; continue from spooled output.");
    }
    if val.get("fallback_tool").and_then(Value::as_str).is_some() {
        return Some("Loop detected; fallback is available.");
    }
    Some("Loop detected; change approach before retrying.")
}

fn push_tool_follow_up_hint(hints: &mut Vec<String>, hint: impl Into<String>) {
    let hint = hint.into();
    if hint.trim().is_empty() || hints.iter().any(|existing| existing == &hint) {
        return;
    }
    hints.push(hint);
}

fn tool_follow_up_hints(val: &Value) -> Vec<String> {
    let mut hints = Vec::with_capacity(5);
    if let Some(hint) = tool_recovery_hint(val) {
        push_tool_follow_up_hint(&mut hints, hint);
    }
    if let Some(next_action) = val.get("next_action").and_then(Value::as_str) {
        push_tool_follow_up_hint(&mut hints, next_action);
    }
    if let Some(path) = val.get("spool_path").and_then(Value::as_str) {
        push_tool_follow_up_hint(&mut hints, spooled_output_hint(path));
    }
    if val
        .get("next_continue_args")
        .and_then(PtyContinuationArgs::from_value)
        .is_some()
    {
        push_tool_follow_up_hint(&mut hints, NEXT_CONTINUE_PROMPT);
    } else if val
        .get("next_read_args")
        .and_then(ReadChunkContinuationArgs::from_value)
        .is_some()
    {
        push_tool_follow_up_hint(&mut hints, NEXT_READ_PROMPT);
    }
    hints
}

/// Return follow-up guidance in the same order used by the live renderer.
///
/// The transcript viewer stores complete command captures separately from the
/// compact live row. Keep this metadata in that capture so exporting or
/// reviewing the whole conversation does not lose recovery instructions that
/// were rendered after the command body.
pub(crate) fn tool_follow_up_hints_for_capture(val: &Value, rendered_output: Option<&str>) -> Vec<String> {
    tool_follow_up_hints(val)
        .into_iter()
        .filter(|hint| !rendered_output.is_some_and(|output| output.contains(hint.as_str())))
        .collect()
}

pub(super) fn render_tool_follow_up_hints(
    renderer: &mut AnsiRenderer,
    val: &Value,
    rendered_output: Option<&str>,
) -> Result<()> {
    let mut rendered_any = false;
    for hint in tool_follow_up_hints(val) {
        if rendered_output.is_some_and(|output| output.contains(hint.as_str())) {
            continue;
        }
        if !rendered_any {
            renderer.line(MessageStyle::ToolDetail, "")?;
            rendered_any = true;
        }
        renderer.line(MessageStyle::ToolDetail, &hint)?;
    }
    Ok(())
}

fn preferred_follow_up_rendered_body(val: &Value) -> Option<&str> {
    val.get("output")
        .and_then(Value::as_str)
        .or_else(|| val.get("content").and_then(Value::as_str))
}

fn render_tool_follow_up_hints_for_value(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    render_tool_follow_up_hints(renderer, val, preferred_follow_up_rendered_body(val))
}

async fn render_terminal_tool_output(
    renderer: &mut AnsiRenderer,
    val: &Value,
    vt_config: Option<&VTCodeConfig>,
    allow_tool_ansi: bool,
) -> Result<()> {
    let git_styles = GitStyles::new();
    let ls_styles = LsStyles::from_env();
    render_terminal_command_panel(renderer, val, &git_styles, &ls_styles, vt_config, allow_tool_ansi).await
}

pub(crate) async fn render_tool_output(
    renderer: &mut AnsiRenderer,
    tool_name: Option<&str>,
    val: &Value,
    vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    let allow_tool_ansi = vt_config.map(|cfg| cfg.ui.allow_tool_ansi).unwrap_or(false);
    let is_git_diff_output = is_git_diff_payload(val);

    match tool_name {
        Some(tools::WRITE_FILE) | Some(tools::CREATE_FILE) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_write_file_preview(renderer, val, &git_styles, &ls_styles);
        }
        Some(tools::EDIT_FILE) | Some(tools::SEARCH_REPLACE) | Some(tools::DELETE_FILE)
            if val.get("diff").is_some() || val.get("diff_preview").is_some() =>
        {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_write_file_preview(renderer, val, &git_styles, &ls_styles);
        }
        Some(tools::APPLY_PATCH) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_apply_patch_diff_preview(renderer, val, &git_styles, &ls_styles);
        }
        Some(tools::UNIFIED_FILE) => {
            if val.get("diff").is_some() || val.get("diff_preview").is_some() {
                let git_styles = GitStyles::new();
                let ls_styles = LsStyles::from_env();
                return render_write_file_preview(renderer, val, &git_styles, &ls_styles);
            }
            if val.get("content").is_some() {
                render_read_file_output(renderer, val)?;
                render_tool_follow_up_hints(renderer, val, val.get("content").and_then(Value::as_str))?;
                return Ok(());
            }
        }
        Some(tools::RUN_PTY_CMD)
        | Some(tools::READ_PTY_SESSION)
        | Some(tools::CREATE_PTY_SESSION)
        | Some(tools::SEND_PTY_INPUT)
        | Some(tools::CLOSE_PTY_SESSION)
        | Some(tools::RESIZE_PTY_SESSION)
        | Some(tools::LIST_PTY_SESSIONS)
        | Some(tools::EXEC_COMMAND)
        | Some(tools::EXEC_PTY_CMD) => {
            return render_terminal_tool_output(renderer, val, vt_config, allow_tool_ansi).await;
        }
        Some(tools::UNIFIED_EXEC) if !is_git_diff_output && should_render_command_session_terminal_panel(val) => {
            return render_terminal_tool_output(renderer, val, vt_config, allow_tool_ansi).await;
        }
        Some(tools::WEB_FETCH) => {
            render_generic_output(renderer, val)?;
            render_tool_follow_up_hints_for_value(renderer, val)?;
            return Ok(());
        }
        Some(tools::LIST_FILES) => {
            let ls_styles = LsStyles::from_env();
            render_list_dir_output(renderer, val, &ls_styles)?;
            render_tool_follow_up_hints_for_value(renderer, val)?;
            return Ok(());
        }
        Some(tools::READ_FILE) => {
            render_read_file_output(renderer, val)?;
            render_tool_follow_up_hints(renderer, val, val.get("content").and_then(Value::as_str))?;
            return Ok(());
        }
        Some(tools::EXECUTE_CODE) => {
            return render_terminal_tool_output(renderer, val, vt_config, allow_tool_ansi).await;
        }
        Some(tools::TASK_TRACKER) if render_tracker_view(renderer, val)? => {
            return Ok(());
        }
        _ => {}
    }

    render_simple_tool_status(renderer, tool_name, val)?;

    if let Some(notice) = val.get("security_notice").and_then(Value::as_str) {
        renderer.line(MessageStyle::ToolDetail, notice)?;
    }

    render_tool_follow_up_hints_for_value(renderer, val)?;

    if let Some(tool) = tool_name
        && tool.starts_with("mcp_")
    {
        if let Some(profile) = resolve_renderer_profile(tool, vt_config) {
            match profile {
                McpRendererProfile::Context7 => render_context7_output(renderer, val)?,
                McpRendererProfile::SequentialThinking => render_sequential_output(renderer, val)?,
            }
        } else {
            render_generic_output(renderer, val)?;
        }
        // Early return for MCP tools - don't fall through to other rendering logic
        return Ok(());
    }

    let output_mode = vt_config.map(|cfg| cfg.ui.tool_output_mode).unwrap_or(ToolOutputMode::Compact);
    let tail_limit = resolve_stdout_tail_limit(vt_config);
    let git_styles = GitStyles::new();
    let ls_styles = LsStyles::from_env();
    let disable_spool = val.get("no_spool").and_then(Value::as_bool).unwrap_or(false);

    // PTY tools use "output" field instead of "stdout"
    let stream_tool_name = if is_git_diff_output { None } else { tool_name };

    if let Some(output) = val.get("output").and_then(Value::as_str) {
        render_stream_section(
            renderer,
            "",
            output,
            output_mode,
            tail_limit,
            stream_tool_name,
            &git_styles,
            &ls_styles,
            MessageStyle::ToolOutput,
            allow_tool_ansi,
            disable_spool,
            vt_config,
        )
        .await?;
    } else if let Some(stdout) = val.get("stdout").and_then(Value::as_str) {
        render_stream_section(
            renderer,
            "stdout",
            stdout,
            output_mode,
            tail_limit,
            stream_tool_name,
            &git_styles,
            &ls_styles,
            MessageStyle::ToolOutput,
            allow_tool_ansi,
            disable_spool,
            vt_config,
        )
        .await?;
    }
    if let Some(stderr) = val.get("stderr").and_then(Value::as_str) {
        render_stream_section(
            renderer,
            "stderr",
            stderr,
            output_mode,
            tail_limit,
            tool_name,
            &git_styles,
            &ls_styles,
            MessageStyle::ToolError,
            allow_tool_ansi,
            disable_spool,
            vt_config,
        )
        .await?;
    }
    Ok(())
}

pub(crate) fn format_unified_diff_lines(diff_content: &str) -> Vec<String> {
    format_diff_content_lines_with_numbers(diff_content)
}

pub(crate) fn is_git_diff_payload(val: &Value) -> bool {
    val.get("content_type")
        .and_then(Value::as_str)
        .is_some_and(|content_type| content_type == "git_diff")
}

fn render_simple_tool_status(renderer: &mut AnsiRenderer, _tool_name: Option<&str>, val: &Value) -> Result<()> {
    let has_error = val.get("error").is_some() || val.get("error_type").is_some();

    if has_error {
        render_error_details(renderer, val)?;
    }

    Ok(())
}

fn should_render_command_session_terminal_panel(val: &Value) -> bool {
    let has_command = val
        .get("command")
        .map(|command| match command {
            Value::String(text) => !text.trim().is_empty(),
            Value::Array(parts) => !parts.is_empty(),
            _ => false,
        })
        .unwrap_or(false);
    let has_terminal_stream = val
        .get("output")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
        || val
            .get("stdout")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
        || val
            .get("stderr")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty());
    let has_session_context = ["id", "session_id", "process_id", "is_exited", "exit_code"]
        .iter()
        .any(|key| val.get(*key).is_some());

    !is_git_diff_payload(val) && (has_command || has_terminal_stream || has_session_context)
}

fn render_error_details(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    if let Some(error_msg) = val
        .get("message")
        .and_then(|v| v.as_str())
        .filter(|msg| !msg.trim().is_empty())
        .or_else(|| val.get("error").and_then(|v| v.as_str()).filter(|msg| !msg.trim().is_empty()))
    {
        renderer.line(MessageStyle::ToolError, &format!("Error: {error_msg}"))?;
    }

    if let Some(error_type) = val.get("error_type").and_then(|v| v.as_str()) {
        let type_description = match error_type {
            "InvalidParameters" => "Invalid parameters provided",
            "ToolNotFound" => "Tool not found",
            "ResourceNotFound" => "Resource not found",
            "PermissionDenied" => "Permission denied",
            "ExecutionError" => "Execution error",
            "PolicyViolation" => "Policy violation",
            "Timeout" => "Operation timed out",
            "NetworkError" => "Network error",
            "EncodingError" => "Encoding error",
            "FileSystemError" => "File system error",
            _ => error_type,
        };
        renderer.line(MessageStyle::ToolDetail, &format!("Type: {type_description}"))?;
    }

    if let Some(original) = val.get("original_error").and_then(|v| v.as_str())
        && !original.trim().is_empty()
    {
        let display_error = vtcode_commons::formatting::truncate_byte_budget(original, 197, "...");
        renderer.line(MessageStyle::ToolDetail, &format!("Details: {display_error}"))?;
    }

    if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::ToolDetail, &format!("Path: {path}"))?;
    }

    if let Some(line) = val.get("line").and_then(|v| v.as_u64()) {
        if let Some(col) = val.get("column").and_then(|v| v.as_u64()) {
            renderer.line(MessageStyle::ToolDetail, &format!("Location: line {line}, column {col}"))?;
        } else {
            renderer.line(MessageStyle::ToolDetail, &format!("Location: line {line}"))?;
        }
    }

    if let Some(suggestions) = val.get("recovery_suggestions").and_then(|v| v.as_array())
        && !suggestions.is_empty()
    {
        renderer.line(MessageStyle::ToolDetail, "")?;
        renderer.line(MessageStyle::ToolDetail, "Suggestions:")?;
        for (idx, suggestion) in suggestions.iter().take(5).enumerate() {
            if let Some(text) = suggestion.as_str() {
                renderer.line(MessageStyle::ToolDetail, &format!("{}. {}", idx + 1, text))?;
            }
        }
        if suggestions.len() > 5 {
            renderer.line(MessageStyle::ToolDetail, &format!("    ... and {} more", suggestions.len() - 5))?;
        }
    }

    Ok(())
}

#[cfg(test)]
pub(crate) fn collect_inline_output(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<vtcode_core::ui::InlineCommand>,
) -> String {
    let mut lines: Vec<String> = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        match command {
            vtcode_core::ui::InlineCommand::AppendLine { segments, .. } => {
                lines.push(segments.into_iter().map(|segment| segment.text).collect::<String>());
            }
            vtcode_core::ui::InlineCommand::ReplaceLast { lines: replacement_lines, .. } => {
                lines.extend(
                    replacement_lines
                        .into_iter()
                        .map(|line| line.into_iter().map(|segment| segment.text).collect::<String>()),
                );
            }
            vtcode_core::ui::InlineCommand::RecordDiffReview(anchor) => {
                lines.push(format!("RecordDiffReview {}", anchor.file_path));
                lines.push(anchor.notice);
            }
            _ => {}
        }
    }
    lines.join("\n")
}

/// Drain test-sink commands and return recorded diff-review anchors.
#[cfg(test)]
pub(crate) fn collect_inline_diff_review_anchors(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<vtcode_core::ui::InlineCommand>,
) -> Vec<vtcode_commons::ui_protocol::DiffReviewAnchor> {
    let mut anchors = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        if let vtcode_core::ui::InlineCommand::RecordDiffReview(anchor) = command {
            anchors.push(anchor);
        }
    }
    anchors
}

#[cfg(test)]
mod tests;
