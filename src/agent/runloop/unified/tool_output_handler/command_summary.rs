//! Compact command summary rendering for completed command outcomes.

use super::*;

/// Extract the command string from tool call arguments for display.
///
/// Uses display-safe joining (bare `|`, `>`, `;`; quotes only whitespace) so
pub(super) fn extract_command_line(args: &serde_json::Value) -> Option<String> {
    display_command_text(args)
}

/// Shared display → relativize → single-line preview pipeline for `• Ran …`
/// headers. Keeps the collapsed row and the viewer header consistent instead
/// of echoing a multi-line script.
pub(super) fn compact_command_preview(args: &serde_json::Value, workspace_root: Option<&Path>) -> Option<String> {
    display_command_text(args)
        .map(|command| relativize_command_paths(&command, workspace_root))
        .map(|command| preview_command(&command, COMPACT_PREVIEW_LEN))
        .filter(|command| !command.is_empty())
}

pub(super) fn compact_command_text(name: &str, args: &serde_json::Value, workspace_root: Option<&Path>) -> String {
    // Display join (no shell_words quoting) plus a first-line head-truncated
    // preview: the collapsed row must stay readable, not executable-looking.
    compact_command_preview(args, workspace_root).unwrap_or_else(|| name.to_string())
}

pub(super) fn compact_hidden_line_count(output: &serde_json::Value, complete_capture: Option<&str>) -> usize {
    if let Some(capture) = complete_capture {
        return normalize_terminal_output_lines(capture).len();
    }

    canonical_pipe_streams(output)
        .into_iter()
        .map(|stream| {
            if stream.label == Some("stderr") {
                return 0;
            }

            let line_count = normalize_terminal_output_lines(stream.text).len();
            if stream.label.is_none()
                && let Some(stderr) = output_text(output, "stderr")
                && streams_are_aliases(stream.text, stderr)
            {
                return line_count.saturating_sub(normalize_terminal_output_lines(stderr).len());
            }
            line_count
        })
        .sum()
}

pub(super) fn render_command_summary(
    renderer: &mut AnsiRenderer,
    name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
    command_success: bool,
    workspace_root: Option<&Path>,
    viewer_id: Option<ToolOutputId>,
    force_expanded: bool,
) -> Result<()> {
    if let Some(viewer_id) = viewer_id {
        // Carry the identity on the summary command itself. Text/order
        // matching is ambiguous when async calls run the same command.
        renderer.set_next_tool_output_anchor(viewer_id);
    }
    let stream_label = crate::agent::runloop::unified::tool_summary::stream_label_from_output(output, command_success);
    let summary_ctx = crate::agent::runloop::unified::tool_summary::ToolSummaryRenderContext { workspace_root };
    let status = ToolDisplayStatus::from_command_output(output, command_success);
    let bullet_color = status.color(ColorPalette::default());
    if force_expanded {
        crate::agent::runloop::unified::tool_summary::render_expanded_tool_call_summary(
            renderer,
            name,
            args_val,
            stream_label,
            &summary_ctx,
            bullet_color,
        )
    } else {
        crate::agent::runloop::unified::tool_summary::render_tool_call_summary(
            renderer,
            name,
            args_val,
            stream_label,
            &summary_ctx,
            bullet_color,
        )
    }
}

pub(super) fn value_has_content(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::String(value) => !value.trim().is_empty(),
        serde_json::Value::Array(values) => !values.is_empty(),
        serde_json::Value::Object(values) => !values.is_empty(),
        serde_json::Value::Number(_) => true,
    }
}

pub(super) const STRUCTURED_COMMAND_OUTPUT_FIELDS: &[&str] = &[
    "output",
    "stdout",
    "stderr",
    "content",
    "command",
    "critical_note",
    "next_action",
    "exit_code",
];

pub(super) const COMPACT_COMMAND_ARTIFACT_FIELDS: &[&str] = &[
    "generated_files",
    "json_result",
    "modified_files",
    "diff",
    "diff_preview",
    "failure_diagnostics",
    "security_notice",
    "artifacts",
];

