//! Split from streams.rs; see module docs there.

use super::*;

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(crate) fn render_preview_line(
    renderer: &mut AnsiRenderer,
    display_line: &str,
    rendered_line: Option<&str>,
    prefix: Option<&str>,
    truncate_line: bool,
    fallback_style: MessageStyle,
    override_style: Option<AnsiStyle>,
) -> Result<()> {
    if display_line.is_empty() {
        return Ok(());
    }

    let line = rendered_line.unwrap_or(display_line);

    if !truncate_line || display_width(line) <= MAX_LINE_LENGTH {
        return match prefix {
            Some(pfx) => {
                let mut buf = String::with_capacity(pfx.len() + line.len());
                buf.push_str(pfx);
                buf.push_str(line);
                renderer.line_with_override_style(
                    fallback_style,
                    override_style.unwrap_or(fallback_style.style()),
                    &buf,
                )
            }
            None => renderer.line_with_override_style(
                fallback_style,
                override_style.unwrap_or(fallback_style.style()),
                line,
            ),
        };
    }

    let truncated = truncate_with_ellipsis(line, MAX_LINE_LENGTH, "...");
    let text = if let Some(pfx) = prefix {
        let mut buf = String::with_capacity(pfx.len() + truncated.len());
        buf.push_str(pfx);
        buf.push_str(&truncated);
        buf
    } else {
        truncated
    };

    renderer.line_with_override_style(fallback_style, override_style.unwrap_or(fallback_style.style()), &text)
}

pub(crate) fn collect_run_command_preview(content: &str) -> (SmallVec<[&str; 32]>, usize, usize) {
    let preview = excerpt_text_lines(content, RUN_COMMAND_HEAD_PREVIEW_LINES, RUN_COMMAND_TAIL_PREVIEW_LINES);
    let mut collected: SmallVec<[&str; 32]> = SmallVec::with_capacity(preview.head.len() + preview.tail.len());
    collected.extend(preview.head.iter().copied());
    collected.extend(preview.tail.iter().copied());
    (collected, preview.total, preview.hidden_count)
}

/// Trims a streaming preview to its `cap` most recent rows.
///
/// Callers read the tail (last rows carry the summary, exit line, and errors),
/// so rows are dropped from the front. Returns `true` when anything was
/// dropped, which the caller turns into the "truncated" flag used for the
/// hidden-lines notice. A `cap` of 0 keeps nothing rather than overflowing.
pub(crate) fn trim_to_tail(lines: &mut SmallVec<[&str; 32]>, cap: usize) -> bool {
    if lines.len() <= cap {
        return false;
    }
    lines.drain(..lines.len() - cap);
    true
}

pub(crate) async fn render_run_command_preview(
    renderer: &mut AnsiRenderer,
    content: &str,
    tool_name: Option<&str>,
    fallback_style: MessageStyle,
    disable_spool: bool,
    config: Option<&VTCodeConfig>,
) -> Result<()> {
    let run_tool_name = tool_name.unwrap_or(vtcode_core::config::constants::tools::RUN_PTY_CMD);
    if !disable_spool && let Ok(Some(log_path)) = spool_output_if_needed(content, run_tool_name, config).await {
        let total = content.lines().count();
        renderer.line(
            MessageStyle::ToolDetail,
            &format!(
                "Command output too large ({} bytes, {} lines), spooled to: {}",
                content.len(),
                total,
                log_path.display()
            ),
        )?;
    }

    let (preview_lines, _total, hidden) = collect_run_command_preview(content);
    if preview_lines.is_empty() {
        return Ok(());
    }

    // Show hidden lines notice if needed
    if hidden > 0 {
        renderer.line(MessageStyle::ToolDetail, &hidden_lines_notice(hidden, HiddenLinesNoticeKind::CommandPreview))?;
    }

    // Render command output with bash syntax highlighting.
    // Wrap the preview lines in markdown code fences with "bash" language hint
    // to get proper syntax highlighting for command output.
    let lines_vec: SmallVec<[&str; 32]> = preview_lines.iter().copied().collect();
    let markdown = build_markdown_code_block(&lines_vec, Some("bash"), true);
    renderer.render_markdown_output(fallback_style, &markdown)?;

    Ok(())
}
