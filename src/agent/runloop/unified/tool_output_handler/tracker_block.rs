//! Inline task-tracker block rendering (progress rows, tree rows, transcript lines).

use super::*;
pub(super) use crate::agent::runloop::tool_output::is_git_diff_payload;

pub(super) fn is_run_pty_tool(name: &str, args_val: &serde_json::Value) -> bool {
    renders_pty_command_header(name, args_val)
}

pub(super) fn is_command_output_call(name: &str, args_val: &serde_json::Value) -> bool {
    name != tools::SEND_PTY_INPUT
        && (name == tools::EXECUTE_CODE
            || tool_intent::is_command_run_tool_call(name, args_val)
            || is_run_pty_tool(name, args_val))
}

pub(super) fn compact_run_completion_line(output: &serde_json::Value, status: ToolDisplayStatus) -> Option<String> {
    if let Some(exit_code) = output.get("exit_code").and_then(serde_json::Value::as_i64) {
        if matches!(status, ToolDisplayStatus::Success) && exit_code == 0 {
            return Some("✓ run completed (exit code: 0)".to_string());
        }
        if matches!(status, ToolDisplayStatus::Warning) && exit_code == 0 {
            return Some("⚠ run completed with warnings (exit code: 0)".to_string());
        }
        return Some(format!("✗ run error, exit code: {exit_code}"));
    }

    if output.get("is_exited").and_then(serde_json::Value::as_bool) == Some(true) {
        if matches!(status, ToolDisplayStatus::Success) {
            return Some("✓ done".to_string());
        }
        if matches!(status, ToolDisplayStatus::Warning) {
            return Some("⚠ done with warnings".to_string());
        }
        return Some("✗ failed".to_string());
    }

    match status {
        ToolDisplayStatus::Failure => Some("✗ failed".to_string()),
        ToolDisplayStatus::Warning => Some("⚠ completed with warnings".to_string()),
        ToolDisplayStatus::Success => None,
    }
}

pub(super) fn has_renderable_stream_content(output: &serde_json::Value) -> bool {
    ["output", "stdout", "stderr", "content"].iter().any(|key| {
        output
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    })
}

pub(super) fn is_task_tracker_tool(name: &str) -> bool {
    matches!(name, tools::TASK_TRACKER | tools::MATRIX)
}

pub(super) fn task_tracker_block_lines(output: &serde_json::Value, expanded: bool) -> Vec<TrackerLine> {
    crate::agent::runloop::tool_output::tracker_transcript_lines(output, expanded)
}

/// Style map for tracker rows: status surfaces through text styling only.
///
/// Done rows render struck-through, italic, and dimmed; the focused current
/// row renders bold in the theme `primary` accent; blocked rows use the
/// `warning` token; other in-progress rows use `primary`; pending rows keep
/// the default style. All colors come from theme tokens through the shared
/// style bridge, so WCAG contrast holds by construction.
pub(super) fn task_tracker_block_segments(lines: &[TrackerLine]) -> Vec<Vec<InlineSegment>> {
    use vtcode_core::ui::markdown::RenderMarkdownOptions;
    use vtcode_core::ui::theme;
    use vtcode_core::ui::tui::convert_style;

    let default_style = std::sync::Arc::new(InlineTextStyle::default());
    let base_style = MessageStyle::Info.style();
    let theme_styles = theme::active_styles();
    let mut info_fallback = convert_style(base_style);
    if info_fallback.color.is_none() {
        info_fallback = info_fallback.merge_color(Some(theme_styles.foreground));
    }
    // `primary`/`warning` meet the WCAG AA floor in every built-in theme.
    let primary = convert_style(theme_styles.primary);
    let warning = convert_style(theme_styles.warning);
    let render_options = RenderMarkdownOptions {
        preserve_code_indentation: true,
        disable_code_block_table_reparse: false,
        table_max_width: None,
    };
    lines
        .iter()
        .map(|line| {
            render_tracker_line(
                line,
                &theme_styles,
                &info_fallback,
                &primary,
                &warning,
                &default_style,
                &render_options,
            )
            .unwrap_or_else(|| {
                vec![InlineSegment {
                    text: line.text.clone(),
                    style: default_style.clone(),
                }]
            })
        })
        .collect()
}

