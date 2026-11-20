//! Responsive Markdown table layout for the terminal.
//!
//! The width-aware grid and key/value fallback are adapted from OpenAI Codex's
//! `codex-rs/tui/src/markdown_render.rs` and
//! `codex-rs/tui/src/markdown_render/table_key_value.rs`, researched through
//! [DeepWiki](https://deepwiki.com/openai/codex/4.1.2-chatwidget-and-conversation-display).

use super::MarkdownLine;
use crate::tui::core_tui::session::text_utils::{char_display_width, display_width};
use anstyle::Style;
use pulldown_cmark::Alignment;
use unicode_segmentation::UnicodeSegmentation;
use vtcode_commons::terminal_capabilities::ambiguous_width_wide;

const TABLE_COLUMN_GAP: usize = 2;
const TABLE_CELL_PADDING: usize = 1;
const TABLE_HEADER_SEPARATOR_CHAR: char = '━';
const TABLE_BODY_SEPARATOR_CHAR: char = '─';
const MIN_TABLE_CELL_WIDTH: usize = 3;

/// Separator glyph for a table rule.
///
/// Box-drawing characters are East-Asian-ambiguous in UAX #11, so a terminal
/// that renders the set as double width breaks every rule row; ASCII glyphs
/// (`=`, `-`) are the only safe chrome there.
fn rule_separator_char(header: bool) -> char {
    if ambiguous_width_wide() {
        return if header { '=' } else { '-' };
    }
    if header {
        TABLE_HEADER_SEPARATOR_CHAR
    } else {
        TABLE_BODY_SEPARATOR_CHAR
    }
}

const RECORD_FIELD_LEADING_PADDING: usize = 1;
const RECORD_FIELD_GAP: usize = 2;
const RECORD_STACKED_VALUE_INDENT: usize = 2;
const MIN_RECORD_VALUE_WIDTH: usize = 3;
const MIN_SCANNABLE_NARRATIVE_WIDTH: usize = 12;
const CRAMPED_EXPANSIVE_CELL_LINES: usize = 4;
const CATASTROPHIC_NARRATIVE_CELL_LINES: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TableColumnKind {
    Narrative,
    TokenHeavy,
    Compact,
}

#[derive(Clone, Copy, Debug)]
struct TableColumnMetrics {
    max_width: usize,
    header_token_width: usize,
    body_token_width: usize,
    kind: TableColumnKind,
}

#[derive(Debug, Default)]
pub(crate) struct TableBuffer {
    pub(crate) headers: Vec<MarkdownLine>,
    pub(crate) rows: Vec<Vec<MarkdownLine>>,
    pub(crate) current_row: Vec<MarkdownLine>,
    pub(crate) in_head: bool,
    pub(crate) alignments: Vec<Alignment>,
}

pub(crate) fn render_table(table: &TableBuffer, base_style: Style, max_width: Option<usize>) -> Vec<MarkdownLine> {
    let column_count = table
        .headers
        .len()
        .max(table.rows.iter().map(Vec::len).max().unwrap_or(0))
        .max(table.alignments.len());
    if column_count == 0 {
        return Vec::new();
    }

    let has_header = !table.headers.is_empty();
    let mut headers = table.headers.clone();
    headers.resize(column_count, MarkdownLine::default());
    let rows = table
        .rows
        .iter()
        .map(|row| normalize_row(row, column_count))
        .collect::<Vec<_>>();
    let alignments = normalize_alignments(&table.alignments, column_count);
    let metrics = collect_table_column_metrics(&headers, &rows, column_count);

    // Headerless tables have no stable labels for a record fallback. Preserve
    // their grid shape and scale content widths when a terminal constrains it.
    if !has_header {
        let mut widths = metrics.iter().map(|column| column.max_width.max(1)).collect::<Vec<_>>();
        if let Some(width) = max_width {
            let minimum_grid_width = table_overhead(column_count).saturating_add(column_count);
            if minimum_grid_width > width {
                return render_unlabelled_fallback(&rows, width, base_style);
            }
            scale_columns_to_fit(&mut widths, width);
        }
        return render_table_grid(&headers, &rows, &widths, &alignments, false, base_style);
    }

    let widths = compute_column_widths(&metrics, max_width);
    let separator_style = base_style.dimmed();
    if widths
        .as_ref()
        .is_none_or(|widths| should_render_records(&rows, widths, &metrics))
    {
        return render_table_records(&headers, &rows, max_width, base_style, separator_style);
    }

    render_table_grid(&headers, &rows, widths.as_deref().unwrap_or_default(), &alignments, true, base_style)
}

