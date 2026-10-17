//! `render_tool_output_common`: the central inline-render dispatcher.

use super::*;

pub(super) async fn render_tool_output_common(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
    command_success: bool,
    vt_config: Option<&VTCodeConfig>,
    workspace_root: Option<&Path>,
) -> Result<()> {
    let inline_run_tool = renderer.supports_inline_ui() && streams_pty_output(name, args_val);
    let git_diff_payload = is_git_diff_payload(output);
    let status = ToolDisplayStatus::from_command_output(output, command_success);
    let has_spool_path = output.get("spool_path").is_some();
    let complete_capture = if renderer.supports_inline_ui()
        && is_command_output_call(name, args_val)
        && (inline_run_tool || has_spool_path)
    {
        load_complete_output(output, workspace_root).await
    } else {
        None
    };

    // For streamed inline PTY tools, retain the complete capture separately.
    // Expanded mode renders a bounded live block; compact mode waits for the
    // completion row so the transcript does not jump through transient PTY
    // replacements.
    let inline_pty_command = inline_run_tool && is_command_output_call(name, args_val);
    let compact_pty_without_live_preview = inline_pty_command && renderer.is_compact_display();
    if inline_pty_command && !git_diff_payload {
        // Prefer the complete PTY spool (or the complete inline result) for
        // the session-local tool-output viewer. The live PTY block, when
        // enabled, remains bounded separately.
        let viewer_id = if let Some(capture) = complete_capture.as_deref() {
            let mut viewer_lines =
                build_merged_command_output_lines(name, args_val, capture, workspace_root, output, status);
            append_capture_status_line(&mut viewer_lines, output, complete_capture.as_deref());
            append_follow_up_capture_lines(&mut viewer_lines, output, Some(capture));
            Some(handle.record_tool_output(viewer_lines))
        } else {
            // A rejected or unavailable spool must not fall back to a
            // potentially untrusted path. Keep the command call visible in
            // the viewer while retaining fail-closed spool handling.
            let mut viewer_lines = if has_spool_path {
                build_merged_command_output_lines(name, args_val, "", workspace_root, output, status)
            } else {
                build_pipe_command_output_lines(name, args_val, output, workspace_root, status)
            };
            append_capture_status_line(&mut viewer_lines, output, complete_capture.as_deref());
            append_follow_up_capture_lines(&mut viewer_lines, output, complete_capture.as_deref());
            Some(handle.record_tool_output(viewer_lines))
        };

        let compact_success = renderer.is_compact_display()
            && is_command_output_call(name, args_val)
            && matches!(status, ToolDisplayStatus::Success)
            && !has_compact_command_artifact(output, complete_capture.as_deref());
        if compact_success {
            renderer.collapse_pty_block_to_compact_activity(
                compact_command_text(name, args_val, workspace_root),
                compact_hidden_line_count(output, complete_capture.as_deref()),
                None,
                viewer_id,
            )?;
            return Ok(());
        }

        // A completed PTY result with a warning, diagnostic, diff, or failed
        // capture is a hard boundary regardless of whether a live preview was
        // shown. The next successful command must start a new group.
        renderer.flush_compact_command_group();

        if complete_capture_unavailable(output, complete_capture.as_deref()) {
            renderer.line(MessageStyle::Warning, "Complete command output capture unavailable.")?;
        }
        if let Some(message) = warning_message(output) {
            renderer.line(MessageStyle::Warning, &format!("⚠ {message}"))?;
        }

        // Expanded mode retains the existing live PTY block and makes the
        // post-execution summary available to the normal transcript path.
        // Compact mode has no live block, so render an anchored summary now
        // to keep attention-worthy results identifiable in the inline UI.
        if compact_pty_without_live_preview {
            render_command_summary(renderer, name, args_val, output, command_success, workspace_root, viewer_id, true)?;
        } else {
            record_summary_line(name, args_val, output, command_success);
        }

        if let Some(note) = output_text(output, "critical_note") {
            renderer.line(MessageStyle::ToolError, note)?;
            transcript::append(note);
        }

        // A distinct stderr field is not part of the live PTY block. Keep it
        // visible after completion, while alias detection avoids repeating a
        // stderr stream already included in the terminal capture.
        if let Some(stderr) = stderr_for_inline_display(output) {
            let stderr_lines = normalize_terminal_output_lines(stderr);
            if !stderr_lines.is_empty() {
                renderer.line(MessageStyle::ToolError, &format!("stderr: {}", stderr_lines.join("\n")))?;
            }
        }

        if !has_renderable_stream_content(output) && matches!(status, ToolDisplayStatus::Success) {
            if !renderer.is_compact_display() {
                renderer.line(MessageStyle::Info, "(no output)")?;
            }
            return Ok(());
        }

        // Send completion as a status line only when the command needs
        // attention; on success the colored header bullet is sufficient.
        if !matches!(status, ToolDisplayStatus::Success) {
            if let Some(completion) = compact_run_completion_line(output, status) {
                let indented = format!("    {}", completion);
                renderer.line(MessageStyle::Status, &indented)?;
                transcript::append(&completion);
            }
        }
        return Ok(());
    }

    // Session follow-ups need a viewer capture too: the inline body is capped
    // at 10 rows and the expand notice opens this record, so the complete
    // stdin/stdout capture stays reachable without leaving the TUI.
    let is_session_followup = is_exec_session_call(name, args_val);
    // Drop any unused expand anchor from a prior call that did not overflow.
    let _ = renderer.take_session_expand_anchor();
    renderer.set_session_body(is_session_followup);
    let viewer_id = if renderer.supports_inline_ui() && (is_command_output_call(name, args_val) || is_session_followup)
    {
        let mut viewer_lines = if is_session_followup {
            build_pipe_command_output_lines(name, args_val, output, workspace_root, status)
        } else if inline_run_tool || has_spool_path {
            complete_capture.as_deref().map_or_else(
                || build_merged_command_output_lines(name, args_val, "", workspace_root, output, status),
                |capture| build_merged_command_output_lines(name, args_val, capture, workspace_root, output, status),
            )
        } else {
            build_pipe_command_output_lines(name, args_val, output, workspace_root, status)
        };
        append_capture_status_line(&mut viewer_lines, output, complete_capture.as_deref());
        append_follow_up_capture_lines(&mut viewer_lines, output, complete_capture.as_deref());
        let viewer_id = handle.record_tool_output(viewer_lines);
        // Session expand notices consume this when the 10-row body overflows.
        if is_session_followup {
            renderer.set_session_expand_anchor(viewer_id);
        }
        Some(viewer_id)
    } else {
        None
    };

    let compact_command = renderer.supports_inline_ui()
        && is_command_output_call(name, args_val)
        && renderer.is_compact_display()
        && matches!(status, ToolDisplayStatus::Success)
        && !git_diff_payload;
    let compact_file_diff = renderer.supports_inline_ui()
        && renderer.is_compact_display()
        && matches!(status, ToolDisplayStatus::Success)
        && crate::agent::runloop::unified::tool_summary::is_file_modification_tool(name, args_val)
        && has_file_operation_diff(output);
    let compact_artifact = has_compact_command_artifact(output, complete_capture.as_deref());
    if !matches!(status, ToolDisplayStatus::Success) {
        // Warnings and failures are hard boundaries even for command aliases
        // that do not use the live PTY path (for example, `bash`).
        renderer.flush_compact_command_group();
    }
    if git_diff_payload || compact_command && compact_artifact {
        // Attention-worthy output is a hard boundary: do not let a command
        // with visible diagnostics or a diff merge into the preceding group.
        renderer.flush_compact_command_group();
    }
    if compact_file_diff {
        // File changes are glanceable activity, not command-group members.
        // Flush before rendering the file heading so a preceding command row
        // cannot absorb it and the following command starts a fresh group.
        renderer.flush_compact_command_group();
    }
    if compact_command {
        renderer.render_compact_command_activity(
            compact_command_text(name, args_val, workspace_root),
            compact_hidden_line_count(output, complete_capture.as_deref()),
            None,
            viewer_id,
        )?;
        if !compact_artifact {
            return Ok(());
        }
    }

    // Streamed PTY tools with a diff retain the existing live summary in
    // expanded mode. Compact mode suppresses that live row, so render an
    // anchored summary before the diff body instead.
    let skip_live_pty_summary = inline_run_tool && git_diff_payload && !compact_pty_without_live_preview;
    if !(compact_command || skip_live_pty_summary || compact_file_diff) {
        render_command_summary(
            renderer,
            name,
            args_val,
            output,
            command_success,
            workspace_root,
            viewer_id,
            !matches!(status, ToolDisplayStatus::Success) || git_diff_payload,
        )?;
    }

    if complete_capture_unavailable(output, complete_capture.as_deref()) {
        renderer.line(MessageStyle::Warning, "Complete command output capture unavailable.")?;
    }
    if let Some(message) = warning_message(output) {
        renderer.line(MessageStyle::Warning, &format!("⚠ {message}"))?;
    }

    let result = crate::agent::runloop::tool_output::render_tool_output(renderer, Some(name), output, vt_config).await;
    if result.is_ok() && compact_command && compact_artifact {
        render_structured_command_context(renderer, output)?;
    }
    if !matches!(status, ToolDisplayStatus::Success) {
        // The warning/failure row itself is visible, but it must not remain
        // the active tail that a later successful command could extend.
        renderer.flush_compact_command_group();
    }
    if compact_command && compact_artifact {
        // Some attention-worthy metadata (for example, a critical note) can
        // be rendered without emitting another line. End the active compact
        // tail explicitly so the next command cannot merge into this row.
        renderer.flush_compact_command_group();
    }
    result
}
