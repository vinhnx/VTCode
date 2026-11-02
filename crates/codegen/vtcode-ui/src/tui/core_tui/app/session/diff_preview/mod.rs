//! Diff preview rendering for file edit approval
//!
//! Renders a syntax-highlighted diff preview with permission controls.

use anstyle::Style as AnsiStyle;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::collections::HashMap;
use vtcode_commons::ui_protocol::DiffPreviewMode as DiffLayoutMode;
use vtcode_diff::{
    DiffCell, DiffDisplayKind, DiffDisplayLine, DiffLayout, DiffRow, DiffRowKind, LayoutOptions,
    diff_display_line_number_width, diff_gutter_fits, diff_layout_width, diff_side_by_side_fits, layout_display_lines,
};

use super::Session;
use crate::tui::core_tui::app::types::{DiffPreviewMode, DiffPreviewState, TrustMode};
use crate::tui::core_tui::style::{ratatui_color_from_ansi, ratatui_style_from_ansi};
use crate::tui::utils::diff_styles::{
    DiffColorLevel, DiffColorPalette, DiffLineType, current_diff_render_style_context, style_content,
    style_file_header_new, style_file_header_old, style_gutter, style_hunk_header, style_line_bg, style_sign,
    style_word_bg,
};

pub(crate) fn render_diff_preview(session: &Session, frame: &mut Frame<'_>, area: Rect) {
    let Some(preview) = session.diff_preview_state() else {
        return;
    };

    // Clear the full overlay area so empty side-by-side cells cannot show
    // through to the transcript underneath.
    frame.render_widget(Clear, area);

    let palette = DiffColorPalette::default();
    let counts = preview.document.stats;

    let [header, content, controls] = area
        .try_layout(&Layout::vertical([Constraint::Length(2), Constraint::Min(5), Constraint::Length(4)]))
        .unwrap_or([Rect::ZERO; 3]);

    let configured_layout = session.core.appearance.diff_preview_mode;
    let layout_mode = effective_diff_layout(configured_layout, content.width);
    render_file_header(frame, header, preview, &palette, counts.additions, counts.deletions, layout_mode);
    // Remaining-row disclosure reuses the same laid-out rows as painting so a
    // frame never runs `layout_display_lines` twice for the same viewport.
    let remaining_rows = match layout_mode {
        DiffLayoutMode::SideBySide => render_diff_content_side_by_side(frame, content, preview),
        _ => render_diff_content(frame, content, preview),
    };
    render_controls(frame, controls, preview, remaining_rows);
}

fn effective_diff_layout(configured: DiffLayoutMode, width: u16) -> DiffLayoutMode {
    if configured == DiffLayoutMode::SideBySide && diff_side_by_side_fits(Some(width as usize)) {
        DiffLayoutMode::SideBySide
    } else {
        DiffLayoutMode::Inline
    }
}

fn should_show_inline_gutter(
    style_context: crate::tui::utils::diff_styles::DiffRenderStyleContext,
    width: usize,
    line_number_width: usize,
) -> bool {
    style_context.level() == DiffColorLevel::Ansi16 || diff_gutter_fits(width, line_number_width)
}

