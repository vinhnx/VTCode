//! Split from streams.rs; see module docs there.

use super::*;

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(crate) fn render_diff_content_block(
    renderer: &mut AnsiRenderer,
    diff_content: &str,
    tool_name: Option<&str>,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    fallback_style: MessageStyle,
    mode: ToolOutputMode,
    tail_limit: usize,
) -> Result<()> {
    let language = diff_language_hint_from_content(diff_content);
    render_diff_content_block_with_language(
        renderer,
        diff_content,
        tool_name,
        git_styles,
        ls_styles,
        fallback_style,
        mode,
        tail_limit,
        language.as_deref(),
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(crate) fn render_diff_content_block_with_language(
    renderer: &mut AnsiRenderer,
    diff_content: &str,
    tool_name: Option<&str>,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    fallback_style: MessageStyle,
    mode: ToolOutputMode,
    tail_limit: usize,
    language: Option<&str>,
) -> Result<()> {
    let diff_lines = display_lines_from_unified_diff(diff_content);
    let effective_limit = if renderer.prefers_untruncated_output() || matches!(mode, ToolOutputMode::Full) {
        tail_limit.max(1000)
    } else {
        tail_limit
    };
    let bounded_lines = bounded_display_lines(&diff_lines, effective_limit);
    let lines_slice = bounded_lines.as_slice();
    let vertical_omitted = lines_slice.len() < diff_lines.len();
    let line_number_width = diff_display_line_number_width(lines_slice);
    let available_width = renderer.diff_content_width(fallback_style);
    let wrap_for_reflow = renderer.prefers_untruncated_output();
    let review_path = resolve_diff_review_path(diff_content);
    let pretruncated = is_pretruncated_diff_content(diff_content);
    let has_row_background = git_styles.add.as_ref().is_some_and(|style| style.get_bg_color().is_some());
    let show_gutter_probe = should_show_diff_gutter(
        renderer.capabilities().supports_color(),
        has_row_background,
        available_width,
        line_number_width,
    );
    // Expandable "full diff" is only honest when the function still holds the
    // complete body in `diff_content` and we clipped it for display.
    let can_expand_full = wrap_for_reflow && !pretruncated;
    let safety_capped = can_expand_full
        && lines_slice
            .iter()
            .any(|line| line_exceeds_wrap_safety_cap(line, line_number_width, show_gutter_probe));

    // Bound syntect cost: large previews fall back to solid tints so a
    // 100-file `apply_patch` stays O(n) tinting instead of O(n) parses.
    // `should_highlight`-style byte budget, applied once per block.
    let language = if diff_content.len() > 50_000 { None } else { language };

    if renderer.diff_preview_mode() == vtcode_commons::ui_protocol::DiffPreviewMode::SideBySide
        && diff_side_by_side_fits(available_width)
    {
        let result =
            render_diff_content_side_by_side_with_language(renderer, lines_slice, git_styles, fallback_style, language);
        if can_expand_full && (vertical_omitted || safety_capped) {
            attach_diff_review_anchor(
                renderer,
                diff_content,
                diff_lines.len().saturating_sub(lines_slice.len()),
                safety_capped,
                review_path.as_str(),
            );
        }
        return result;
    }

    // Without ANSI styling the row tint and foreground fallback disappear,
    // so keep the marker/line-number gutter as the only remaining add/delete
    // distinction even when the content width is narrow.
    let show_gutter = show_gutter_probe;
    let result = render_diff_content_inline_with_language(
        renderer,
        lines_slice,
        tool_name,
        git_styles,
        ls_styles,
        fallback_style,
        line_number_width,
        show_gutter,
        language,
        review_path.as_str(),
        can_expand_full,
    );
    if can_expand_full && (vertical_omitted || safety_capped) {
        attach_diff_review_anchor(
            renderer,
            diff_content,
            diff_lines.len().saturating_sub(lines_slice.len()),
            safety_capped,
            review_path.as_str(),
        );
        if safety_capped && !vertical_omitted {
            // Spec S2.1: safety-cap truncation must advertise expand even when
            // the vertical budget retained every logical row.
            let notice = vtcode_commons::ui_protocol::diff_review_notice(&review_path, 0, true);
            renderer.line(MessageStyle::ToolDetail, &notice)?;
        }
    }
    result
}

/// Attach a UI-only expand payload when the transcript body was clipped and
/// `diff_content` still holds the complete source body.
fn attach_diff_review_anchor(
    renderer: &AnsiRenderer,
    diff_content: &str,
    omitted_lines: usize,
    safety_capped: bool,
    file_path: &str,
) {
    if diff_content.is_empty() || (omitted_lines == 0 && !safety_capped) || is_pretruncated_diff_content(diff_content) {
        return;
    }
    let file_path = if is_generic_diff_review_label(file_path) {
        resolve_diff_review_path(diff_content)
    } else {
        file_path.to_owned()
    };
    let notice = vtcode_commons::ui_protocol::diff_review_notice(&file_path, omitted_lines as u64, safety_capped);
    renderer.record_diff_review(vtcode_commons::ui_protocol::DiffReviewAnchor {
        file_path,
        unified: diff_content.to_owned(),
        omitted_lines: omitted_lines as u64,
        notice,
    });
}

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
fn render_diff_content_inline_with_language(
    renderer: &mut AnsiRenderer,
    lines_slice: &[DiffDisplayLine],
    tool_name: Option<&str>,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    fallback_style: MessageStyle,
    line_number_width: usize,
    show_gutter: bool,
    language: Option<&str>,
    review_path: &str,
    can_expand_full: bool,
) -> Result<()> {
    let color_enabled = renderer.capabilities().supports_color();
    let target_width = renderer.diff_content_width(fallback_style);
    // Inline TUI sinks word-wrap transcript rows with a hanging gutter indent,
    // so emit logical bodies whole instead of ellipsis-truncating to the
    // measured width. CLI/no-sink renders keep the bounded preview cap.
    let wrap_for_reflow = renderer.prefers_untruncated_output();
    let max_line_width = if wrap_for_reflow {
        DIFF_WRAP_SOURCE_MAX_WIDTH
    } else {
        target_width.map_or(MAX_LINE_LENGTH, |width| width.min(MAX_LINE_LENGTH))
    };
    let mut formatted_buffer = String::with_capacity(256);
    let mut display_buffer = String::with_capacity(256);
    // Explicit `language` (single-file file-ops previews) wins for every row.
    // Otherwise track the current file from `diff --git` / `---` / `+++`
    // headers so multi-file diffs highlight each file with its own grammar
    // instead of the first file's grammar.
    let mut current_hint: Option<String> = None;

    for line in lines_slice {
        if language.is_none()
            && matches!(line.kind, DiffDisplayKind::Metadata)
            && let Some(hint) = language_hint_for_display_line(line)
        {
            current_hint = Some(hint);
        }
        let effective_language = language.or(current_hint.as_deref());
        display_buffer.clear();
        let raw_line = diff_display_text(line, line_number_width, show_gutter);
        if raw_line.is_empty() {
            continue;
        }
        // Expandable omission copy only when the full body is still available
        // for review (not a pre-truncated registry excerpt).
        if can_expand_full && wrap_for_reflow && raw_line.contains("lines omitted") {
            if let Some(omitted) = parse_omitted_line_count(&raw_line) {
                display_buffer.push_str(&vtcode_commons::ui_protocol::diff_review_notice(review_path, omitted, false));
            } else if !raw_line.contains("review full diff") {
                display_buffer.push_str(&format!("{raw_line} — review full diff for {review_path}"));
            } else {
                display_buffer.push_str(&raw_line);
            }
        } else if display_width(&raw_line) > max_line_width {
            display_buffer.push_str(&truncate_with_ellipsis(&raw_line, max_line_width, "..."));
        } else {
            display_buffer.push_str(&raw_line);
        }

        if let Some(summary_line) =
            colorize_diff_summary_line(&display_buffer, renderer.capabilities().supports_color())
        {
            render_preview_line(
                renderer,
                &display_buffer,
                Some(&summary_line),
                None,
                false,
                fallback_style,
                Some(fallback_style.style()),
            )?;
            continue;
        }

        let line_style = select_render_line_style(line, &display_buffer, tool_name, git_styles, ls_styles);
        let line_style = semantic_diff_line_style(line, line_style, git_styles);
        let word_bg = match line.kind {
            DiffDisplayKind::Addition => git_styles.add_word.and_then(|s| s.get_bg_color()),
            DiffDisplayKind::Deletion => git_styles.remove_word.and_then(|s| s.get_bg_color()),
            _ => None,
        };
        // Route add/delete rows through the shared formatter; compact layouts
        // omit the visible gutter while prose falls back to the solid tint.
        let rendered_owned = if color_enabled
            && (!show_gutter || target_width.is_none_or(|width| width >= diff_gutter_width(line_number_width)))
        {
            Some(format_diff_line_with_gutter_and_syntax_to_width(
                line,
                line_style,
                line_number_width,
                word_bg,
                git_styles,
                target_width,
                show_gutter,
                effective_language,
                wrap_for_reflow,
                &mut formatted_buffer,
            ))
        } else {
            None
        };

        render_preview_line(
            renderer,
            &display_buffer,
            rendered_owned.filter(|r| {
                // Expandable omission copy in `display_buffer` is authoritative
                // when present; do not let the raw numbered formatter replace it
                // with a path-less variant.
                if display_buffer.contains("review full diff") {
                    false
                } else {
                    *r != display_buffer.as_str()
                }
            }),
            None,
            false,
            fallback_style,
            line_style,
        )?;
    }

    Ok(())
}

/// Render diff display lines as dual-pane (old | new) rows.
///
/// Context lines appear on both sides. Consecutive deletion/addition runs are
/// zipped index-wise. Hunk headers and metadata span the full width.
#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
fn render_diff_content_side_by_side_with_language(
    renderer: &mut AnsiRenderer,
    lines_slice: &[DiffDisplayLine],
    git_styles: &GitStyles,
    fallback_style: MessageStyle,
    language: Option<&str>,
) -> Result<()> {
    let rows = side_by_side_rows(lines_slice);
    let line_number_width = diff_display_line_number_width(lines_slice);
    let color_enabled = renderer.capabilities().supports_color();
    let wrap_for_reflow = renderer.prefers_untruncated_output();
    let mut formatted_buffer = String::with_capacity(512);
    let mut display_buffer = String::with_capacity(512);

    // Target pane width: leave room for gutter + divider within the actual
    // content width when terminal sizing is available. Keep the bounded
    // fallback for non-terminal tests and redirected output.
    let total_width = renderer.diff_content_width(fallback_style).unwrap_or(MAX_LINE_LENGTH);
    let pane_width = ((total_width.saturating_sub(3)) / 2).max(20);
    let full_width_limit = if wrap_for_reflow {
        DIFF_WRAP_SOURCE_MAX_WIDTH
    } else {
        total_width.min(MAX_LINE_LENGTH)
    };
    let mut current_hint: Option<String> = None;

    for row in rows {
        // Track the current file from full-width metadata so each pane
        // highlights with its own grammar when no explicit hint is given.
        if language.is_none()
            && row.is_full_width()
            && let Some(left) = row.left.as_ref()
            && matches!(left.kind, DiffDisplayKind::Metadata)
            && let Some(hint) = language_hint_for_display_line(left)
        {
            current_hint = Some(hint);
        }
        let effective_language = language.or(current_hint.as_deref());
        display_buffer.clear();
        let raw_line = format_side_by_side_row_plain_to_width(&row, line_number_width, pane_width, wrap_for_reflow);
        if raw_line.is_empty() {
            continue;
        }
        // Source rows are already bounded independently by `pane_width`; do
        // not apply the single-line preview cap to the combined panes on a
        // wide terminal or the right pane would be clipped before styling.
        let was_truncated = row.is_full_width() && display_width(&raw_line) > full_width_limit;
        if was_truncated {
            display_buffer.push_str(&truncate_with_ellipsis(&raw_line, full_width_limit, "..."));
        } else {
            display_buffer.push_str(&raw_line);
        }

        if let Some(summary_line) =
            colorize_diff_summary_line(&display_buffer, renderer.capabilities().supports_color())
        {
            render_preview_line(
                renderer,
                &display_buffer,
                Some(&summary_line),
                None,
                false,
                fallback_style,
                Some(fallback_style.style()),
            )?;
            continue;
        }

        // Side-by-side rows carry their own per-pane ANSI backgrounds.
        // Pass a bg-less override so an empty left/right cell cannot inherit
        // the sibling pane's tint via the line-level style.
        let rendered_owned = if color_enabled && !was_truncated {
            Some(format_side_by_side_row_ansi(
                &row,
                line_number_width,
                pane_width,
                git_styles,
                effective_language,
                &mut formatted_buffer,
            ))
        } else {
            None
        };

        render_preview_line(
            renderer,
            &display_buffer,
            rendered_owned.filter(|r| *r != display_buffer.as_str()),
            None,
            false,
            fallback_style,
            Some(AnsiStyle::new()),
        )?;
    }

    Ok(())
}

/// Plain-text dual-pane row for fallback / truncation.
fn format_side_by_side_row_plain_to_width(
    row: &SideBySideRow,
    number_width: usize,
    pane_width: usize,
    wrap_for_reflow: bool,
) -> String {
    if row.is_full_width() {
        return row.left.as_ref().map(|l| l.text.clone()).unwrap_or_default();
    }
    let left = pane_cell_plain(row.left.as_ref(), number_width, pane_width, wrap_for_reflow);
    let right = pane_cell_plain(row.right.as_ref(), number_width, pane_width, wrap_for_reflow);
    format!("{left}│{right}")
}

fn pane_cell_plain(
    line: Option<&DiffDisplayLine>,
    number_width: usize,
    pane_width: usize,
    wrap_for_reflow: bool,
) -> String {
    let Some(line) = line else {
        return " ".repeat(pane_width);
    };
    let (marker, number) = match line.kind {
        DiffDisplayKind::Addition => ('+', line.new_line),
        DiffDisplayKind::Deletion => ('-', line.old_line),
        _ => (' ', line.new_line.or(line.old_line)),
    };
    let no = number.map(|n| n.to_string()).unwrap_or_default();
    let gutter = format!("{marker}{no:>number_width$}│");
    let body_width = if wrap_for_reflow {
        DIFF_WRAP_SOURCE_MAX_WIDTH
    } else {
        pane_width.saturating_sub(gutter.chars().count())
    };
    let body = truncate_chars_to_width(&line.text, body_width);
    let pad = pane_width
        .saturating_sub(gutter.chars().count())
        .saturating_sub(display_width(&body));
    format!("{gutter}{body}{}", " ".repeat(pad))
}

/// ANSI-styled dual-pane row with per-pane backgrounds.
pub(crate) fn format_side_by_side_row_ansi<'a>(
    row: &SideBySideRow,
    number_width: usize,
    pane_width: usize,
    git_styles: &GitStyles,
    language: Option<&str>,
    out: &'a mut String,
) -> &'a str {
    out.clear();
    use std::fmt::Write as _;
    let reset_background = diff_background_reset(git_styles);

    if row.is_full_width() {
        let text = row.left.as_ref().map(|l| l.text.as_str()).unwrap_or("");
        let _ = write!(out, "{Reset}");
        let style = row.left.as_ref().and_then(|line| match line.kind {
            DiffDisplayKind::HunkHeader => git_styles.hunk,
            DiffDisplayKind::Metadata if line.text.starts_with("--- ") => git_styles.file_old,
            DiffDisplayKind::Metadata if line.text.starts_with("+++ ") => git_styles.file_new,
            DiffDisplayKind::Metadata => git_styles.header,
            _ => None,
        });
        if let Some(style) = style {
            let _ = write!(out, "{style}");
        }
        out.push_str(text);
        let _ = write!(out, "{Reset}");
        return out.as_str();
    }

    let left = format_side_by_side_pane_ansi(row.left.as_ref(), number_width, pane_width, git_styles, language);
    let right = format_side_by_side_pane_ansi(row.right.as_ref(), number_width, pane_width, git_styles, language);
    // Start the row with a full reset + default bg so nothing carries over
    // from the previous line.
    out.push_str(reset_background);
    out.push_str(&left);
    let divider = AnsiStyle::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightBlack)));
    let _ = write!(out, "{divider}│{reset_background}");
    out.push_str(&right);
    // End with full reset so the next line starts clean.
    let _ = write!(out, "\x1b[0m");
    out.as_str()
}

