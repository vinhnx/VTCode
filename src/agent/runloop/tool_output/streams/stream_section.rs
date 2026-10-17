//! Split from streams.rs; see module docs there.

use super::*;

pub(crate) fn is_exec_session_tool(tool_name: Option<&str>) -> bool {
    tool_name.is_some_and(crate::agent::runloop::unified::is_exec_session_tool_name)
}

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
#[cfg_attr(
    feature = "profiling",
    tracing::instrument(skip(renderer, content, git_styles, ls_styles, config), level = "debug")
)]
pub(crate) async fn render_stream_section(
    renderer: &mut AnsiRenderer,
    title: &str,
    content: &str,
    mode: ToolOutputMode,
    tail_limit: usize,
    tool_name: Option<&str>,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    fallback_style: MessageStyle,
    allow_ansi: bool,
    disable_spool: bool,
    config: Option<&VTCodeConfig>,
) -> Result<()> {
    use std::fmt::Write as FmtWrite;

    // `unified_exec` follow-ups share the tool name with launches; the caller
    // marks them so a session poll gets the dim/expand path instead of the
    // run-command head/tail preview.
    let force_session_body = renderer.session_body_active();
    let is_run_command = !force_session_body
        && tool_name.is_some_and(|name| {
            tool_intent::is_command_run_tool(name)
                || name == vtcode_core::config::constants::tools::UNIFIED_EXEC
                || name == vtcode_core::config::constants::tools::EXEC_PTY_CMD
        });
    // Session follow-ups re-render the same captured terminal text on every
    // poll, so their body is plain rather than re-colored: git-diff detection
    // and LS_COLORS styling both misfire on build logs (`PASS … .rs`), and the
    // run-command path renders the same content through a plain fence already.
    let is_exec_session = force_session_body || (!is_run_command && is_exec_session_tool(tool_name));
    let allow_ansi_for_tool = allow_ansi && !is_run_command;
    let apply_line_styles = !is_run_command && !is_exec_session;

    // Strip ANSI codes once and reuse for both diff detection and normalization.
    // This avoids scanning the same content twice when ANSI is not allowed.
    let stripped_for_diff = strip_ansi_codes(content);
    let is_diff_content = apply_line_styles && looks_like_diff_content(stripped_for_diff.as_ref());
    let normalized_content = if allow_ansi_for_tool {
        Cow::Borrowed(content)
    } else {
        // Reuse the already-stripped result instead of scanning again.
        // Note: stripped_for_diff is consumed here, but we've already computed
        // is_diff_content above, so we use normalized_content for diff rendering below.
        stripped_for_diff
    };

    if is_run_command {
        return render_run_command_preview(
            renderer,
            normalized_content.as_ref(),
            tool_name,
            fallback_style,
            disable_spool,
            config,
        )
        .await;
    }

    // Use normalized content directly (token budget logic removed).
    // No clone needed since we own the Cow and don't use it after this point.
    let was_truncated_by_tokens = false;

    if !disable_spool
        && let Some(tool) = tool_name
        && let Ok(Some(log_path)) = spool_output_if_needed(normalized_content.as_ref(), tool, config).await
    {
        // Skip preview entirely for extremely large output
        if normalized_content.len() > EXTREME_OUTPUT_THRESHOLD_MB {
            let mut msg_buffer = String::with_capacity(256);
            let _ = write!(
                &mut msg_buffer,
                "Output too large ({} bytes), spooled to: {}",
                normalized_content.len(),
                log_path.display()
            );
            renderer.line(MessageStyle::ToolDetail, &msg_buffer)?;
            renderer.line(MessageStyle::ToolDetail, "(Preview skipped due to size)")?;
            return Ok(());
        }

        // Use compact head+tail preview (like diff view) instead of dumping tail lines
        let head_lines = RUN_COMMAND_HEAD_PREVIEW_LINES;
        let tail_lines_count = RUN_COMMAND_TAIL_PREVIEW_LINES;
        let preview = excerpt_text_lines(normalized_content.as_ref(), head_lines, tail_lines_count);
        let total = preview.total;

        let mut msg_buffer = String::with_capacity(256);
        if !is_run_command {
            let uppercase_title = if title.is_empty() {
                Cow::Borrowed("OUTPUT")
            } else {
                Cow::Owned(title.to_ascii_uppercase())
            };
            let _ = write!(
                &mut msg_buffer,
                "[{}] {} bytes, {} lines — spooled to: {}",
                uppercase_title.as_ref(),
                normalized_content.len(),
                total,
                log_path.display()
            );
        } else {
            let _ = write!(
                &mut msg_buffer,
                "{} bytes, {} lines — spooled to: {}",
                normalized_content.len(),
                total,
                log_path.display()
            );
        }
        renderer.line(MessageStyle::ToolDetail, &msg_buffer)?;

        // Render head lines
        for line in preview.head.iter() {
            render_preview_line(renderer, line, None, Some("  "), true, fallback_style, None)?;
        }

        // Show omitted notice between head and tail
        if preview.hidden_count > 0 {
            renderer.line(
                MessageStyle::ToolDetail,
                &hidden_lines_notice(preview.hidden_count, HiddenLinesNoticeKind::CommandPreview),
            )?;
        }

        // Render tail lines
        for line in preview.tail.iter() {
            render_preview_line(renderer, line, None, Some("  "), true, fallback_style, None)?;
        }

        return Ok(());
    }

    if is_diff_content {
        // Use normalized_content for diff rendering - it's already stripped when ANSI is not allowed
        render_diff_content_block(
            renderer,
            normalized_content.as_ref(),
            tool_name,
            git_styles,
            ls_styles,
            fallback_style,
            mode,
            tail_limit,
        )?;
        return Ok(());
    }

    // Token budget logic removed - use normalized content directly
    let prefer_full = renderer.prefers_untruncated_output();
    let (mut lines_vec, total, mut truncated) =
        select_stream_lines_streaming(normalized_content.as_ref(), mode, tail_limit, prefer_full);
    if prefer_full && trim_to_tail(&mut lines_vec, INLINE_STREAM_MAX_LINES) {
        truncated = true;
    }
    // Session follow-ups stay below the shared display budget even when the
    // interactive TUI would otherwise accept an untruncated body.
    if is_exec_session && trim_to_tail(&mut lines_vec, EXEC_SESSION_OUTPUT_MAX_LINES) {
        truncated = true;
    }

    let truncated = truncated || was_truncated_by_tokens;

    if lines_vec.is_empty() {
        return Ok(());
    }

    // Exec-session stdin/stdout bodies are the lowest visual tier: theme
    // `pty_output` plus DIM so a re-rendered build log recedes under the
    // assistant's reply. Headers keep normal tool brightness.
    let session_body_style = is_exec_session.then(|| fallback_style.style().effects(Effects::DIMMED));

    if !is_exec_session && should_render_as_code_block(fallback_style) && !apply_line_styles {
        let markdown = build_markdown_code_block(&lines_vec, None, true);
        renderer.render_markdown_output(fallback_style, &markdown)?;
    } else {
        for line in &lines_vec {
            if apply_line_styles && let Some(style) = select_line_style(tool_name, line, git_styles, ls_styles) {
                render_preview_line(renderer, line, None, None, true, fallback_style, Some(style))?;
            } else {
                render_preview_line(renderer, line, None, None, true, fallback_style, session_body_style)?;
            }
        }
    }

    let hidden = if truncated {
        total.saturating_sub(lines_vec.len())
    } else {
        0
    };
    if hidden > 0 {
        // Session overflow advertises the in-TUI expand affordance (underline
        // marks the clickable action); command previews keep the share hint.
        let notice_kind = if was_truncated_by_tokens {
            HiddenLinesNoticeKind::TokenBudget
        } else if is_exec_session {
            HiddenLinesNoticeKind::ExecSessionExpand
        } else {
            HiddenLinesNoticeKind::Generic
        };
        let notice = hidden_lines_notice_with(hidden, notice_kind, renderer.supports_inline_ui());
        if is_exec_session {
            // Tag the notice with the recorded capture so the TUI can open the
            // tool-output viewer on this session's complete stdin/stdout body.
            if let Some(anchor) = renderer.take_session_expand_anchor() {
                renderer.set_next_tool_output_anchor(anchor);
            }
            // Dim with the body so the notice does not out-shout the capture,
            // while the underlined action stays a distinct hit target.
            renderer.line_with_override_style(
                MessageStyle::ToolDetail,
                MessageStyle::ToolDetail.style().effects(Effects::DIMMED),
                &notice,
            )?;
        } else {
            renderer.line(MessageStyle::ToolDetail, &notice)?;
        }
    }

    Ok(())
}