fn normalize_row(row: &[MarkdownLine], column_count: usize) -> Vec<MarkdownLine> {
    let mut normalized = row.iter().take(column_count).cloned().collect::<Vec<_>>();
    normalized.resize(column_count, MarkdownLine::default());
    normalized
}

fn normalize_alignments(alignments: &[Alignment], column_count: usize) -> Vec<Alignment> {
    (0..column_count)
        .map(|column| alignments.get(column).cloned().unwrap_or(Alignment::None))
        .collect()
}

fn table_overhead(column_count: usize) -> usize {
    column_count
        .saturating_mul(TABLE_CELL_PADDING * 2)
        .saturating_add(column_count.saturating_sub(1).saturating_mul(TABLE_COLUMN_GAP))
}

/// Total rendered width including one-cell padding and gaps between columns.
fn total_table_width(column_widths: &[usize]) -> usize {
    column_widths
        .iter()
        .sum::<usize>()
        .saturating_add(table_overhead(column_widths.len()))
}

/// Proportionally scale a headerless table while retaining its column layout.
fn scale_columns_to_fit(col_widths: &mut [usize], max_width: usize) {
    if col_widths.is_empty() {
        return;
    }

    let current = total_table_width(col_widths);
    if current <= max_width {
        return;
    }

    let available = max_width.saturating_sub(table_overhead(col_widths.len()));
    if available < col_widths.len() {
        col_widths.fill(1);
        return;
    }

    let total_content = col_widths.iter().sum::<usize>();
    if total_content == 0 {
        let each = available / col_widths.len();
        col_widths.fill(each.max(1));
        return;
    }

    for width in col_widths.iter_mut() {
        let scaled = (*width as u128 * available as u128 / total_content as u128) as usize;
        *width = scaled.max(1);
    }

    while total_table_width(col_widths) > max_width {
        let Some((index, _)) = col_widths
            .iter()
            .enumerate()
            .filter(|(_, width)| **width > 1)
            .max_by_key(|(_, width)| **width)
        else {
            break;
        };
        col_widths[index] -= 1;
    }
}

fn collect_table_column_metrics(
    headers: &[MarkdownLine],
    rows: &[Vec<MarkdownLine>],
    column_count: usize,
) -> Vec<TableColumnMetrics> {
    (0..column_count)
        .map(|column| {
            let header_text = line_text(&headers[column]);
            let header_token_width = longest_token_width(&header_text);
            let mut max_width = headers[column].width();
            let mut body_token_width: usize = 0;
            let mut body_token_count: usize = 0;
            let mut long_body_token_count: usize = 0;
            let mut total_words: usize = 0;
            let mut total_cells: usize = 0;
            let mut total_cell_width: usize = 0;

            for row in rows {
                let cell = &row[column];
                max_width = max_width.max(cell.width());
                let text = line_text(cell);
                let mut word_count = 0;
                for token in text.split_whitespace() {
                    let token_width = display_width(token);
                    body_token_width = body_token_width.max(token_width);
                    long_body_token_count += usize::from(token_width >= 20);
                    body_token_count += 1;
                    word_count += 1;
                }
                if word_count > 0 {
                    total_words += word_count;
                    total_cells += 1;
                    total_cell_width += display_width(text.as_str());
                }
            }

            let average_words = if total_cells == 0 {
                header_text.split_whitespace().count() as f64
            } else {
                total_words as f64 / total_cells as f64
            };
            let average_width = if total_cells == 0 {
                display_width(header_text.as_str()) as f64
            } else {
                total_cell_width as f64 / total_cells as f64
            };
            let kind = if long_body_token_count > 0
                && long_body_token_count >= body_token_count.saturating_sub(long_body_token_count)
            {
                TableColumnKind::TokenHeavy
            } else if average_words >= 4.0 || average_width >= 28.0 {
                TableColumnKind::Narrative
            } else {
                TableColumnKind::Compact
            };

            TableColumnMetrics {
                max_width,
                header_token_width,
                body_token_width,
                kind,
            }
        })
        .collect()
}