fn diff_background_reset(git_styles: &GitStyles) -> &'static str {
    if git_styles.add.as_ref().is_some_and(|style| style.get_bg_color().is_some()) {
        "\x1b[0m\x1b[49m"
    } else {
        "\x1b[0m"
    }
}

fn format_side_by_side_pane_ansi(
    line: Option<&DiffDisplayLine>,
    number_width: usize,
    pane_width: usize,
    git_styles: &GitStyles,
    language: Option<&str>,
) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(pane_width + 16);
    let Some(line) = line else {
        // SGR 49 = default background. More reliable than SGR 0 (full reset)
        // for clearing an inherited tint in color-capable terminal parsers;
        // ANSI16 stays foreground-only and needs only the full reset.
        out.push_str(diff_background_reset(git_styles));
        out.push_str(&" ".repeat(pane_width));
        return out;
    };
    let base_style = select_line_style_for_kind(line, git_styles);
    let bg = base_style.and_then(|s| s.get_bg_color());
    let word_bg = match line.kind {
        DiffDisplayKind::Addition => git_styles.add_word.and_then(|s| s.get_bg_color()),
        DiffDisplayKind::Deletion => git_styles.remove_word.and_then(|s| s.get_bg_color()),
        _ => None,
    };
    let (marker, number) = match line.kind {
        DiffDisplayKind::Addition => ('+', line.new_line),
        DiffDisplayKind::Deletion => ('-', line.old_line),
        _ => (' ', line.new_line.or(line.old_line)),
    };
    let no = number.map(|n| n.to_string()).unwrap_or_default();

    // One continuous tint through the whole pane: marker + number + │ + body
    // + pad. No Reset between cells — that would carve out an unstyled gutter
    // strip next to the coloured body.
    let marker_style = match marker {
        '+' => AnsiStyle::new().fg_color(Some(git_styles.addition_fg)).bg_color(bg),
        '-' => AnsiStyle::new().fg_color(Some(git_styles.deletion_fg)).bg_color(bg),
        _ => AnsiStyle::new().fg_color(Some(git_styles.gutter_fg)).bg_color(bg),
    };
    // Avoid DIM: its attenuation is terminal-dependent and can fail contrast
    // against the stronger intraline background.
    let gutter_style = AnsiStyle::new().fg_color(Some(git_styles.gutter_fg)).bg_color(bg);
    // Color-capable rows use the row tint and reserve the stronger tint for
    // changed spans. ANSI16 uses the bright body foreground fallback.
    let body_style = match marker {
        '+' => git_styles.add.unwrap_or_default(),
        '-' => git_styles.remove.unwrap_or_default(),
        _ => AnsiStyle::new().bg_color(bg),
    };

    let _ = write!(out, "{marker_style}{marker}");
    let _ = write!(out, "{gutter_style}{no:>number_width$}│");
    // Gutter is 1 (sign) + number_width + 1 (│).
    let body_width = pane_width.saturating_sub(2 + number_width);
    let truncated = display_width(&line.text) > body_width;
    let body = truncate_chars_to_width(&line.text, body_width);
    // Skip chips on truncated rows — `changed` offsets are into the original
    // text and would highlight the wrong slice after truncation.
    let word_ranges: &[(usize, usize)] = if truncated { &[] } else { &line.changed };
    let highlighted = if truncated {
        highlight_diff_content(&body, bg, word_ranges, word_bg)
    } else {
        highlight_diff_body_with_syntax(&body, language, bg, word_ranges, word_bg, None)
            .or_else(|| highlight_diff_content(&body, bg, word_ranges, word_bg))
    };
    match highlighted {
        Some(hl) => {
            out.push_str(&hl);
        }
        None => {
            let _ = write!(out, "{body_style}{body}");
        }
    }
    // Pad to exact remaining cells so the pane stays aligned.
    // Track used width in display cells, not chars, to handle wide glyphs.
    let mut used = 1 + number_width + 1; // sign + number + │
    for ch in body.chars() {
        used += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
    }
    let pad = pane_width.saturating_sub(used);
    if pad > 0 {
        let _ = write!(out, "{body_style}{}", " ".repeat(pad));
    }
    // End the pane with a reset so the next pane starts clean.
    let _ = write!(out, "{Reset}");
    out
}

/// Truncate to at most `max_width` display cells without an ellipsis marker.
fn truncate_chars_to_width(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let mut out = String::with_capacity(text.len());
    let mut used = 0usize;
    for ch in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if used + w > max_width {
            break;
        }
        out.push(ch);
        used += w;
    }
    out
}

pub(crate) fn select_line_style_for_kind(line: &DiffDisplayLine, git_styles: &GitStyles) -> Option<AnsiStyle> {
    match line.kind {
        DiffDisplayKind::Addition => git_styles.add,
        DiffDisplayKind::Deletion => git_styles.remove,
        _ => None,
    }
}