fn render_file_header(
    frame: &mut Frame<'_>,
    area: Rect,
    preview: &DiffPreviewState,
    palette: &DiffColorPalette,
    additions: usize,
    deletions: usize,
    layout_mode: DiffLayoutMode,
) {
    let header_style = Style::default().fg(ratatui_color_from_ansi(palette.header_fg));
    // Keep the (+N -N) counts visible: truncate the path to the width left
    // after the action label, counts, and optional layout badge.
    let badge_width = usize::from(layout_mode == DiffLayoutMode::SideBySide) * "  [side-by-side]".chars().count();
    let counts_width = format!(" (+{additions} -{deletions})").chars().count();
    let action_width = header_action_label(preview.mode).chars().count();
    let path_budget = (area.width as usize)
        .saturating_sub(action_width)
        .saturating_sub(counts_width)
        .saturating_sub(badge_width);
    let file_path = truncate_to_width(&preview.file_path, path_budget);
    let mut spans = vec![
        Span::styled(header_action_label(preview.mode), header_style),
        Span::styled(file_path, header_style),
        Span::styled(" (", header_style),
        Span::styled(format!("+{additions}"), Style::default().fg(ratatui_color_from_ansi(palette.added_fg))),
        Span::styled(" ", header_style),
        Span::styled(format!("-{deletions}"), Style::default().fg(ratatui_color_from_ansi(palette.removed_fg))),
        Span::styled(")", header_style),
    ];
    if layout_mode == DiffLayoutMode::SideBySide {
        spans.push(Span::styled("  [side-by-side]", Style::default().fg(Color::DarkGray)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn style_diff_metadata(text: &str, style_context: crate::tui::utils::diff_styles::DiffRenderStyleContext) -> Style {
    let trimmed = text.trim_start();
    if trimmed.starts_with("--- ") || trimmed.starts_with("*** Delete File:") {
        style_file_header_old(style_context)
    } else if trimmed.starts_with("+++ ") || trimmed.starts_with("*** Add File:") {
        style_file_header_new(style_context)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn render_diff_content(frame: &mut Frame<'_>, area: Rect, preview: &DiffPreviewState) -> usize {
    let style_context = current_diff_render_style_context();
    let width = area.width as usize;

    let mut lines: Vec<Line> = Vec::new();
    let max_display = area.height.saturating_sub(1) as usize;
    let display_lines = preview.display_lines.get(preview.scroll_offset..).unwrap_or_default();
    let line_number_width = diff_display_line_number_width(display_lines);
    // Without row backgrounds (ANSI16/no-color), the gutter is the only
    // reliable addition/deletion cue, so keep it even when the body is tight.
    let show_gutter = should_show_inline_gutter(style_context, width, line_number_width);
    let rows = layout_display_lines(
        display_lines,
        LayoutOptions {
            layout: DiffLayout::Unified,
            width: diff_layout_width(width, line_number_width, show_gutter),
            max_rows: 2_000,
            ..LayoutOptions::default()
        },
    );
    let total_rows = rows.len();
    let mut syntax_offsets = HashMap::new();
    let mut active_syntax_key = None;
    for row in &rows {
        if lines.len() >= max_display {
            break;
        }

        match row.kind {
            DiffRowKind::Metadata => {
                let text = row_text(row);
                lines.push(Line::from(Span::styled(text.clone(), style_diff_metadata(&text, style_context))));
            }
            DiffRowKind::HunkHeader => {
                let header_style = style_hunk_header(style_context);
                let text = row_text(row);
                lines.push(pad_line_to_width(
                    Line::from(Span::styled(text, header_style)).style(header_style),
                    width,
                    header_style,
                ));
            }
            DiffRowKind::Omission => {
                lines.push(Line::from(Span::styled(row_text(row), Style::default().fg(Color::DarkGray))));
            }
            DiffRowKind::Context | DiffRowKind::Addition | DiffRowKind::Deletion => {
                if let Some(cell) = row.left.as_ref() {
                    let display_line = display_line_from_cell(cell);
                    let (syntax, offset) =
                        syntax_for_cell(preview, cell, row.continuation, &mut active_syntax_key, &mut syntax_offsets);
                    lines.push(build_inline_diff_line(
                        &display_line,
                        syntax,
                        offset,
                        style_context,
                        width,
                        line_number_width,
                        show_gutter,
                    ));
                }
            }
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled("(no changes)", Style::default().fg(Color::DarkGray))));
    }

    frame.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::NONE)), area);
    total_rows.saturating_sub(max_display)
}

/// Test-only remaining-row count using a single laid-out pass (same options as
/// `render_diff_content`). Production remaining counts come from the paint path.
#[cfg(test)]
pub(crate) fn remaining_inline_rows_for_test(preview: &DiffPreviewState, content: Rect) -> usize {
    let style_context = current_diff_render_style_context();
    let width = content.width as usize;
    let display_lines = preview.display_lines.get(preview.scroll_offset..).unwrap_or_default();
    let line_number_width = diff_display_line_number_width(display_lines);
    let show_gutter = should_show_inline_gutter(style_context, width, line_number_width);
    let rows = layout_display_lines(
        display_lines,
        LayoutOptions {
            layout: DiffLayout::Unified,
            width: diff_layout_width(width, line_number_width, show_gutter),
            max_rows: 2_000,
            ..LayoutOptions::default()
        },
    );
    let visible = content.height.saturating_sub(1) as usize;
    rows.len().saturating_sub(visible)
}

fn render_diff_content_side_by_side(frame: &mut Frame<'_>, area: Rect, preview: &DiffPreviewState) -> usize {
    let style_context = current_diff_render_style_context();

    // Split into three independent areas so pane backgrounds physically
    // cannot bleed across the divider. Left gets floor((w-1)/2), right gets
    // the rest — together with the 1-col divider they always fill `area`.
    let total_w = area.width.max(3);
    let left_w = (total_w.saturating_sub(1)) / 2;
    let right_w = total_w.saturating_sub(left_w + 1);
    let left_area = Rect {
        x: area.x,
        y: area.y,
        width: left_w,
        height: area.height,
    };
    let divider_area = Rect {
        x: area.x + left_w,
        y: area.y,
        width: 1,
        height: area.height,
    };
    let right_area = Rect {
        x: area.x + left_w + 1,
        y: area.y,
        width: right_w,
        height: area.height,
    };

    // Column headers row (top 1 row of the content area).
    let header_h = 1u16.min(area.height);
    let header_left = Rect { height: header_h, ..left_area };
    let header_div = Rect { height: header_h, ..divider_area };
    let header_right = Rect { height: header_h, ..right_area };
    let body_left = Rect {
        y: area.y + header_h,
        height: area.height.saturating_sub(header_h),
        ..left_area
    };
    let body_div = Rect {
        y: area.y + header_h,
        height: area.height.saturating_sub(header_h),
        ..divider_area
    };
    let body_right = Rect {
        y: area.y + header_h,
        height: area.height.saturating_sub(header_h),
        ..right_area
    };

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("Old", Style::default().fg(Color::DarkGray)))),
        header_left,
    );
    frame
        .render_widget(Paragraph::new(Line::from(Span::styled("│", Style::default().fg(Color::DarkGray)))), header_div);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("New", Style::default().fg(Color::DarkGray)))),
        header_right,
    );

    let display_lines = side_by_side_display_lines(preview);
    let rows = layout_display_lines(
        display_lines,
        LayoutOptions {
            layout: DiffLayout::SideBySide,
            width: area.width as usize,
            max_rows: 2_000,
            min_side_by_side_width: vtcode_diff::DIFF_MIN_SIDE_BY_SIDE_WIDTH,
            ..LayoutOptions::default()
        },
    );
    let total_side_by_side_rows = rows.len();
    let max_display = body_left.height as usize;

    let mut left_lines: Vec<Line> = Vec::new();
    let mut right_lines: Vec<Line> = Vec::new();
    let mut full_width_lines: Vec<(usize, Line)> = Vec::new();
    let mut left_syntax_offsets = HashMap::new();
    let mut right_syntax_offsets = HashMap::new();
    let mut left_active_syntax_key = None;
    let mut right_active_syntax_key = None;

    for row in &rows {
        if left_lines.len() >= max_display {
            break;
        }

        if matches!(row.kind, DiffRowKind::Metadata | DiffRowKind::HunkHeader | DiffRowKind::Omission) {
            let line = if row.kind == DiffRowKind::HunkHeader {
                let header_style = style_hunk_header(style_context);
                pad_line_to_width(
                    Line::from(Span::styled(row_text(row), header_style)).style(header_style),
                    area.width as usize,
                    header_style,
                )
            } else {
                let text = row_text(row);
                Line::from(Span::styled(text.clone(), style_diff_metadata(&text, style_context)))
            };
            // Track full-width rows separately; both panes show a blank spacer.
            full_width_lines.push((left_lines.len(), line));
            // Line::default() has no spans — paints nothing, buffer stays default.
            left_lines.push(Line::default());
            right_lines.push(Line::default());
            continue;
        }

        // Left pane
        match &row.left {
            Some(cell) => {
                let line = display_line_from_cell(cell);
                let (syntax, offset) = syntax_for_cell(
                    preview,
                    cell,
                    row.continuation,
                    &mut left_active_syntax_key,
                    &mut left_syntax_offsets,
                );
                left_lines.push(build_side_pane_line(&line, syntax, offset, style_context, left_w as usize));
            }
            _ => {
                // Empty left: no spans — terminal default bg, no bleed.
                left_lines.push(Line::default());
            }
        }

        // Right pane
        match &row.right {
            Some(cell) => {
                let line = display_line_from_cell(cell);
                let (syntax, offset) = syntax_for_cell(
                    preview,
                    cell,
                    row.continuation,
                    &mut right_active_syntax_key,
                    &mut right_syntax_offsets,
                );
                right_lines.push(build_side_pane_line(&line, syntax, offset, style_context, right_w as usize));
            }
            _ => {
                right_lines.push(Line::default());
            }
        }
    }

    if left_lines.is_empty() {
        left_lines.push(Line::from(Span::styled("(no changes)", Style::default().fg(Color::DarkGray))));
    }

    // Render panes independently — no shared Line means no bg bleed.
    // The divider needs one row per body line; a single-Line Paragraph would
    // only paint the top row and leave the rest of the column blank.
    let divider_lines: Vec<Line> = (0..body_div.height)
        .map(|_| Line::from(Span::styled("│", Style::default().fg(Color::DarkGray))))
        .collect();
    frame.render_widget(Paragraph::new(left_lines), body_left);
    frame.render_widget(Paragraph::new(divider_lines), body_div);
    frame.render_widget(Paragraph::new(right_lines), body_right);

    // Overlay full-width bands (hunk headers) on top of both panes.
    for (row_index, line) in full_width_lines {
        let y = body_left.y + row_index as u16;
        if y >= body_left.y + body_left.height {
            continue;
        }
        let band = Rect { x: area.x, y, width: area.width, height: 1 };
        frame.render_widget(Paragraph::new(line), band);
    }
    total_side_by_side_rows.saturating_sub(max_display)
}