/// Render one tracker line with its status style.
///
/// Headers, diagnostics, truncation, and pending rows keep the default
/// rendering; other statuses tint through [`render_tracker_status_row`].
pub(super) fn render_tracker_line(
    line: &TrackerLine,
    theme_styles: &vtcode_core::ui::theme::ThemeStyles,
    info_fallback: &InlineTextStyle,
    primary: &InlineTextStyle,
    warning: &InlineTextStyle,
    default_style: &std::sync::Arc<InlineTextStyle>,
    render_options: &vtcode_core::ui::markdown::RenderMarkdownOptions,
) -> Option<Vec<InlineSegment>> {
    let row_style = match line.status {
        None | Some(TaskItemStatus::Pending) => {
            return render_tracker_inline_row(&line.text, theme_styles, info_fallback, default_style, render_options);
        }
        Some(TaskItemStatus::Completed) => InlineTextStyle {
            color: None,
            bg_color: None,
            effects: Effects::STRIKETHROUGH | Effects::ITALIC | Effects::DIMMED,
        },
        Some(TaskItemStatus::Blocked) => warning.clone(),
        Some(TaskItemStatus::InProgress) if crate::agent::runloop::tool_output::is_tracker_current_row(&line.text) => {
            let mut current = primary.clone();
            current.effects |= Effects::BOLD;
            current
        }
        Some(TaskItemStatus::InProgress) => primary.clone(),
    };
    render_tracker_status_row(&line.text, &row_style, theme_styles, info_fallback, render_options)
}

/// Split a tree row into its structural prefix and markdown body.
///
/// Handles the current-task marker (`  ▶ `), branch prefixes (`  ├ `), and —
/// for tolerance with legacy payloads — one leading status glyph. Returns
/// `None` for title/diagnostic lines so they keep their plain style.
pub(super) fn split_tracker_row_prefix(line: &str) -> Option<(&str, &str)> {
    let mut rest = line;
    let mut consumed_prefix = false;
    let leading_spaces = rest.len() - rest.trim_start_matches(' ').len();
    rest = &rest[leading_spaces..];
    if let Some(after) = rest.strip_prefix("▶ ") {
        rest = after;
        consumed_prefix = true;
    }
    loop {
        if let Some(after) = rest
            .strip_prefix("├ ")
            .or_else(|| rest.strip_prefix("└ "))
            .or_else(|| rest.strip_prefix("│ "))
        {
            rest = after;
            consumed_prefix = true;
            continue;
        }
        break;
    }
    for token in ["□ ", "[x] ", "[-] ", "[!] "] {
        if let Some(after) = rest.strip_prefix(token) {
            rest = after;
            consumed_prefix = true;
            break;
        }
    }
    if !consumed_prefix || rest.is_empty() {
        return None;
    }
    let prefix_len = line.len() - rest.len();
    Some((&line[..prefix_len], rest))
}