fn compute_column_widths(metrics: &[TableColumnMetrics], max_width: Option<usize>) -> Option<Vec<usize>> {
    let mut widths = metrics
        .iter()
        .map(|column| column.max_width.max(MIN_TABLE_CELL_WIDTH))
        .collect::<Vec<_>>();
    let Some(max_width) = max_width else {
        return Some(widths);
    };

    let available = max_width.saturating_sub(table_overhead(metrics.len()));
    let minimum_total = metrics.len().saturating_mul(MIN_TABLE_CELL_WIDTH);
    if available < minimum_total {
        return None;
    }

    let mut floors = metrics.iter().map(preferred_column_floor).collect::<Vec<_>>();
    let floor_total = floors.iter().sum::<usize>();
    if floor_total > available {
        let minimums = vec![MIN_TABLE_CELL_WIDTH; floors.len()];
        shrink_columns(&mut floors, &minimums, metrics, floor_total - available);
    }

    let total_width = widths.iter().sum::<usize>();
    if total_width > available {
        let remaining = shrink_columns(&mut widths, &floors, metrics, total_width - available);
        if remaining > 0 {
            return None;
        }
    }

    Some(widths)
}

fn preferred_column_floor(metrics: &TableColumnMetrics) -> usize {
    let target = match metrics.kind {
        TableColumnKind::Narrative | TableColumnKind::TokenHeavy => 16,
        TableColumnKind::Compact => metrics.header_token_width.max(metrics.body_token_width.min(16)),
    };
    target
        .max(MIN_TABLE_CELL_WIDTH)
        .min(metrics.max_width.max(MIN_TABLE_CELL_WIDTH))
}

/// Shrink columns by content class so token-heavy values yield width before
/// narrative text, while compact values remain scannable for as long as possible.
fn shrink_columns(widths: &mut [usize], floors: &[usize], metrics: &[TableColumnMetrics], mut amount: usize) -> usize {
    for kind in [
        TableColumnKind::TokenHeavy,
        TableColumnKind::Narrative,
        TableColumnKind::Compact,
    ] {
        let slack_total = widths
            .iter()
            .enumerate()
            .filter(|(index, _)| metrics[*index].kind == kind)
            .map(|(index, width)| width.saturating_sub(floors[index]))
            .sum::<usize>();
        let to_remove = amount.min(slack_total);
        if to_remove == 0 {
            continue;
        }

        let mut low = 0;
        let mut high = widths
            .iter()
            .enumerate()
            .filter(|(index, _)| metrics[*index].kind == kind)
            .map(|(index, width)| width.saturating_sub(floors[index]))
            .max()
            .unwrap_or(0);
        while low < high {
            let cap = low + (high - low) / 2;
            let removed = widths
                .iter()
                .enumerate()
                .filter(|(index, _)| metrics[*index].kind == kind)
                .map(|(index, width)| width.saturating_sub(floors[index]).saturating_sub(cap))
                .sum::<usize>();
            if removed > to_remove {
                low = cap + 1;
            } else {
                high = cap;
            }
        }

        let cap = low;
        let mut removed = 0;
        for (index, width) in widths.iter_mut().enumerate() {
            if metrics[index].kind != kind {
                continue;
            }
            let reduction = width.saturating_sub(floors[index]).saturating_sub(cap);
            *width -= reduction;
            removed += reduction;
        }

        let mut remainder = to_remove - removed;
        for (index, width) in widths.iter_mut().enumerate() {
            if remainder == 0 {
                break;
            }
            if metrics[index].kind == kind && width.saturating_sub(floors[index]) == cap {
                *width -= 1;
                remainder -= 1;
            }
        }

        amount -= to_remove;
        if amount == 0 {
            break;
        }
    }
    amount
}