fn side_by_side_display_lines(preview: &DiffPreviewState) -> &[DiffDisplayLine] {
    let lines = &preview.display_lines;
    let mut start = preview.scroll_offset.min(lines.len());
    if start == 0 {
        return lines;
    }

    match lines.get(start).map(|line| line.kind) {
        Some(DiffDisplayKind::Deletion) => {
            while start > 0 && lines[start - 1].kind == DiffDisplayKind::Deletion {
                start -= 1;
            }
        }
        Some(DiffDisplayKind::Addition) => {
            while start > 0 && lines[start - 1].kind == DiffDisplayKind::Addition {
                start -= 1;
            }
            while start > 0 && lines[start - 1].kind == DiffDisplayKind::Deletion {
                start -= 1;
            }
        }
        _ => {}
    }
    &lines[start..]
}

fn row_text(row: &DiffRow) -> String {
    row.left
        .as_ref()
        .map(|cell| cell.segments.iter().map(|segment| segment.text.as_str()).collect())
        .unwrap_or_default()
}

fn display_line_from_cell(cell: &DiffCell) -> DiffDisplayLine {
    let kind = match cell.marker {
        '+' => DiffDisplayKind::Addition,
        '-' => DiffDisplayKind::Deletion,
        _ => DiffDisplayKind::Context,
    };
    let mut text = String::new();
    let mut changed = Vec::new();
    for segment in &cell.segments {
        let start = text.len();
        text.push_str(&segment.text);
        if segment.emphasized && start < text.len() {
            changed.push((start, text.len()));
        }
    }
    DiffDisplayLine {
        kind,
        old_line: cell.old_line,
        new_line: cell.new_line,
        text,
        changed,
    }
}

