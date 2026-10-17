//! Split from streams.rs; see module docs there.

use super::*;

pub(crate) fn highlight_diff_content(
    content: &str,
    bg: Option<anstyle::Color>,
    word_ranges: &[(usize, usize)],
    word_bg: Option<anstyle::Color>,
) -> Option<String> {
    highlight_diff_content_with_foreground(content, bg, word_ranges, word_bg, None)
}

fn highlight_diff_content_with_foreground(
    content: &str,
    bg: Option<anstyle::Color>,
    word_ranges: &[(usize, usize)],
    word_bg: Option<anstyle::Color>,
    foreground: Option<anstyle::Color>,
) -> Option<String> {
    // ANSI16/no-color mode passes `bg: None`, so there is no tint or word
    // chip to paint. Return `None` so the caller falls through to the
    // foreground fallback body style.
    if content.is_empty() {
        return None;
    }
    let bg = bg?;
    let mut out = String::with_capacity(content.len() + 16);
    // Reset first so no prior SGR state bleeds into this run.
    out.push_str(&Reset.to_string());

    if word_ranges.is_empty() || word_bg.is_none() {
        out.push_str(&AnsiStyle::new().fg_color(foreground).bg_color(Some(bg)).render().to_string());
        out.push_str(content);
        out.push_str(&Reset.to_string());
        return Some(out);
    }

    // Two-level: line tint on unchanged spans; stronger chip on changed.
    let word_bg = word_bg.expect("checked above");
    let mut cursor = 0usize;
    for &(start, end) in word_ranges {
        let start = start.min(content.len());
        let end = end.min(content.len()).max(start);
        if start > cursor {
            out.push_str(&AnsiStyle::new().fg_color(foreground).bg_color(Some(bg)).render().to_string());
            out.push_str(&content[cursor..start]);
            out.push_str(&Reset.to_string());
        }
        if end > start {
            out.push_str(
                &AnsiStyle::new()
                    .fg_color(foreground)
                    .bg_color(Some(word_bg))
                    .render()
                    .to_string(),
            );
            out.push_str(&content[start..end]);
            out.push_str(&Reset.to_string());
        }
        cursor = end.max(cursor);
    }
    if cursor < content.len() {
        out.push_str(&AnsiStyle::new().fg_color(foreground).bg_color(Some(bg)).render().to_string());
        out.push_str(&content[cursor..]);
        out.push_str(&Reset.to_string());
    }
    Some(out)
}
/// Syntax segments for a diff body line, or `None` when solid tint applies.
///
/// Prose (`md`/`txt`), unknown/plain grammars, and foreground-less results
/// stay solid so the add/del row tint carries the semantics. Reuses the
/// markdown pipeline's brightened `highlight_line_for_diff` so ANSI rows meet
/// the same WCAG contrast as markdown diff rows on the same tint.
pub(crate) fn syntax_segments_for_diff_body(content: &str, language: Option<&str>) -> Option<Vec<(AnsiStyle, String)>> {
    let hint = language.map(str::trim).filter(|hint| !hint.is_empty())?;
    if is_prose_language_hint(Some(hint)) {
        return None;
    }
    if std::ptr::eq(
        vtcode_ui::tui::ui::syntax_highlight::find_syntax_by_token(hint),
        vtcode_ui::tui::ui::syntax_highlight::find_syntax_plain_text(),
    ) {
        return None;
    }
    let segments = vtcode_ui::tui::ui::markdown::highlight_line_for_diff(content, Some(hint))?;
    if segments.is_empty() {
        return None;
    }
    let reconstructed: String = segments.iter().map(|(_, text)| text.as_str()).collect();
    if reconstructed != content {
        return None;
    }
    // Require at least one explicit foreground; otherwise the grammar added
    // no semantic value and the solid tint is more legible.
    if segments
        .iter()
        .all(|(style, text)| style.get_fg_color().is_none() || text.trim().is_empty())
    {
        return None;
    }
    Some(segments)
}

