mod commands;
mod commands_processing;
mod files;
pub(crate) mod large_output;
#[cfg(test)]
mod large_output_tests;
mod mcp;
mod panels;
mod streams;
mod styles;

// Re-export large output handling utilities for external use
#[allow(unused_imports)]
pub(crate) use large_output::{
    LargeOutputConfig, SpoolResult, cleanup_old_spool_dirs, format_compact_notification,
    format_spool_notification, spool_large_output,
};
// Re-export stream utilities
#[allow(unused_imports)]
pub(crate) use streams::{
    render_code_fence_blocks, resolve_stdout_tail_limit, spool_output_with_notification,
};

use anyhow::Result;
use serde_json::Value;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::mcp::McpRendererProfile;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use commands::render_terminal_command_panel;
use files::{
    format_diff_content_lines_with_numbers, render_list_dir_output, render_read_file_output,
    render_write_file_preview,
};
use mcp::{
    render_context7_output, render_generic_output, render_sequential_output,
    resolve_renderer_profile,
};
use streams::render_stream_section;
use styles::{GitStyles, LsStyles};

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
        Some(tools::UNIFIED_FILE) => {
            if val.get("diff_preview").is_some() {
                let git_styles = GitStyles::new();
                let ls_styles = LsStyles::from_env();
                return render_write_file_preview(renderer, val, &git_styles, &ls_styles);
            }
            if val.get("content").is_some() {
                return render_read_file_output(renderer, val);
            }
        }
        Some(tools::RUN_PTY_CMD)
        | Some(tools::READ_PTY_SESSION)
        | Some(tools::CREATE_PTY_SESSION)
        | Some(tools::SEND_PTY_INPUT)
        | Some(tools::CLOSE_PTY_SESSION)
        | Some(tools::RESIZE_PTY_SESSION)
        | Some(tools::LIST_PTY_SESSIONS) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_terminal_command_panel(
                renderer,
                val,
                &git_styles,
                &ls_styles,
                vt_config,
                allow_tool_ansi,
            )
            .await;
        }
        Some(tools::UNIFIED_EXEC)
            if !is_git_diff_output && should_render_unified_exec_terminal_panel(val) =>
        {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_terminal_command_panel(
                renderer,
                val,
                &git_styles,
                &ls_styles,
                vt_config,
                allow_tool_ansi,
            )
            .await;
        }
        Some(tools::WEB_FETCH) => {
            return render_generic_output(renderer, val);
        }
        Some(tools::LIST_FILES) => {
            let ls_styles = LsStyles::from_env();
            return render_list_dir_output(renderer, val, &ls_styles);
        }
        Some(tools::READ_FILE) => {
            return render_read_file_output(renderer, val);
        }
        Some(tools::EXECUTE_CODE) => {
            let git_styles = GitStyles::new();
            let ls_styles = LsStyles::from_env();
            return render_terminal_command_panel(
                renderer,
                val,
                &git_styles,
                &ls_styles,
                vt_config,
                allow_tool_ansi,
            )
            .await;
        }
        Some(tools::TASK_TRACKER) | Some(tools::PLAN_TASK_TRACKER) => {
            if render_tracker_view(renderer, val)? {
                return Ok(());
            }
        }
        _ => {}
    }

    render_simple_tool_status(renderer, tool_name, val)?;

    if let Some(notice) = val.get("security_notice").and_then(Value::as_str) {
        renderer.line(MessageStyle::ToolDetail, notice)?;
    }

    // Render follow-up prompt if present (with double-rendering protection)
    if let Some(follow_up_prompt) = val.get("follow_up_prompt").and_then(Value::as_str) {
        // Check if prompt already appears in output to avoid double-rendering
        let already_rendered = val
            .get("output")
            .and_then(|v| v.as_str())
            .map(|output| output.contains(follow_up_prompt))
            .unwrap_or(false);

        if !already_rendered {
            renderer.line(MessageStyle::ToolDetail, "")?; // Add spacing
            renderer.line(MessageStyle::ToolDetail, follow_up_prompt)?;
        }
    }

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

    let output_mode = vt_config
        .map(|cfg| cfg.ui.tool_output_mode)
        .unwrap_or(ToolOutputMode::Compact);
    let tail_limit = resolve_stdout_tail_limit(vt_config);
    let git_styles = GitStyles::new();
    let ls_styles = LsStyles::from_env();
    let disable_spool = val
        .get("no_spool")
        .and_then(Value::as_bool)
        .unwrap_or(false);

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

fn is_git_diff_payload(val: &Value) -> bool {
    val.get("content_type")
        .and_then(Value::as_str)
        .is_some_and(|content_type| content_type == "git_diff")
}