type DiffLineKey = (DiffDisplayKind, Option<u32>, Option<u32>);

fn syntax_for_cell<'a>(
    preview: &'a DiffPreviewState,
    cell: &DiffCell,
    continuation: bool,
    active_key: &mut Option<DiffLineKey>,
    offsets: &mut HashMap<DiffLineKey, usize>,
) -> (Option<&'a [(AnsiStyle, String)]>, usize) {
    let kind = match cell.marker {
        '+' => DiffDisplayKind::Addition,
        '-' => DiffDisplayKind::Deletion,
        ' ' if cell.old_line.is_some() || cell.new_line.is_some() => DiffDisplayKind::Context,
        _ => return (None, 0),
    };
    let key = if continuation {
        *active_key
    } else {
        Some((kind, cell.old_line, cell.new_line))
    };
    let Some(key) = key else {
        return (None, 0);
    };
    let offset = offsets.get(&key).copied().unwrap_or_default();
    let text_len = cell.segments.iter().map(|segment| segment.text.len()).sum::<usize>();
    offsets.insert(key, offset.saturating_add(text_len));
    *active_key = Some(key);
    (preview.syntax_segments(key.0, key.1, key.2), offset)
}

/// Build a single pane line with a complete row tint (gutter + content + pad).
fn build_side_pane_line(
    display_line: &DiffDisplayLine,
    syntax: Option<&[(AnsiStyle, String)]>,
    syntax_offset: usize,
    style_context: crate::tui::utils::diff_styles::DiffRenderStyleContext,
    width: usize,
) -> Line<'static> {
    let spans = build_side_pane_spans(display_line, syntax, syntax_offset, style_context, width);
    let line_bg = style_line_bg(line_type_for_kind(display_line.kind), style_context);
    pad_line_to_width(Line::from(spans).style(line_bg), width, line_bg)
}

