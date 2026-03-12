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

// Re-export stream utilities
pub(crate) use streams::{render_code_fence_blocks, resolve_stdout_tail_limit};

use anyhow::Result;
use serde_json::Value;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::mcp::McpRendererProfile;
use vtcode_core::tools::continuation::{
    NEXT_CONTINUE_PROMPT, NEXT_READ_PROMPT, PtyContinuationArgs, ReadChunkContinuationArgs,
};
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

fn tool_recovery_hint(val: &Value) -> Option<&'static str> {
    if !val
        .get("loop_detected")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
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
        push_tool_follow_up_hint(
            &mut hints,
            format!(
                "Large output was spooled to \"{}\". Use read_file/grep_file to inspect details.",
                path
            ),
        );
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
    render_terminal_command_panel(
        renderer,
        val,
        &git_styles,
        &ls_styles,
        vt_config,
        allow_tool_ansi,
    )
    .await
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
        Some(tools::UNIFIED_FILE) => {
            if val.get("diff_preview").is_some() {
                let git_styles = GitStyles::new();
                let ls_styles = LsStyles::from_env();
                return render_write_file_preview(renderer, val, &git_styles, &ls_styles);
            }
            if val.get("content").is_some() {
                render_read_file_output(renderer, val)?;
                render_tool_follow_up_hints(
                    renderer,
                    val,
                    val.get("content").and_then(Value::as_str),
                )?;
                return Ok(());
            }
        }
        Some(tools::RUN_PTY_CMD)
        | Some(tools::READ_PTY_SESSION)
        | Some(tools::CREATE_PTY_SESSION)
        | Some(tools::SEND_PTY_INPUT)
        | Some(tools::CLOSE_PTY_SESSION)
        | Some(tools::RESIZE_PTY_SESSION)
        | Some(tools::LIST_PTY_SESSIONS) => {
            return render_terminal_tool_output(renderer, val, vt_config, allow_tool_ansi).await;
        }
        Some(tools::UNIFIED_EXEC)
            if !is_git_diff_output && should_render_unified_exec_terminal_panel(val) =>
        {
            return render_terminal_tool_output(renderer, val, vt_config, allow_tool_ansi).await;
        }
        Some("web_fetch") => {
            render_generic_output(renderer, val)?;
            render_tool_follow_up_hints_for_value(renderer, val)?;
            return Ok(());
        }
        Some("list_files") => {
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

pub(crate) fn tracker_view_lines(val: &Value) -> Vec<String> {
    let view = val.get("view").and_then(Value::as_object);
    let summary_lines = tracker_summary_lines(val);

    let has_view_lines = view
        .and_then(|obj| obj.get("lines"))
        .and_then(Value::as_array)
        .is_some_and(|lines| !lines.is_empty());
    if !has_view_lines && summary_lines.is_empty() {
        return Vec::new();
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

    let mut lines = Vec::new();
    lines.push(format!("• {}", title));
    lines.extend(summary_lines);
    if let Some(view_lines) = view
        .and_then(|obj| obj.get("lines"))
        .and_then(Value::as_array)
    {
        for line in view_lines {
            if let Some(display) = line.get("display").and_then(Value::as_str) {
                lines.push(display.to_string());
            } else if let Some(text) = line.as_str() {
                lines.push(text.to_string());
            }
        }
    }

    lines
}

fn render_tracker_view(renderer: &mut AnsiRenderer, val: &Value) -> Result<bool> {
    let lines = tracker_view_lines(val);
    if lines.is_empty() {
        return Ok(false);
    }

    for line in lines {
        renderer.line(MessageStyle::ToolDetail, &line)?;
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
        preferred_follow_up_rendered_body, render_tool_output,
        should_render_unified_exec_terminal_panel, tracker_summary_lines,
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

    #[test]
    fn preferred_follow_up_rendered_body_prefers_output_over_content() {
        let payload = json!({
            "output": "stdout body",
            "content": "content body"
        });

        assert_eq!(
            preferred_follow_up_rendered_body(&payload),
            Some("stdout body")
        );
    }

    #[test]
    fn preferred_follow_up_rendered_body_falls_back_to_content() {
        let payload = json!({
            "content": "content body"
        });

        assert_eq!(
            preferred_follow_up_rendered_body(&payload),
            Some("content body")
        );
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

    #[tokio::test]
    async fn render_tool_output_unified_exec_renders_structured_hints() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer =
            AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let payload = json!({
            "command": "cargo check",
            "output": "tail preview",
            "session_id": "run-123",
            "is_exited": false,
            "next_continue_args": {
                "session_id": "run-123"
            },
            "spool_path": ".vtcode/context/tool_outputs/run-123.txt"
        });

        render_tool_output(
            &mut renderer,
            Some(vtcode_core::config::constants::tools::UNIFIED_EXEC),
            &payload,
            None,
        )
        .await
        .expect("structured hint payload should render");

        let inline_output = collect_inline_output(&mut receiver);
        assert!(inline_output.contains("Large output was spooled to"));
        assert!(inline_output.contains("Use `next_continue_args`."));
    }

    #[tokio::test]
    async fn render_tool_output_read_file_renders_spool_hint_on_early_return_path() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer =
            AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let payload = json!({
            "path": "README.md",
            "content": "preview",
            "spool_path": ".vtcode/context/tool_outputs/readme.txt"
        });

        render_tool_output(
            &mut renderer,
            Some(vtcode_core::config::constants::tools::READ_FILE),
            &payload,
            None,
        )
        .await
        .expect("read_file payload should render");

        let inline_output = collect_inline_output(&mut receiver);
        assert!(inline_output.contains("Large output was spooled to"));
    }

    #[tokio::test]
    async fn render_tool_output_web_fetch_content_fallback_renders_follow_up_hint() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer =
            AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let payload = json!({
            "content": "preview",
            "spool_path": ".vtcode/context/tool_outputs/web.txt"
        });

        render_tool_output(&mut renderer, Some("web_fetch"), &payload, None)
            .await
            .expect("web_fetch payload should render");

        let inline_output = collect_inline_output(&mut receiver);
        assert!(inline_output.contains("Large output was spooled to"));
    }

    #[tokio::test]
    async fn render_tool_output_read_file_long_preview_keeps_preview_limits() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer =
            AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let content = (1..=100)
            .map(|idx| format!("{idx}: line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        let payload = json!({
            "path": "src/main.rs",
            "content": content
        });

        render_tool_output(
            &mut renderer,
            Some(vtcode_core::config::constants::tools::READ_FILE),
            &payload,
            None,
        )
        .await
        .expect("read_file preview payload should render");

        let inline_output = collect_inline_output(&mut receiver);
        assert!(inline_output.contains("line 1"));
        assert!(inline_output.contains("line 12"));
        assert!(inline_output.contains("88 more lines"));
    }

    #[tokio::test]
    async fn render_tool_output_renders_loop_recovery_hint_from_structured_fields() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer =
            AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let payload = json!({
            "loop_detected": true,
            "fallback_tool": vtcode_core::config::constants::tools::UNIFIED_SEARCH
        });

        render_tool_output(
            &mut renderer,
            Some(vtcode_core::config::constants::tools::UNIFIED_SEARCH),
            &payload,
            None,
        )
        .await
        .expect("loop recovery hint payload should render");

        let inline_output = collect_inline_output(&mut receiver);
        assert!(inline_output.contains("Loop detected; fallback is available."));
    }

    #[tokio::test]
    async fn render_tool_output_renders_spooled_loop_recovery_hint() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer =
            AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let payload = json!({
            "loop_detected": true,
            "spool_path": ".vtcode/context/tool_outputs/readme.txt",
            "next_read_args": {
                "path": ".vtcode/context/tool_outputs/readme.txt",
                "offset": 81,
                "limit": 40
            }
        });

        render_tool_output(
            &mut renderer,
            Some(vtcode_core::config::constants::tools::READ_FILE),
            &payload,
            None,
        )
        .await
        .expect("spooled loop recovery hint payload should render");

        let inline_output = collect_inline_output(&mut receiver);
        assert!(inline_output.contains("Loop detected; continue from spooled output."));
    }

    #[tokio::test]
    async fn render_tool_output_does_not_duplicate_loop_recovery_hint() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer =
            AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let payload = json!({
            "loop_detected": true,
            "fallback_tool": vtcode_core::config::constants::tools::UNIFIED_SEARCH,
            "output": "Loop detected; fallback is available."
        });

        render_tool_output(&mut renderer, Some("custom_tool"), &payload, None)
            .await
            .expect("duplicate hint payload should render");

        let inline_output = collect_inline_output(&mut receiver);
        assert_eq!(
            inline_output
                .matches("Loop detected; fallback is available.")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn render_tool_output_unified_exec_keeps_exit_127_output_and_guidance() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer =
            AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        let payload = json!({
            "command": "pip install pymupdf",
            "output": "bash: pip: command not found",
            "session_id": "run-127",
            "is_exited": true,
            "exit_code": 127,
            "critical_note": "Command `pip` was not found in PATH.",
            "next_action": "Check the command name or install the missing binary, then rerun the command."
        });

        render_tool_output(
            &mut renderer,
            Some(vtcode_core::config::constants::tools::UNIFIED_EXEC),
            &payload,
            None,
        )
        .await
        .expect("exit 127 payload should render");

        let inline_output = collect_inline_output(&mut receiver);
        assert!(inline_output.contains("bash: pip: command not found"));
        assert!(inline_output.contains("Command `pip` was not found in PATH."));
        assert!(inline_output.contains(
            "Check the command name or install the missing binary, then rerun the command."
        ));
        assert!(inline_output.contains("✓ exit 127"));
        assert!(!inline_output.contains("Solution:"));
        assert_eq!(
            inline_output
                .matches(
                    "Check the command name or install the missing binary, then rerun the command."
                )
                .count(),
            1
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