pub(super) fn structured_command_context(output: &serde_json::Value) -> Option<String> {
    let object = output.as_object()?;
    let metadata = object
        .iter()
        .filter(|(key, value)| {
            !STRUCTURED_COMMAND_OUTPUT_FIELDS.contains(&key.as_str()) && !matches!(value, serde_json::Value::Null)
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<serde_json::Map<_, _>>();

    if metadata.is_empty() {
        return None;
    }

    serde_json::to_string_pretty(&serde_json::Value::Object(metadata)).ok()
}

pub(super) fn append_structured_command_context(lines: &mut Vec<String>, output: &serde_json::Value) {
    let Some(context) = structured_command_context(output) else {
        return;
    };

    lines.push("  structured output:".to_string());
    lines.extend(context.lines().map(|line| format!("    {line}")));
}

pub(super) fn render_structured_command_context(renderer: &mut AnsiRenderer, output: &serde_json::Value) -> Result<()> {
    let Some(context) = structured_command_context(output) else {
        return Ok(());
    };

    renderer.line(MessageStyle::ToolDetail, "structured output:")?;
    for line in context.lines() {
        renderer.line(MessageStyle::ToolDetail, &format!("  {line}"))?;
    }
    Ok(())
}

pub(super) fn complete_capture_unavailable(output: &serde_json::Value, complete_capture: Option<&str>) -> bool {
    output.get("spool_path").is_some() && complete_capture.is_none()
}

pub(super) fn has_compact_command_artifact(output: &serde_json::Value, complete_capture: Option<&str>) -> bool {
    output_text(output, "critical_note").is_some()
        || stderr_for_inline_display(output).is_some()
        || complete_capture_unavailable(output, complete_capture)
        || COMPACT_COMMAND_ARTIFACT_FIELDS
            .iter()
            .any(|key| output.get(*key).is_some_and(value_has_content))
        || [
            "security_notice",
            "next_action",
            "next_continue_args",
            "next_read_args",
            "fallback_tool",
            "fallback_tool_args",
        ]
        .iter()
        .any(|key| output.get(*key).is_some_and(value_has_content))
        || output.get("loop_detected").and_then(serde_json::Value::as_bool) == Some(true)
}

pub(super) fn has_file_operation_diff(output: &serde_json::Value) -> bool {
    !vtcode_core::tools::file_ops::canonical_diff_previews(output).is_empty()
}

pub(super) fn warning_message(output: &serde_json::Value) -> Option<String> {
    let warning = output.get("warning")?;
    match warning {
        serde_json::Value::String(message) => {
            let message = message.trim();
            (!message.is_empty()).then(|| message.to_string())
        }
        serde_json::Value::Number(number) if number.as_f64().is_some_and(|value| value != 0.0) => {
            Some(format!("warning count: {number}"))
        }
        serde_json::Value::Bool(true) => Some("completed with warnings".to_string()),
        serde_json::Value::Array(values) if !values.is_empty() => Some("completed with warnings".to_string()),
        serde_json::Value::Object(values) if !values.is_empty() => {
            let message = warning
                .as_object()
                .and_then(|fields| fields.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty());
            Some(message.unwrap_or("completed with warnings").to_string())
        }
        _ => None,
    }
}

pub(super) fn append_warning_line(lines: &mut Vec<String>, output: &serde_json::Value) {
    if let Some(message) = warning_message(output) {
        lines.push(format!("    ⚠ {message}"));
    }
}

pub(super) fn append_capture_status_line(
    lines: &mut Vec<String>,
    output: &serde_json::Value,
    complete_capture: Option<&str>,
) {
    if complete_capture_unavailable(output, complete_capture) {
        lines.push("    Complete command output capture unavailable.".to_string());
    }
}

/// Record the tool-call summary line ("• Ran ...") to the transcript only.
pub(super) fn record_summary_line(
    name: &str,
    args: &serde_json::Value,
    _output: &serde_json::Value,
    _command_success: bool,
) {
    let action_label = if tool_intent::is_command_run_tool_call(name, args) {
        "Run command"
    } else {
        name
    };
    let headline = if action_label == "Run command" {
        if let Some(cmd) = extract_command_line(args) {
            format!("Ran {cmd}")
        } else {
            "Ran command".to_string()
        }
    } else {
        format!("• {action_label}")
    };
    transcript::append(&headline);
}