fn line_type_for_kind(kind: DiffDisplayKind) -> DiffLineType {
    match kind {
        DiffDisplayKind::Addition => DiffLineType::Insert,
        DiffDisplayKind::Deletion => DiffLineType::Delete,
        _ => DiffLineType::Context,
    }
}

fn build_inline_diff_line(
    display_line: &DiffDisplayLine,
    syntax: Option<&[(AnsiStyle, String)]>,
    syntax_offset: usize,
    style_context: crate::tui::utils::diff_styles::DiffRenderStyleContext,
    width: usize,
    line_number_width: usize,
    show_gutter: bool,
) -> Line<'static> {
    let line_type = if display_line.kind == DiffDisplayKind::Context {
        DiffLineType::Context
    } else if display_line.kind == DiffDisplayKind::Addition {
        DiffLineType::Insert
    } else {
        DiffLineType::Delete
    };

    let gutter_style = style_gutter(line_type, style_context);
    let sign_style = style_sign(line_type, style_context);
    let line_bg = style_line_bg(line_type, style_context);
    let mut content_style = style_content(line_type, style_context);
    if !show_gutter && let Some(foreground) = style_sign(line_type, style_context).fg {
        // With the compact presentation there is no +/- marker left to carry
        // the semantic cue. Keep a contrast-safe add/delete foreground as the
        // fallback while preserving any syntax foregrounds on highlighted
        // spans.
        content_style = content_style.fg(foreground);
    }
    let word_bg = style_word_bg(line_type, style_context).bg;

    let mut spans = Vec::with_capacity(4);
    if show_gutter {
        let prefix = match line_type {
            DiffLineType::Insert => "+",
            DiffLineType::Delete => "-",
            DiffLineType::Context => " ",
        };

        // Single gutter: `sign + number + │`.
        let line_no = match display_line.kind {
            DiffDisplayKind::Deletion => display_line.old_line,
            DiffDisplayKind::Addition => display_line.new_line,
            _ => display_line.new_line.or(display_line.old_line),
        };
        let line_num_str = match line_no {
            Some(n) => format!("{n:>width$}", width = line_number_width),
            None => " ".repeat(line_number_width),
        };
        spans.extend([
            Span::styled(prefix.to_owned(), sign_style),
            Span::styled(line_num_str, gutter_style),
            Span::styled(" │ ".to_owned(), gutter_style),
        ]);
    }

    spans.extend(styled_content_spans(
        &display_line.text,
        &display_line.changed,
        syntax,
        syntax_offset,
        content_style,
        line_bg.bg,
        word_bg,
    ));

    pad_line_to_width(Line::from(spans).style(line_bg), width, line_bg)
}