/// Layer syntax foregrounds over the diff row tint + intraline chips.
///
/// Keeps the row background (`bg`) and stronger changed-span background
/// (`word_bg`) while preserving per-token syntax foregrounds. Falls back to
/// `foreground` when a syntax segment carries no explicit color.
pub(crate) fn highlight_diff_body_with_syntax(
    content: &str,
    language: Option<&str>,
    bg: Option<anstyle::Color>,
    word_ranges: &[(usize, usize)],
    word_bg: Option<anstyle::Color>,
    foreground: Option<anstyle::Color>,
) -> Option<String> {
    let bg = bg?;
    if content.is_empty() {
        return None;
    }
    let segments = syntax_segments_for_diff_body(content, language)?;
    let mut out = String::with_capacity(content.len() + segments.len() * 16);
    out.push_str(&Reset.to_string());
    let mut cursor = 0usize;
    for (style, text) in &segments {
        let segment_len = text.len();
        let segment_end = cursor.saturating_add(segment_len);
        let mut boundaries = Vec::with_capacity(word_ranges.len() * 2 + 2);
        boundaries.push(0);
        boundaries.push(segment_len);
        for &(start, end) in word_ranges {
            if start < segment_end && end > cursor {
                let local_start = start.max(cursor).saturating_sub(cursor).min(segment_len);
                let local_end = end.min(segment_end).saturating_sub(cursor).min(segment_len);
                boundaries.push(local_start);
                boundaries.push(local_end);
            }
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        for pair in boundaries.windows(2) {
            let start = pair[0];
            let end = pair[1];
            if start >= end {
                continue;
            }
            // Clamp to UTF-8 boundaries instead of dropping bytes: word ranges
            // from `vtcode-diff` are byte-safe today, but clamping keeps the
            // row lossless if a producer ever emits a mid-char offset.
            let start = text.floor_char_boundary(start).min(text.len());
            let end = text.ceil_char_boundary(end).min(text.len());
            if start >= end {
                continue;
            }
            let global_start = cursor.saturating_add(start);
            let global_end = cursor.saturating_add(end);
            let changed = word_bg.is_some()
                && word_ranges
                    .iter()
                    .any(|&(range_start, range_end)| global_start < range_end && global_end > range_start);
            let background = if changed { word_bg } else { Some(bg) };
            let mut effective = *style;
            if effective.get_fg_color().is_none() {
                effective = effective.fg_color(foreground);
            }
            effective = effective.bg_color(background);
            out.push_str(&effective.render().to_string());
            out.push_str(&text[start..end]);
            out.push_str(&Reset.to_string());
        }
        cursor = segment_end;
    }
    Some(out)
}

pub(crate) fn semantic_diff_line_style(
    line: &DiffDisplayLine,
    style: Option<AnsiStyle>,
    git_styles: &GitStyles,
) -> Option<AnsiStyle> {
    let mut style = style?;
    if style.get_fg_color().is_none() {
        let foreground = match line.kind {
            DiffDisplayKind::Addition => Some(git_styles.addition_fg),
            DiffDisplayKind::Deletion => Some(git_styles.deletion_fg),
            _ => None,
        };
        if let Some(foreground) = foreground {
            style = style.fg_color(Some(foreground));
        }
    }
    Some(style)
}

pub(crate) fn diff_display_text(line: &DiffDisplayLine, line_number_width: usize, show_gutter: bool) -> String {
    if show_gutter || !line.kind.is_diff() {
        return line.numbered_text(line_number_width);
    }

    // Keep one styled leading cell as the compact row marker. It preserves
    // diff-row identity in the inline transcript without spending columns on
    // +/- signs, line numbers, or the vertical separator.
    if line.text.is_empty() {
        "  ".to_owned()
    } else {
        format!(" {}", line.text)
    }
}

pub(crate) fn select_render_line_style(
    line: &DiffDisplayLine,
    display_line: &str,
    tool_name: Option<&str>,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Option<AnsiStyle> {
    select_line_style_for_kind(line, git_styles).or_else(|| {
        // In the compact layout context rows start with a single blank, so a
        // source line beginning with `+` or `-` must not be reclassified as an
        // insertion or deletion by the generic parser.
        (!line.kind.is_diff())
            .then(|| select_line_style(tool_name, display_line, git_styles, ls_styles))
            .flatten()
    })
}

pub(crate) fn should_show_diff_gutter(
    color_enabled: bool,
    has_row_background: bool,
    available_width: Option<usize>,
    line_number_width: usize,
) -> bool {
    !color_enabled
        || !has_row_background
        || available_width.is_none_or(|width| diff_gutter_fits(width, line_number_width))
}

#[cfg(test)]
pub(crate) fn format_diff_line_with_gutter_and_syntax<'a>(
    line: &DiffDisplayLine,
    base_style: Option<AnsiStyle>,
    line_number_width: usize,
    word_bg: Option<anstyle::Color>,
    git_styles: &GitStyles,
    out: &'a mut String,
) -> &'a str {
    format_diff_line_with_gutter_and_syntax_to_width(
        line,
        base_style,
        line_number_width,
        word_bg,
        git_styles,
        None,
        true,
        None,
        false,
        out,
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(crate) fn format_diff_line_with_gutter_and_syntax_to_width<'a>(
    line: &DiffDisplayLine,
    base_style: Option<AnsiStyle>,
    line_number_width: usize,
    word_bg: Option<anstyle::Color>,
    git_styles: &GitStyles,
    target_width: Option<usize>,
    show_gutter: bool,
    language: Option<&str>,
    wrap_for_reflow: bool,
    out: &'a mut String,
) -> &'a str {
    use std::fmt::Write as _;

    out.clear();
    let (marker, mut content) = match line.kind {
        DiffDisplayKind::Addition => ('+', line.text.as_str()),
        DiffDisplayKind::Deletion => ('-', line.text.as_str()),
        DiffDisplayKind::Context => (' ', line.text.as_str()),
        DiffDisplayKind::Metadata | DiffDisplayKind::HunkHeader => {
            let max_width = if wrap_for_reflow {
                DIFF_WRAP_SOURCE_MAX_WIDTH
            } else {
                target_width.map_or(MAX_LINE_LENGTH, |width| width.min(MAX_LINE_LENGTH))
            };
            let text = line.numbered_text(line_number_width);
            let text = if wrap_for_reflow && text.contains("lines omitted") && !text.contains("review full diff") {
                format!("{text} — review full diff")
            } else {
                text
            };
            let text = if display_width(&text) > max_width {
                truncate_with_ellipsis(&text, max_width, "...")
            } else {
                text
            };
            if let Some(style) = base_style {
                out.push_str(&style.render().to_string());
            }
            out.push_str(&text);
            out.push_str(&Reset.to_string());
            return out;
        }
    };
    if content.is_empty() {
        content = " ";
    }

    // Keep the complete rendered row within the measured width when one is
    // available; otherwise retain the generic preview cap. Inline TUI sinks
    // opt into reflow wrapping: emit the logical body whole so transcript
    // hanging-indent wrap can show the full line without ellipsis.
    let prefix_width: usize = if show_gutter { 4 + line_number_width } else { 1 };
    let max_width = if wrap_for_reflow {
        DIFF_WRAP_SOURCE_MAX_WIDTH
    } else {
        target_width.map_or(MAX_LINE_LENGTH, |width| width.min(MAX_LINE_LENGTH))
    };
    let content_width = if wrap_for_reflow {
        max_width
    } else {
        max_width.saturating_sub(prefix_width)
    };
    let content_owned;
    let mut truncated = false;
    let content: &str = if display_width(content) > content_width {
        content_owned = truncate_with_ellipsis(content, content_width, "...");
        truncated = true;
        &content_owned
    } else {
        content
    };

    let bg = base_style.and_then(|style| style.get_bg_color());
    // Unified gutter: the sign uses the shared accessible marker foreground;
    // numbers use a theme-aware muted foreground. The body uses the row tint
    // while changed spans receive the stronger word background.
    let marker_style = match marker {
        '+' => AnsiStyle::new().fg_color(Some(git_styles.addition_fg)).bg_color(bg),
        '-' => AnsiStyle::new().fg_color(Some(git_styles.deletion_fg)).bg_color(bg),
        _ => AnsiStyle::new().fg_color(Some(git_styles.gutter_fg)),
    };
    let gutter_style = AnsiStyle::new().fg_color(Some(git_styles.gutter_fg)).bg_color(bg);
    let reset = Reset;
    out.reserve(line.text.len() + 32);
    // Single gutter: `sign + number + │ + content`. The `│` keeps markdown
    // bullets (`- foo`) distinct from the diff marker (`+`/`-`).
    // Reset between spans so SGR state never leaks into the next. The outer
    // line style deliberately remains active for the message indent, making
    // the row tint continuous from the transcript prefix through the gutter.
    let line_no = match marker {
        '+' => line.new_line,
        '-' => line.old_line,
        _ => line.new_line.or(line.old_line),
    }
    .unwrap_or_default();
    let mut body_style = base_style.unwrap_or_else(|| AnsiStyle::new().bg_color(bg));
    if !show_gutter && body_style.get_fg_color().is_none() {
        let foreground = match marker {
            '+' => Some(git_styles.addition_fg),
            '-' => Some(git_styles.deletion_fg),
            _ => None,
        };
        if let Some(foreground) = foreground {
            body_style = body_style.fg_color(Some(foreground));
        }
    }

    if show_gutter {
        let _ = write!(out, "{}", marker_style.render());
        out.push_str(match marker {
            '+' => "+",
            '-' => "-",
            _ => " ",
        });
        let _ = write!(out, "{reset}");
        let _ = write!(out, "{}", gutter_style.render());
        let _ = write!(out, "{line_no:>line_number_width$} │ ");
        let _ = write!(out, "{reset}");
    } else {
        // One blank, styled cell keeps compact rows identifiable to the
        // inline transcript's diff reflow while dropping the visible gutter.
        let compact_marker = AnsiStyle::new().bg_color(bg);
        let _ = write!(out, "{compact_marker} ");
        let _ = write!(out, "{Reset}");
    }
    // Gutter is 1 (sign) + number_width + 3 (` │ `); compact rows use one
    // marker cell only.
    // Color-capable rows use a neutral foreground on the row tint; ANSI16
    // falls back to the bright body foreground. Skip chips on truncated rows — `changed` offsets are into the original
    // text and would highlight the wrong slice after ellipsis truncation.
    let word_ranges: &[(usize, usize)] = if matches!(marker, '+' | '-') && !content.is_empty() && !truncated {
        &line.changed
    } else {
        &[]
    };
    let fallback_fg = (!show_gutter).then(|| body_style.get_fg_color()).flatten();
    // Syntax foregrounds layer over the row tint; prose and unknown grammars
    // fall back to the solid tint so add/del semantics stay scannable.
    let highlighted = if matches!(marker, '+' | '-') && !truncated {
        highlight_diff_body_with_syntax(content, language, bg, word_ranges, word_bg, fallback_fg)
            .or_else(|| highlight_diff_content_with_foreground(content, bg, word_ranges, word_bg, fallback_fg))
    } else {
        highlight_diff_content_with_foreground(content, bg, word_ranges, word_bg, fallback_fg)
    };
    if let Some(highlighted) = highlighted {
        out.push_str(&highlighted);
    } else {
        let _ = write!(out, "{}", body_style.render());
        out.push_str(content);
        let _ = write!(out, "{reset}");
    }

    // Continue the base row tint through the unused cells on the right. Word
    // chips above remain stronger because padding is appended only after the
    // body has restored the base row state. Reflow-wrapped rows already exceed
    // the measured width, so skip padding rather than inventing overflow.
    if let Some(bg) = bg
        && let Some(target_width) = target_width
        && !wrap_for_reflow
    {
        let visible_width = prefix_width.saturating_add(display_width(content));
        let padding = target_width.saturating_sub(visible_width);
        if padding > 0 {
            let _ = write!(out, "{}{}{}", AnsiStyle::new().bg_color(Some(bg)).render(), " ".repeat(padding), reset);
        }
    }
    out
}