fn should_render_records(rows: &[Vec<MarkdownLine>], column_widths: &[usize], metrics: &[TableColumnMetrics]) -> bool {
    if rows.is_empty() {
        return false;
    }

    let affected_rows = rows
        .iter()
        .filter(|row| {
            let mut row_is_affected = false;
            let mut cramped_expansive_cells = 0;
            for (column, cell) in row.iter().enumerate() {
                let width = column_widths[column].max(1);
                let wrapped_lines = wrap_markdown_line(cell, width).len();
                let token_width = longest_token_width(&line_text(cell));
                let fragmented = matches!(metrics[column].kind, TableColumnKind::Compact | TableColumnKind::TokenHeavy)
                    && token_width > width;
                let expansive =
                    metrics[column].kind != TableColumnKind::Compact && wrapped_lines >= CRAMPED_EXPANSIVE_CELL_LINES;
                let catastrophic = metrics[column].kind == TableColumnKind::Narrative
                    && width < MIN_SCANNABLE_NARRATIVE_WIDTH
                    && wrapped_lines >= CATASTROPHIC_NARRATIVE_CELL_LINES;
                row_is_affected |= fragmented || expansive || catastrophic;
                if expansive {
                    cramped_expansive_cells += 1;
                }
            }
            row_is_affected || cramped_expansive_cells >= 2
        })
        .count();
    let threshold = if rows.len() == 1 {
        1
    } else {
        2.max(rows.len().div_ceil(3))
    };
    affected_rows >= threshold
}

fn render_table_grid(
    headers: &[MarkdownLine],
    rows: &[Vec<MarkdownLine>],
    column_widths: &[usize],
    alignments: &[Alignment],
    has_header: bool,
    base_style: Style,
) -> Vec<MarkdownLine> {
    let mut lines = Vec::with_capacity(rows.len() * 2 + usize::from(has_header) * 2);
    let separator_style = base_style.dimmed();

    if has_header {
        lines.extend(render_table_row(headers, column_widths, alignments, base_style, true));
        lines.push(render_table_separator(column_widths, rule_separator_char(true), separator_style));
    }

    for (row_index, row) in rows.iter().enumerate() {
        lines.extend(render_table_row(row, column_widths, alignments, base_style, false));
        if row_index + 1 < rows.len() {
            lines.push(render_table_separator(column_widths, rule_separator_char(false), separator_style));
        }
    }

    lines
}

fn render_table_separator(column_widths: &[usize], separator: char, style: Style) -> MarkdownLine {
    let mut line = MarkdownLine::default();
    for (column, width) in column_widths.iter().enumerate() {
        if column > 0 {
            line.push_segment(style, &" ".repeat(TABLE_COLUMN_GAP));
        }
        line.push_segment(style, &separator.to_string().repeat(width + TABLE_CELL_PADDING * 2));
    }
    line
}

fn render_table_row(
    cells: &[MarkdownLine],
    column_widths: &[usize],
    alignments: &[Alignment],
    base_style: Style,
    bold: bool,
) -> Vec<MarkdownLine> {
    if column_widths.is_empty() {
        return vec![MarkdownLine::default()];
    }

    let wrapped_cells = column_widths
        .iter()
        .enumerate()
        .map(|(column, width)| {
            cells
                .get(column)
                .map(|cell| wrap_markdown_line(cell, (*width).max(1)))
                .unwrap_or_else(|| vec![MarkdownLine::default()])
        })
        .collect::<Vec<_>>();
    let row_height = wrapped_cells.iter().map(Vec::len).max().unwrap_or(1);
    let mut lines = Vec::with_capacity(row_height);

    for line_index in 0..row_height {
        let mut line = MarkdownLine::default();
        for (column, width) in column_widths.iter().enumerate() {
            push_spaces(&mut line, base_style, TABLE_CELL_PADDING);
            if let Some(cell_line) = wrapped_cells[column].get(line_index) {
                let content_width = cell_line.width();
                let remaining = width.saturating_sub(content_width);
                let alignment = alignments.get(column).cloned().unwrap_or(Alignment::None);
                let (left_padding, right_padding) = match alignment {
                    Alignment::Left | Alignment::None => (0, remaining),
                    Alignment::Center => (remaining / 2, remaining - remaining / 2),
                    Alignment::Right => (remaining, 0),
                };
                push_spaces(&mut line, base_style, left_padding);
                append_cell_segments(&mut line, cell_line, bold);
                push_spaces(&mut line, base_style, right_padding);
            } else {
                push_spaces(&mut line, base_style, *width);
            }
            push_spaces(&mut line, base_style, TABLE_CELL_PADDING);
            if column + 1 < column_widths.len() {
                push_spaces(&mut line, base_style.dimmed(), TABLE_COLUMN_GAP);
            }
        }
        trim_trailing_whitespace(&mut line);
        lines.push(line);
    }

    lines
}