fn render_tracker_view(renderer: &mut AnsiRenderer, val: &Value) -> Result<bool> {
    let view = val.get("view").and_then(Value::as_object);
    let summary_lines = tracker_summary_lines(val);

    let has_view_lines = view
        .and_then(|obj| obj.get("lines"))
        .and_then(Value::as_array)
        .is_some_and(|lines| !lines.is_empty());
    if !has_view_lines && summary_lines.is_empty() {
        return Ok(false);
    }

    let title = view
        .and_then(|obj| obj.get("title"))
        .and_then(Value::as_str)
        .or_else(|| {
            val.get("checklist")
                .and_then(|c| c.get("title"))
                .and_then(Value::as_str)
        })
        .unwrap_or("Task tracker");

    renderer.line(MessageStyle::ToolDetail, &format!("• {}", title))?;
    for line in summary_lines {
        renderer.line(MessageStyle::ToolDetail, &line)?;
    }
    if let Some(lines) = view
        .and_then(|obj| obj.get("lines"))
        .and_then(Value::as_array)
    {
        for line in lines {
            if let Some(display) = line.get("display").and_then(Value::as_str) {
                renderer.line(MessageStyle::ToolDetail, display)?;
            } else if let Some(text) = line.as_str() {
                renderer.line(MessageStyle::ToolDetail, text)?;
            }
        }
    }

    Ok(true)
}

fn tracker_summary_lines(val: &Value) -> Vec<String> {
    let mut lines = Vec::new();

    if let Some(status) = val.get("status").and_then(Value::as_str)
        && !status.trim().is_empty()
    {
        lines.push(format!("  Tracker status: {}", status));
    }

    let Some(checklist) = val.get("checklist").and_then(Value::as_object) else {
        if let Some(message) = val.get("message").and_then(Value::as_str)
            && !message.trim().is_empty()
        {
            lines.push(format!("  Update: {}", message));
        }
        return lines;
    };

    let total = checklist.get("total").and_then(Value::as_u64).unwrap_or(0);
    let completed = checklist
        .get("completed")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let in_progress = checklist
        .get("in_progress")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let pending = checklist
        .get("pending")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let blocked = checklist
        .get("blocked")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    if total > 0 {
        let progress_percent = checklist
            .get("progress_percent")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| (completed * 100) / total.max(1));
        lines.push(format!(
            "  Progress: {}/{} complete ({}%)",
            completed, total, progress_percent
        ));
        lines.push(format!(
            "  Breakdown: {} in progress, {} pending, {} blocked",
            in_progress, pending, blocked
        ));
    }

    if let Some(items) = checklist.get("items").and_then(Value::as_array) {
        let active_items = items
            .iter()
            .filter(|item| {
                item.get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status == "in_progress")
            })
            .map(|item| {
                let index = item.get("index").and_then(Value::as_u64).unwrap_or(0);
                let description = item
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("Unnamed task");
                if index > 0 {
                    format!("#{} {}", index, description)
                } else {
                    description.to_string()
                }
            })
            .collect::<Vec<_>>();

        if !active_items.is_empty() {
            lines.push("  Active items:".to_string());
            for item in active_items.iter().take(3) {
                lines.push(format!("    - {}", item));
            }
            if active_items.len() > 3 {
                lines.push(format!("    - ... and {} more", active_items.len() - 3));
            }
        }
    }

    if let Some(message) = val.get("message").and_then(Value::as_str)
        && !message.trim().is_empty()
    {
        lines.push(format!("  Update: {}", message));
    }

    lines
}

fn render_simple_tool_status(
    renderer: &mut AnsiRenderer,
    _tool_name: Option<&str>,
    val: &Value,
) -> Result<()> {
    let has_error = val.get("error").is_some() || val.get("error_type").is_some();

    if has_error {
        render_error_details(renderer, val)?;
    }

    Ok(())
}