/// Render one compact tree row as styled segments: plain tree prefix plus a
/// markdown-rendered body so `` `code` ``, **bold**, and file paths display
/// styled instead of raw source. Returns `None` when the row has no tree
/// prefix or markdown yields no visible output (caller falls back to plain).
pub(super) fn render_tracker_inline_row(
    line: &str,
    theme_styles: &vtcode_core::ui::theme::ThemeStyles,
    fallback: &InlineTextStyle,
    default_style: &std::sync::Arc<InlineTextStyle>,
    render_options: &vtcode_core::ui::markdown::RenderMarkdownOptions,
) -> Option<Vec<InlineSegment>> {
    use vtcode_core::ui::markdown::render_markdown_to_lines_with_options;
    use vtcode_core::ui::tui::convert_style;

    let (prefix, body) = split_tracker_row_prefix(line)?;
    if body.trim().is_empty() {
        return None;
    }
    let rendered =
        render_markdown_to_lines_with_options(body, MessageStyle::Info.style(), theme_styles, None, *render_options);
    let mut segments = Vec::with_capacity(4);
    segments.push(InlineSegment {
        text: prefix.to_string(),
        style: default_style.clone(),
    });
    // Single-line descriptions stay one row so inline replacement counts stay
    // aligned; join any extra markdown lines with a space.
    let rendered_lines = rendered
        .iter()
        .filter(|rendered_line| !rendered_line.is_empty())
        .collect::<Vec<_>>();
    let mut wrote_body = false;
    for (line_index, rendered_line) in rendered_lines.iter().enumerate() {
        if line_index > 0 {
            segments.push(InlineSegment {
                text: " ".to_string(),
                style: default_style.clone(),
            });
        }
        for seg in &rendered_line.segments {
            if seg.text.is_empty() {
                continue;
            }
            let converted = convert_style(seg.style);
            let mut inline_style = fallback.clone();
            inline_style.color = None;
            if let Some(color) = converted.color
                && Some(color) != fallback.color
            {
                inline_style.color = Some(color);
            }
            if let Some(bg) = converted.bg_color {
                inline_style.bg_color = Some(bg);
            }
            inline_style.effects = converted.effects | fallback.effects;
            segments.push(InlineSegment {
                text: seg.text.clone(),
                style: std::sync::Arc::new(inline_style),
            });
            wrote_body = true;
        }
    }
    wrote_body.then_some(segments)
}

/// Render one status-styled tree row: structural prefix in the row style plus
/// a markdown-rendered body so `` `code` ``, **bold**, and file paths display
/// styled instead of raw source. Unstyled body text takes the row style (done
/// rows strike through, current rows glow in the accent); genuinely distinct
/// markdown spans keep their colors. Returns `None` when the row has no tree
/// prefix or markdown yields no visible output (caller falls back to plain).
pub(super) fn render_tracker_status_row(
    line: &str,
    row_style: &InlineTextStyle,
    theme_styles: &vtcode_core::ui::theme::ThemeStyles,
    info_fallback: &InlineTextStyle,
    render_options: &vtcode_core::ui::markdown::RenderMarkdownOptions,
) -> Option<Vec<InlineSegment>> {
    use vtcode_core::ui::markdown::render_markdown_to_lines_with_options;
    use vtcode_core::ui::tui::convert_style;

    let (prefix, body) = split_tracker_row_prefix(line)?;
    if body.trim().is_empty() {
        return None;
    }
    let rendered =
        render_markdown_to_lines_with_options(body, MessageStyle::Info.style(), theme_styles, None, *render_options);
    let prefix_style = std::sync::Arc::new(row_style.clone());
    let mut segments = Vec::with_capacity(4);
    segments.push(InlineSegment {
        text: prefix.to_string(),
        style: prefix_style.clone(),
    });
    let rendered_lines = rendered
        .iter()
        .filter(|rendered_line| !rendered_line.is_empty())
        .collect::<Vec<_>>();
    let mut wrote_body = false;
    for (line_index, rendered_line) in rendered_lines.iter().enumerate() {
        if line_index > 0 {
            segments.push(InlineSegment { text: " ".to_string(), style: prefix_style.clone() });
        }
        for seg in &rendered_line.segments {
            if seg.text.is_empty() {
                continue;
            }
            let converted = convert_style(seg.style);
            let mut inline_style = row_style.clone();
            if converted.color.is_none_or(|color| Some(color) == info_fallback.color) {
                inline_style.color = row_style.color;
            } else {
                inline_style.color = converted.color;
            }
            if let Some(bg) = converted.bg_color {
                inline_style.bg_color = Some(bg);
            }
            inline_style.effects = converted.effects | row_style.effects;
            // Unstyled body text keeps the row style; the cleared color above
            // already resolves it when markdown matches the Info base.
            if inline_style.color.is_none() {
                inline_style.color = row_style.color;
            }
            segments.push(InlineSegment {
                text: seg.text.clone(),
                style: std::sync::Arc::new(inline_style),
            });
            wrote_body = true;
        }
    }
    wrote_body.then_some(segments)
}