fn render_table_records(
    headers: &[MarkdownLine],
    rows: &[Vec<MarkdownLine>],
    max_width: Option<usize>,
    base_style: Style,
    separator_style: Style,
) -> Vec<MarkdownLine> {
    let label_width = headers.iter().map(MarkdownLine::width).max().unwrap_or(0);
    let natural_value_width = rows
        .iter()
        .flat_map(|row| row.iter().map(MarkdownLine::width))
        .max()
        .unwrap_or(0);
    let natural_width = RECORD_FIELD_LEADING_PADDING
        .saturating_add(label_width)
        .saturating_add(RECORD_FIELD_GAP)
        .saturating_add(natural_value_width)
        .max(1);
    let record_width = max_width.unwrap_or(natural_width).max(1);
    let aligned =
        record_width >= RECORD_FIELD_LEADING_PADDING + label_width + RECORD_FIELD_GAP + MIN_RECORD_VALUE_WIDTH;

    if rows.is_empty() {
        return headers
            .iter()
            .flat_map(|header| {
                if aligned {
                    render_aligned_field(header, None, label_width, record_width, base_style)
                } else {
                    render_stacked_field(header, None, record_width, base_style)
                }
            })
            .collect();
    }

    let mut lines = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        for (column, header) in headers.iter().enumerate() {
            let value = row.get(column).filter(|cell| !cell.is_empty());
            let field_lines = if aligned {
                render_aligned_field(header, value, label_width, record_width, base_style)
            } else {
                render_stacked_field(header, value, record_width, base_style)
            };
            lines.extend(field_lines);
        }
        if row_index + 1 < rows.len() {
            lines
                .push(single_style_line(separator_style, &rule_separator_char(false).to_string().repeat(record_width)));
        }
    }
    lines
}

fn render_aligned_field(
    header: &MarkdownLine,
    value: Option<&MarkdownLine>,
    label_width: usize,
    record_width: usize,
    base_style: Style,
) -> Vec<MarkdownLine> {
    let value_indent = RECORD_FIELD_LEADING_PADDING + label_width + RECORD_FIELD_GAP;
    let value_width = record_width.saturating_sub(value_indent).max(1);
    let value_lines = value
        .map(|cell| wrap_markdown_line(cell, value_width))
        .unwrap_or_else(|| vec![MarkdownLine::default()]);
    let has_value = value.is_some();
    let line_count = if has_value { value_lines.len() } else { 1 };
    let mut lines = Vec::with_capacity(line_count);

    for line_index in 0..line_count {
        let mut line = MarkdownLine::default();
        if line_index == 0 {
            push_spaces(&mut line, base_style, RECORD_FIELD_LEADING_PADDING);
            append_cell_segments(&mut line, header, true);
            push_spaces(&mut line, base_style, label_width.saturating_sub(header.width()));
            if has_value {
                push_spaces(&mut line, base_style, RECORD_FIELD_GAP);
            }
        } else {
            push_spaces(&mut line, base_style, value_indent);
        }
        if has_value {
            if let Some(value_line) = value_lines.get(line_index) {
                append_cell_segments(&mut line, value_line, false);
            }
        }
        lines.push(line);
    }
    lines
}