fn should_render_unified_exec_terminal_panel(val: &Value) -> bool {
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
    if let Some(error_msg) = val.get("message").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::ToolError, &format!("Error: {}", error_msg))?;
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
        renderer.line(
            MessageStyle::ToolDetail,
            &format!("Type: {}", type_description),
        )?;
    }

    if let Some(original) = val.get("original_error").and_then(|v| v.as_str())
        && !original.trim().is_empty()
    {
        let display_error = if original.len() > 200 {
            format!("{}...", &original[..197])
        } else {
            original.to_string()
        };
        renderer.line(
            MessageStyle::ToolDetail,
            &format!("Details: {}", display_error),
        )?;
    }

    if let Some(path) = val.get("path").and_then(|v| v.as_str()) {
        renderer.line(MessageStyle::ToolDetail, &format!("Path: {}", path))?;
    }

    if let Some(line) = val.get("line").and_then(|v| v.as_u64()) {
        if let Some(col) = val.get("column").and_then(|v| v.as_u64()) {
            renderer.line(
                MessageStyle::ToolDetail,
                &format!("Location: line {}, column {}", line, col),
            )?;
        } else {
            renderer.line(
                MessageStyle::ToolDetail,
                &format!("Location: line {}", line),
            )?;
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
            renderer.line(
                MessageStyle::ToolDetail,
                &format!("    ... and {} more", suggestions.len() - 5),
            )?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::sync::mpsc::UnboundedReceiver;
    use vtcode_core::ui::{InlineCommand, InlineHandle};
    use vtcode_core::utils::ansi::AnsiRenderer;

    use super::{
        render_tool_output, should_render_unified_exec_terminal_panel, tracker_summary_lines,
    };

    fn collect_inline_output(receiver: &mut UnboundedReceiver<InlineCommand>) -> String {
        let mut lines: Vec<String> = Vec::new();
        while let Ok(command) = receiver.try_recv() {
            match command {
                InlineCommand::AppendLine { segments, .. } => {
                    lines.push(
                        segments
                            .into_iter()
                            .map(|segment| segment.text)
                            .collect::<String>(),
                    );
                }
                InlineCommand::ReplaceLast {
                    lines: replacement_lines,
                    ..
                } => {
                    for line in replacement_lines {
                        lines.push(
                            line.into_iter()
                                .map(|segment| segment.text)
                                .collect::<String>(),
                        );
                    }
                }
                _ => {}
            }
        }
        lines.join("\n")
    }

    #[test]
    fn unified_exec_terminal_panel_detects_command_payload() {
        let payload = json!({
            "command": "cargo check",
            "output": "Checking vtcode"
        });
        assert!(should_render_unified_exec_terminal_panel(&payload));
    }

    #[test]
    fn unified_exec_terminal_panel_detects_session_payload() {
        let payload = json!({
            "session_id": "run-123",
            "is_exited": true
        });
        assert!(should_render_unified_exec_terminal_panel(&payload));
    }

    #[test]
    fn unified_exec_terminal_panel_ignores_non_terminal_payload() {
        let payload = json!({
            "sessions": [],
            "success": true
        });
        assert!(!should_render_unified_exec_terminal_panel(&payload));
    }

    #[test]
    fn unified_exec_terminal_panel_skips_git_diff_payload() {
        let payload = json!({
            "command": "git diff -- src/main.rs",
            "output": "diff --git a/src/main.rs b/src/main.rs",
            "content_type": "git_diff"
        });
        assert!(!should_render_unified_exec_terminal_panel(&payload));
    }

    #[tokio::test]
    async fn render_tool_output_unified_exec_git_diff_renders_diff_not_command_preview() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer =
            AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let payload = json!({
            "command": "git diff -- src/main.rs",
            "output": "diff --git a/src/main.rs b/src/main.rs\n+added\n-removed\n",
            "content_type": "git_diff",
            "is_exited": true,
            "exit_code": 0
        });

        render_tool_output(
            &mut renderer,
            Some(vtcode_core::config::constants::tools::UNIFIED_EXEC),
            &payload,
            None,
        )
        .await
        .expect("git diff payload should render");

        let inline_output = collect_inline_output(&mut receiver);
        assert!(inline_output.contains("diff --git a/src/main.rs b/src/main.rs"));
        assert!(
            !inline_output.contains("└ "),
            "run-command preview prefix should not appear for git diff payload"
        );
    }

    #[tokio::test]
    async fn render_tool_output_unified_exec_git_diff_stdout_renders_diff_not_command_preview() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer =
            AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let payload = json!({
            "command": "git diff -- src/lib.rs",
            "stdout": "diff --git a/src/lib.rs b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n",
            "content_type": "git_diff",
            "is_exited": true,
            "exit_code": 0
        });

        render_tool_output(
            &mut renderer,
            Some(vtcode_core::config::constants::tools::UNIFIED_EXEC),
            &payload,
            None,
        )
        .await
        .expect("git diff stdout payload should render");

        let inline_output = collect_inline_output(&mut receiver);
        assert!(inline_output.contains("diff --git a/src/lib.rs b/src/lib.rs"));
        assert!(inline_output.contains("@@ -1 +1 @@"));
        assert!(inline_output.contains("new"));
        assert!(
            !inline_output.contains("└ "),
            "run-command preview prefix should not appear for git diff payload"
        );
    }

    #[test]
    fn tracker_summary_lines_include_progress_and_active_items() {
        let payload = json!({
            "status": "updated",
            "message": "Item 2 status changed: pending -> in_progress",
            "checklist": {
                "total": 4,
                "completed": 1,
                "in_progress": 2,
                "pending": 1,
                "blocked": 0,
                "progress_percent": 25,
                "items": [
                    { "index": 1, "description": "A", "status": "completed" },
                    { "index": 2, "description": "B", "status": "in_progress" },
                    { "index": 3, "description": "C", "status": "in_progress" },
                    { "index": 4, "description": "D", "status": "pending" }
                ]
            }
        });

        let lines = tracker_summary_lines(&payload);
        assert!(lines.iter().any(|line| line == "  Tracker status: updated"));
        assert!(
            lines
                .iter()
                .any(|line| line == "  Progress: 1/4 complete (25%)")
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "  Breakdown: 2 in progress, 1 pending, 0 blocked")
        );
        assert!(lines.iter().any(|line| line == "    - #2 B"));
        assert!(lines.iter().any(|line| line == "    - #3 C"));
    }

    #[test]
    fn tracker_summary_lines_still_show_message_without_checklist() {
        let payload = json!({
            "status": "empty",
            "message": "No active checklist."
        });
        let lines = tracker_summary_lines(&payload);
        assert!(lines.iter().any(|line| line == "  Tracker status: empty"));
        assert!(
            lines
                .iter()
                .any(|line| line == "  Update: No active checklist.")
        );
    }
}