/// Build one side-by-side pane: `sign + number + │ + content`, truncated to `width`.
fn build_side_pane_spans(
    display_line: &DiffDisplayLine,
    syntax: Option<&[(AnsiStyle, String)]>,
    syntax_offset: usize,
    style_context: crate::tui::utils::diff_styles::DiffRenderStyleContext,
    width: usize,
) -> Vec<Span<'static>> {
    let line_type = if display_line.kind == DiffDisplayKind::Context {
        DiffLineType::Context
    } else if display_line.kind == DiffDisplayKind::Addition {
        DiffLineType::Insert
    } else {
        DiffLineType::Delete
    };

    let gutter_style = style_gutter(line_type, style_context);
    let sign_style = style_sign(line_type, style_context);
    let content_style = style_content(line_type, style_context);
    let line_bg = style_line_bg(line_type, style_context);
    let word_bg = style_word_bg(line_type, style_context).bg;

    let prefix = match line_type {
        DiffLineType::Insert => "+",
        DiffLineType::Delete => "-",
        DiffLineType::Context => " ",
    };
    let line_no = match display_line.kind {
        DiffDisplayKind::Deletion => display_line.old_line,
        DiffDisplayKind::Addition => display_line.new_line,
        _ => display_line.new_line.or(display_line.old_line),
    };
    let line_num_str = match line_no {
        Some(n) => format!("{n:>3}"),
        None => "   ".to_owned(),
    };

    let mut spans = vec![
        Span::styled(prefix.to_owned(), sign_style),
        Span::styled(line_num_str.clone(), gutter_style),
        Span::styled("│".to_owned(), gutter_style),
    ];
    // Gutter width grows with large line numbers (`{n:>3}` widens past 3
    // digits), so measure it instead of assuming 5 cells.
    let used = 1usize
        .saturating_add(unicode_width::UnicodeWidthStr::width(line_num_str.as_str()))
        .saturating_add(1);

    if used < width {
        let body = truncate_to_width(&display_line.text, width - used);
        let changed = if body.len() < display_line.text.len() {
            &[][..]
        } else {
            &display_line.changed[..]
        };
        spans.extend(styled_content_spans(&body, changed, syntax, syntax_offset, content_style, line_bg.bg, word_bg));
    }

    spans
}

fn styled_content_spans(
    text: &str,
    changed: &[(usize, usize)],
    syntax: Option<&[(AnsiStyle, String)]>,
    syntax_offset: usize,
    fallback: Style,
    line_bg: Option<Color>,
    word_bg: Option<Color>,
) -> Vec<Span<'static>> {
    if text.is_empty() {
        return vec![Span::styled(String::new(), fallback)];
    }

    let mut boundaries = vec![0, text.len()];
    let mut syntax_intervals = Vec::new();
    let mut syntax_cursor = 0usize;
    if let Some(syntax) = syntax {
        for (style, segment) in syntax {
            let start = syntax_cursor;
            let end = syntax_cursor.saturating_add(segment.len());
            syntax_cursor = end;
            if start < syntax_offset.saturating_add(text.len()) && end > syntax_offset {
                boundaries.push(start.max(syntax_offset).saturating_sub(syntax_offset));
                boundaries.push(end.min(syntax_offset.saturating_add(text.len())).saturating_sub(syntax_offset));
                syntax_intervals.push((start, end, *style));
            }
        }
    }
    for &(start, end) in changed {
        if start < text.len() && end > 0 {
            boundaries.push(start.min(text.len()));
            boundaries.push(end.min(text.len()));
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    boundaries
        .windows(2)
        .filter_map(|pair| {
            let start = pair[0];
            let end = pair[1];
            if start >= end {
                return None;
            }
            let global_start = syntax_offset.saturating_add(start);
            let global_end = syntax_offset.saturating_add(end);
            let mut style = syntax_intervals
                .iter()
                .find(|(interval_start, interval_end, _)| {
                    global_start >= *interval_start && global_end <= *interval_end
                })
                .map_or(fallback, |(_, _, syntax_style)| {
                    let mut resolved = ratatui_style_from_ansi(*syntax_style);
                    resolved.bg = line_bg.or(fallback.bg);
                    resolved.remove_modifier(Modifier::DIM)
                });
            if style.bg.is_none() {
                style.bg = line_bg.or(fallback.bg);
            }
            if style.fg.is_none() {
                if let Some(fallback_fg) = fallback.fg {
                    style = style.fg(fallback_fg);
                }
            }
            if changed
                .iter()
                .any(|&(changed_start, changed_end)| start < changed_end && end > changed_start)
            {
                style.bg = word_bg.or(line_bg).or(fallback.bg);
                style = style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            }
            Some(Span::styled(text[start..end].to_owned(), style))
        })
        .collect()
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
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

/// Pad a diff row with tinted spaces so the background spans the full width.
///
/// `Paragraph` only paints behind actual spans; without padding the tint
/// ends at the last character and short rows look striped.
fn pad_line_to_width(mut line: Line<'static>, width: usize, bg_style: Style) -> Line<'static> {
    if width == 0 {
        return line;
    }
    let line_width: usize = line
        .spans
        .iter()
        .map(|span| {
            span.content
                .chars()
                .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1))
                .sum::<usize>()
        })
        .sum();
    let padding = width.saturating_sub(line_width);
    if padding == 0 {
        return line;
    }
    let pad_bg = bg_style.bg.map(|bg| Style::default().bg(bg)).unwrap_or_default();
    line.spans.push(Span::styled(" ".repeat(padding), pad_bg));
    line
}

fn header_action_label(mode: DiffPreviewMode) -> &'static str {
    match mode {
        DiffPreviewMode::EditApproval => "← Edit ",
        DiffPreviewMode::FileConflict => "← Conflict ",
        DiffPreviewMode::ReadonlyReview => "← Review ",
    }
}