fn render_stacked_field(
    header: &MarkdownLine,
    value: Option<&MarkdownLine>,
    record_width: usize,
    base_style: Style,
) -> Vec<MarkdownLine> {
    let label_prefix = usize::from(record_width > 1);
    let label_width = record_width.saturating_sub(label_prefix).max(1);
    let label_lines = wrap_markdown_line(header, label_width);
    let value_indent = if record_width > 1 {
        RECORD_STACKED_VALUE_INDENT.min(record_width - 1)
    } else {
        0
    };
    let value_width = record_width.saturating_sub(value_indent).max(1);
    let mut lines = Vec::with_capacity(label_lines.len() + value.map_or(0, |cell| cell.width()));

    for (index, label_line) in label_lines.iter().enumerate() {
        let mut line = MarkdownLine::default();
        push_spaces(&mut line, base_style, if index == 0 { label_prefix } else { value_indent });
        append_cell_segments(&mut line, label_line, true);
        lines.push(line);
    }

    if let Some(value) = value {
        for value_line in wrap_markdown_line(value, value_width) {
            let mut line = MarkdownLine::default();
            push_spaces(&mut line, base_style, value_indent);
            append_cell_segments(&mut line, &value_line, false);
            lines.push(line);
        }
    }

    lines
}

fn render_unlabelled_fallback(rows: &[Vec<MarkdownLine>], max_width: usize, base_style: Style) -> Vec<MarkdownLine> {
    let wrap_width = max_width.max(1);
    let mut lines = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        for cell in row {
            lines.extend(wrap_markdown_line(cell, wrap_width));
        }
        if row_index + 1 < rows.len() {
            lines.push(single_style_line(
                base_style.dimmed(),
                &rule_separator_char(false).to_string().repeat(wrap_width),
            ));
        }
    }
    lines
}

fn append_cell_segments(line: &mut MarkdownLine, cell: &MarkdownLine, bold: bool) {
    for segment in &cell.segments {
        let style = if bold { segment.style.bold() } else { segment.style };
        line.push_segment_with_link(style, &segment.text, segment.link_target.clone());
    }
}

fn push_spaces(line: &mut MarkdownLine, style: Style, count: usize) {
    if count > 0 {
        line.push_segment(style, &" ".repeat(count));
    }
}

fn single_style_line(style: Style, text: &str) -> MarkdownLine {
    let mut line = MarkdownLine::default();
    line.push_segment(style, text);
    line
}

fn line_text(line: &MarkdownLine) -> String {
    line.segments.iter().map(|segment| segment.text.as_str()).collect()
}

fn longest_token_width(text: &str) -> usize {
    text.split_whitespace().map(display_width).max().unwrap_or(0)
}

fn trim_trailing_whitespace(line: &mut MarkdownLine) {
    while let Some(last) = line.segments.last_mut() {
        let trimmed = last.text.trim_end_matches(char::is_whitespace);
        if trimmed.len() == last.text.len() {
            break;
        }
        if trimmed.is_empty() {
            line.segments.pop();
        } else {
            last.text.truncate(trimmed.len());
            break;
        }
    }
}