/// Single writer for user-facing tracker transcript blocks.
///
/// Approval handoff and the tool pipeline must share replace/dedupe so progress
/// updates never stack a second block. UI `replace_last` uses the UI write
/// length recorded by this helper — never a TRANSCRIPT-derived count, which
/// can clobber unrelated UI lines when the two stores diverge.
pub(crate) fn write_tracker_progress_transcript(handle: &InlineHandle, lines: Vec<TrackerLine>) {
    if lines.is_empty() {
        return;
    }
    let texts: Vec<String> = lines.iter().map(|line| line.text.clone()).collect();
    let ui_write_len = lines.len();
    if transcript::tail_matches(&texts) {
        transcript::remember_tracker_block_with_ui_len(texts, ui_write_len);
        return;
    }
    let segments = task_tracker_block_segments(&lines);
    if let Some(transcript_count) = transcript::tracker_block_len_if_at_tail() {
        let ui_count = transcript::tracker_ui_write_len().unwrap_or(ui_write_len);
        handle.replace_last(ui_count, InlineMessageKind::Tool, segments);
        transcript::replace_last(transcript_count, &texts);
        transcript::remember_tracker_block_with_ui_len(texts, ui_write_len);
        return;
    }
    // Fallback: remembered block drifted. Only replace a tail that is clearly
    // our tracker shape (progress header, tree row, or truncation line), never
    // generic `• Foo N/M` UI summaries such as Questions answered lines.
    // UI replace uses the remembered UI length; transcript replace uses the
    // trailing tracker-like count so expanded blocks replace fully.
    if transcript::last_line().is_some_and(|last| looks_like_tracker_content(&last)) {
        let ui_count = transcript::tracker_ui_write_len().unwrap_or(1).max(1);
        let transcript_count = trailing_tracker_content_len().max(1);
        handle.replace_last(ui_count, InlineMessageKind::Tool, segments);
        transcript::replace_last(transcript_count, &texts);
        transcript::remember_tracker_block_with_ui_len(texts, ui_write_len);
        return;
    }
    for (segments, plain_line) in segments.into_iter().zip(texts.iter()) {
        handle.append_line(InlineMessageKind::Tool, segments);
        transcript::append(plain_line);
    }
    transcript::remember_tracker_block_with_ui_len(texts, ui_write_len);
}

pub(super) fn looks_like_tracker_progress_line(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with("• ") {
        return false;
    }
    if trimmed.starts_with("• Plan") || trimmed.starts_with("• Ran") || trimmed.starts_with("• Questions") {
        return false;
    }
    if trimmed.starts_with("• Tasks") {
        return true;
    }
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.len() < 3 {
        return false;
    }
    if tokens
        .iter()
        .any(|token| token.eq_ignore_ascii_case("answered") || token.eq_ignore_ascii_case("questions"))
    {
        return false;
    }
    let Some(last) = tokens.last() else {
        return false;
    };
    let mut parts = last.split('/');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(a), Some(b), None) if !a.is_empty()
            && !b.is_empty()
            && a.chars().all(|c| c.is_ascii_digit())
            && b.chars().all(|c| c.is_ascii_digit())
    )
}

pub(super) fn looks_like_tracker_tree_row(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("▶ ") {
        return true;
    }
    if trimmed.starts_with("├ ") || trimmed.starts_with("└ ") || trimmed.starts_with("│ ") {
        return true;
    }
    if trimmed.starts_with("□ ")
        || trimmed.starts_with("[x] ")
        || trimmed.starts_with("[-] ")
        || trimmed.starts_with("[!] ")
    {
        return true;
    }
    // Expanded truncation row (`  … N more`).
    trimmed.starts_with('…') || trimmed.starts_with("...")
}

pub(super) fn looks_like_tracker_content(line: &str) -> bool {
    looks_like_tracker_progress_line(line) || looks_like_tracker_tree_row(line)
}

pub(super) fn trailing_tracker_content_len() -> usize {
    const MAX_SCAN: usize = 40;
    transcript::snapshot()
        .iter()
        .rev()
        .take(MAX_SCAN)
        .take_while(|line| looks_like_tracker_content(line))
        .count()
}

pub(super) fn apply_task_tracker_block(handle: &InlineHandle, lines: Vec<TrackerLine>) {
    write_tracker_progress_transcript(handle, lines);
}