fn render_controls(frame: &mut Frame<'_>, area: Rect, preview: &DiffPreviewState, remaining_rows: usize) {
    let mut lines = control_lines(preview);
    if remaining_rows > 0 {
        lines.push(Line::from(Span::styled(format!(" +{remaining_rows} more"), Style::default().fg(Color::DarkGray))));
    }

    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        area,
    );
}

fn control_lines(preview: &DiffPreviewState) -> Vec<Line<'static>> {
    let action_key_style = |color: Color| -> Style { Style::default().fg(color).add_modifier(Modifier::BOLD) };

    let key_green = Color::LightGreen;
    let key_red = Color::LightRed;
    let key_cyan = Color::LightCyan;
    let key_yellow = Color::Yellow;
    let muted = Color::Gray;

    match preview.mode {
        DiffPreviewMode::EditApproval => {
            let trust = match preview.trust_mode {
                TrustMode::Once => "Once",
                TrustMode::Session => "Session",
                TrustMode::Always => "Always",
                TrustMode::AutoTrust => "Auto",
            };

            vec![
                Line::from(vec![
                    Span::styled("Enter", action_key_style(key_green)),
                    Span::raw(" Apply  "),
                    Span::styled("Esc", action_key_style(key_red)),
                    Span::raw(" Reject  "),
                    Span::styled("Tab", action_key_style(key_yellow)),
                    Span::raw("/"),
                    Span::styled("S-Tab", action_key_style(key_yellow)),
                    Span::raw(" Nav"),
                ]),
                Line::from(vec![
                    Span::styled("1", action_key_style(key_cyan)),
                    Span::raw("-Once "),
                    Span::styled("2", action_key_style(key_cyan)),
                    Span::raw("-Sess "),
                    Span::styled("3", action_key_style(key_cyan)),
                    Span::raw("-Always "),
                    Span::styled("4", action_key_style(key_cyan)),
                    Span::raw("-Auto "),
                    Span::styled(format!("[{trust}]"), Style::default().fg(muted).add_modifier(Modifier::BOLD)),
                ]),
            ]
        }
        DiffPreviewMode::FileConflict => vec![
            Line::from(vec![
                Span::styled("Enter", action_key_style(key_green)),
                Span::raw(" Proceed  "),
                Span::styled("r", action_key_style(key_cyan)),
                Span::raw(" Reload  "),
                Span::styled("Esc", action_key_style(key_red)),
                Span::raw(" Abort"),
            ]),
            Line::from(vec![
                Span::styled("Tab", action_key_style(key_yellow)),
                Span::raw("/"),
                Span::styled("S-Tab", action_key_style(key_yellow)),
                Span::raw(" Nav"),
            ]),
        ],
        DiffPreviewMode::ReadonlyReview => vec![
            Line::from(vec![
                Span::styled("Enter", action_key_style(key_green)),
                Span::raw(" Back  "),
                Span::styled("Esc", action_key_style(key_red)),
                Span::raw(" Back"),
            ]),
            Line::from(vec![
                Span::styled("Tab", action_key_style(key_yellow)),
                Span::raw("/"),
                Span::styled("S-Tab", action_key_style(key_yellow)),
                Span::raw(" Nav"),
            ]),
        ],
    }
}

#[cfg(test)]
mod tests;