fn wrap_markdown_line(line: &MarkdownLine, max_width: usize) -> Vec<MarkdownLine> {
    if max_width == 0 {
        return vec![MarkdownLine::default()];
    }
    if line.width() <= max_width {
        return vec![line.clone()];
    }

    let mut rows = Vec::new();
    let mut current = MarkdownLine::default();
    let mut current_width = 0;

    let flush = |current: &mut MarkdownLine, rows: &mut Vec<MarkdownLine>, current_width: &mut usize| {
        trim_trailing_whitespace(current);
        rows.push(std::mem::take(current));
        *current_width = 0;
    };

    for segment in &line.segments {
        let style = segment.style;
        let link_target = segment.link_target.clone();
        for token in segment.text.split_word_bounds() {
            if token.is_empty() {
                continue;
            }

            let token_width = display_width(token);
            if token_width == 0 {
                current.push_segment_with_link(style, token, link_target.clone());
                continue;
            }

            let is_whitespace = token.chars().all(char::is_whitespace);
            let has_content = current_width > 0;
            if is_whitespace && !rows.is_empty() && !has_content {
                continue;
            }
            if current_width + token_width <= max_width {
                current.push_segment_with_link(style, token, link_target.clone());
                current_width += token_width;
                continue;
            }
            if is_whitespace {
                if has_content {
                    flush(&mut current, &mut rows, &mut current_width);
                }
                continue;
            }
            if token_width <= max_width {
                if has_content {
                    flush(&mut current, &mut rows, &mut current_width);
                }
                current.push_segment_with_link(style, token, link_target.clone());
                current_width += token_width;
                continue;
            }

            for grapheme in UnicodeSegmentation::graphemes(token, true) {
                if grapheme.is_empty() {
                    continue;
                }
                let grapheme_width = display_width(grapheme);
                if grapheme_width == 0 {
                    current.push_segment_with_link(style, grapheme, link_target.clone());
                    continue;
                }
                if current_width + grapheme_width > max_width && current_width > 0 {
                    flush(&mut current, &mut rows, &mut current_width);
                }
                current.push_segment_with_link(style, grapheme, link_target.clone());
                current_width += grapheme_width;
            }
        }
    }

    if current_width > 0 || rows.is_empty() {
        rows.push(current);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ml(text: &str) -> MarkdownLine {
        let mut line = MarkdownLine::default();
        line.push_segment(Style::default(), text);
        line
    }

    fn text_lines(lines: &[MarkdownLine]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.segments.iter().map(|segment| segment.text.as_str()).collect())
            .collect()
    }

    fn table(headers: &[&str], rows: &[&[&str]]) -> TableBuffer {
        TableBuffer {
            headers: headers.iter().map(|text| ml(text)).collect(),
            rows: rows.iter().map(|row| row.iter().map(|text| ml(text)).collect()).collect(),
            ..TableBuffer::default()
        }
    }

    #[test]
    fn total_table_width_includes_padding_and_gaps() {
        assert_eq!(total_table_width(&[10, 10]), 26);
        assert_eq!(total_table_width(&[5, 5, 5]), 25);
    }

    #[test]
    fn wide_table_uses_padded_rows_and_horizontal_separators() {
        let lines = render_table(
            &table(&["Name", "Status"], &[&["core", "active"], &["ui", "paused"]]),
            Style::default(),
            None,
        );
        let text = text_lines(&lines);

        assert_eq!(text[0], " Name    Status");
        assert!(text[1].contains('━'));
        assert!(text[2].contains("core"));
        assert!(text[3].contains('─'));
        assert!(!text.iter().any(|line| line.contains('│')));
    }

    #[test]
    fn ambiguous_wide_terminal_renders_ascii_table_rules() {
        // Box-drawing rules break on terminals that render the
        // East-Asian-ambiguous set double-width; ASCII glyphs are the only
        // safe chrome there (nextest isolates the one-time verdict override).
        vtcode_commons::terminal_capabilities::set_ambiguous_width(
            vtcode_commons::ui_protocol::AmbiguousWidthMode::Wide,
        );
        let lines = render_table(
            &table(&["Name", "Status"], &[&["core", "active"], &["ui", "paused"]]),
            Style::default(),
            None,
        );
        let text = text_lines(&lines);

        assert!(text[1].contains('='));
        assert!(text[3].contains('-'));
        assert!(!text.iter().any(|line| line.contains('━') || line.contains('─')));
    }

    #[test]
    fn table_keeps_intrinsic_width_at_boundary_and_records_below_it() {
        let fixture = table(&["A", "B"], &[&["1", "2"]]);
        let wide = text_lines(&render_table(&fixture, Style::default(), Some(12)));
        let narrow = text_lines(&render_table(&fixture, Style::default(), Some(11)));

        assert!(wide.iter().any(|line| line.contains('━')));
        assert!(narrow.iter().any(|line| line.contains("A")));
        assert!(narrow.iter().any(|line| line.contains("B")));
        assert!(!narrow.iter().any(|line| line.contains('━')));
    }

    #[test]
    fn headerless_tables_preserve_grid_layout_when_scaled() {
        let mut fixture = table(&[], &[&["headerless", "value"]]);
        fixture.alignments = vec![Alignment::None, Alignment::None];
        let lines = render_table(&fixture, Style::default(), Some(8));
        let text = text_lines(&lines);

        assert!(text[0].contains("  "));
        assert!(lines.iter().all(|line| line.width() <= 8));
    }

    #[test]
    fn headerless_tables_fallback_before_padding_overflows_width() {
        let fixture = table(&[], &[&["left", "right"]]);
        let lines = render_table(&fixture, Style::default(), Some(6));

        assert!(lines.iter().all(|line| line.width() <= 6));
        assert!(text_lines(&lines).iter().any(|line| line.contains("left")));
    }

    #[test]
    fn wrapped_grid_preserves_columns_before_record_fallback() {
        let fixture = table(&["ID", "Description"], &[&["1", "a short readable description"]]);
        let lines = render_table(&fixture, Style::default(), Some(22));
        let text = text_lines(&lines);

        assert!(text.iter().any(|line| line.contains('━')));
        assert!(!text.iter().any(|line| line.contains("ID:")));
        assert!(lines.iter().all(|line| line.width() <= 22));
    }

    #[test]
    fn narrow_table_uses_aligned_records_with_indented_continuations() {
        let fixture = table(
            &["Claim", "Current state"],
            &[&["Threshold", "Partial: beta/context edit/trigger/pause already exists"]],
        );
        let lines = render_table(&fixture, Style::default(), Some(25));
        let text = text_lines(&lines);

        assert!(text[0].contains("Claim"));
        assert!(text[0].contains("Threshold"));
        assert!(text.iter().any(|line| line.starts_with("          ")));
        assert!(lines.iter().all(|line| line.width() <= 25));
    }

    #[test]
    fn very_narrow_records_stack_value_below_label() {
        let fixture = table(&["Label", "Value"], &[&["one", "a long value"]]);
        let lines = render_table(&fixture, Style::default(), Some(10));
        let text = text_lines(&lines);

        assert!(text.iter().any(|line| line.trim() == "Label"));
        assert!(text.iter().any(|line| line.contains("one")));
        assert!(text.iter().any(|line| line.contains("Value")));
        assert!(lines.iter().all(|line| line.width() <= 10));
    }

    #[test]
    fn markdown_alignment_is_applied_inside_cells() {
        let mut fixture = table(&["L", "C", "R"], &[&["x", "x", "x"]]);
        fixture.alignments = vec![Alignment::Left, Alignment::Center, Alignment::Right];
        let lines = render_table(&fixture, Style::default(), None);
        let header = text_lines(&lines)[0].clone();
        let row = text_lines(&lines)[2].clone();

        assert!(header.contains(" L "));
        assert!(row.contains("  x "));
        assert!(row.ends_with(" x"));
    }

    #[test]
    fn fallback_preserves_value_style_and_link() {
        let mut value = MarkdownLine::default();
        value.push_segment_with_link(
            Style::default().underline(),
            "documentation",
            Some("https://example.com".to_owned()),
        );
        let mut fixture = table(&["Reference"], &[]);
        fixture.rows = vec![vec![value]];

        let lines = render_table(&fixture, Style::default(), Some(5));
        let linked_segments = lines
            .iter()
            .flat_map(|line| line.segments.iter())
            .filter(|segment| segment.link_target.as_deref() == Some("https://example.com"))
            .collect::<Vec<_>>();
        assert_eq!(linked_segments.iter().map(|segment| segment.text.as_str()).collect::<String>(), "documentation");
        assert!(
            linked_segments
                .iter()
                .all(|segment| segment.style.get_effects().contains(anstyle::Effects::UNDERLINE))
        );
    }

    #[test]
    fn fallback_wraps_unicode_values_to_display_width() {
        let fixture = table(&["項目"], &[&["表の説明"]]);
        let lines = render_table(&fixture, Style::default(), Some(7));
        assert!(lines.iter().all(|line| line.width() <= 7), "fallback line exceeded width: {lines:?}");
    }

    #[test]
    fn wrapping_handles_words_and_long_tokens() {
        let line = ml("hello world foo bar");
        let wrapped = wrap_markdown_line(&line, 12);
        assert_eq!(text_lines(&wrapped), ["hello world", "foo bar"]);

        let line = ml("abcdefghij");
        let wrapped = wrap_markdown_line(&line, 5);
        assert_eq!(text_lines(&wrapped), ["abcde", "fghij"]);
    }
}
