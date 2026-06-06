use super::MarkdownLine;
use anstyle::Style;
use std::cmp::max;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Pre-allocated space bytes for column padding. 512 covers any reasonable terminal width.
const SPACES: [u8; 512] = [b' '; 512];

/// Returns a `&str` of `n` space characters without allocating.
/// Falls back to the full buffer if `n` exceeds 512 (unlikely in practice).
fn space_pad(n: usize) -> &'static str {
    let len = n.min(SPACES.len());
    std::str::from_utf8(&SPACES[..len]).expect("SPACES contains only ASCII bytes")
}

#[derive(Debug, Default)]
pub(crate) struct TableBuffer {
    pub(crate) headers: Vec<MarkdownLine>,
    pub(crate) rows: Vec<Vec<MarkdownLine>>,
    pub(crate) current_row: Vec<MarkdownLine>,
    pub(crate) in_head: bool,
}

pub(crate) fn render_table(
    table: &TableBuffer,
    base_style: Style,
    max_width: Option<usize>,
) -> Vec<MarkdownLine> {
    let mut lines = Vec::new();
    if table.headers.is_empty() && table.rows.is_empty() {
        return lines;
    }

    let max_cols = table
        .headers
        .len()
        .max(table.rows.iter().map(|r| r.len()).max().unwrap_or(0));
    let mut col_widths: Vec<usize> = vec![0; max_cols];

    for (col_width, width) in col_widths.iter_mut().zip(table.headers.iter().map(MarkdownLine::width))
    {
        *col_width = max(*col_width, width);
    }

    for row in &table.rows {
        for (col_width, width) in col_widths.iter_mut().zip(row.iter().map(MarkdownLine::width)) {
            *col_width = max(*col_width, width);
        }
    }

    if let Some(mw) = max_width {
        scale_columns_to_fit(&mut col_widths, mw);
    }

    let border_style = base_style.dimmed();

    if !table.headers.is_empty() {
        lines.extend(render_table_rows(
            &table.headers,
            &col_widths,
            border_style,
            base_style,
            true,
        ));

        let mut sep = MarkdownLine::default();
        for (i, width) in col_widths.iter().enumerate() {
            sep.push_segment(border_style, &"─".repeat(*width));
            if i < col_widths.len() - 1 {
                sep.push_segment(border_style, "─┼─");
            }
        }
        lines.push(sep);
    }

    for row in &table.rows {
        lines.extend(render_table_rows(
            row,
            &col_widths,
            border_style,
            base_style,
            false,
        ));
    }

    lines
}

/// Total table width: sum(col_width) + (N-1) separators of ` │ ` (3 chars each).
fn total_table_width(col_widths: &[usize]) -> usize {
    if col_widths.is_empty() {
        return 0;
    }
    let content: usize = col_widths.iter().sum();
    let separators = if col_widths.len() > 1 {
        (col_widths.len() - 1) * 3
    } else {
        0
    };
    content + separators
}

/// Proportionally scale column widths so the table fits within `max_width`.
fn scale_columns_to_fit(col_widths: &mut [usize], max_width: usize) {
    if col_widths.is_empty() {
        return;
    }
    let current = total_table_width(col_widths);
    if current <= max_width {
        return;
    }

    let n = col_widths.len();
    // Fixed overhead: separators between columns (3 chars each: " │ ")
    let fixed = if n > 1 { (n - 1) * 3 } else { 0 };
    let available = max_width.saturating_sub(fixed);
    let total_content: usize = col_widths.iter().sum();

    if total_content == 0 || available == 0 {
        return;
    }

    let scale = (available as f64) / (total_content as f64);
    for w in col_widths.iter_mut() {
        *w = ((*w as f64) * scale).max(1.0) as usize;
    }

    // Fix float rounding: trim widest columns until we fit.
    while total_table_width(col_widths) > max_width {
        if let Some(w) = col_widths.iter_mut().max() {
            if *w <= 1 {
                break;
            }
            *w -= 1;
        }
    }
}

fn render_table_rows(
    cells: &[MarkdownLine],
    col_widths: &[usize],
    border_style: Style,
    base_style: Style,
    bold: bool,
) -> Vec<MarkdownLine> {
    let num_cols = col_widths.len();
    if num_cols == 0 {
        return vec![MarkdownLine::default()];
    }

    let mut wrapped_cells: Vec<Vec<MarkdownLine>> = Vec::with_capacity(num_cols);
    for (i, &width) in col_widths.iter().enumerate() {
        if let Some(cell) = cells.get(i) {
            wrapped_cells.push(wrap_markdown_line(cell, width));
        } else {
            wrapped_cells.push(vec![MarkdownLine::default()]);
        }
    }

    let max_lines = wrapped_cells.iter().map(|c| c.len()).max().unwrap_or(1);
    let mut rows = Vec::with_capacity(max_lines);

    for line_idx in 0..max_lines {
        let mut line = MarkdownLine::default();
        for (col_idx, &width) in col_widths.iter().enumerate() {
            if let Some(cell_line) = wrapped_cells[col_idx].get(line_idx) {
                let cell_text_width = cell_line.width();
                for seg in &cell_line.segments {
                    let style = if bold { seg.style.bold() } else { seg.style };
                    line.push_segment(style, &seg.text);
                }
                let padding = width.saturating_sub(cell_text_width);
                if padding > 0 {
                    line.push_segment(base_style, space_pad(padding));
                }
            } else {
                line.push_segment(base_style, space_pad(width));
            }
            if col_idx < num_cols - 1 {
                line.push_segment(border_style, " │ ");
            }
        }
        rows.push(line);
    }

    rows
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

    let mut rows: Vec<MarkdownLine> = Vec::new();
    let mut current = MarkdownLine::default();
    let mut current_width = 0usize;

    let flush = |current: &mut MarkdownLine, rows: &mut Vec<MarkdownLine>, current_width: &mut usize| {
        trim_trailing_whitespace(current);
        rows.push(std::mem::take(current));
        *current_width = 0;
    };

    for seg in &line.segments {
        let style = seg.style;
        for token in seg.text.split_word_bounds() {
            if token.is_empty() {
                continue;
            }

            let token_width = UnicodeWidthStr::width(token);
            if token_width == 0 {
                current.push_segment(style, token);
                continue;
            }

            let is_whitespace = token.chars().all(char::is_whitespace);
            let has_content = current_width > 0;

            if is_whitespace && !rows.is_empty() && !has_content {
                continue;
            }

            if current_width + token_width <= max_width {
                current.push_segment(style, token);
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
                current.push_segment(style, token);
                current_width += token_width;
                continue;
            }

            for grapheme in UnicodeSegmentation::graphemes(token, true) {
                if grapheme.is_empty() {
                    continue;
                }
                let gw = UnicodeWidthStr::width(grapheme);
                if gw == 0 {
                    current.push_segment(style, grapheme);
                    continue;
                }
                if current_width + gw > max_width && current_width > 0 {
                    flush(&mut current, &mut rows, &mut current_width);
                }
                current.push_segment(style, grapheme);
                current_width += gw;
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

    #[test]
    fn test_scale_columns_no_change_when_fits() {
        let mut widths = vec![10, 10];
        scale_columns_to_fit(&mut widths, 200);
        assert_eq!(widths, vec![10, 10]);
    }

    #[test]
    fn test_scale_columns_proportional_reduction() {
        let mut widths = vec![40, 40];
        scale_columns_to_fit(&mut widths, 60);
        assert!(total_table_width(&widths) <= 60);
        assert!(widths[0] >= 1);
        assert!(widths[1] >= 1);
    }

    #[test]
    fn test_scale_columns_respects_max_width() {
        let mut widths = vec![7, 7, 7, 7, 7];
        scale_columns_to_fit(&mut widths, 30);
        assert!(
            total_table_width(&widths) <= 30,
            "total={} widths={:?}",
            total_table_width(&widths),
            widths
        );
    }

    #[test]
    fn test_total_table_width_two_cols() {
        // 10 + 3 + 10 = 23
        assert_eq!(total_table_width(&[10, 10]), 23);
    }

    #[test]
    fn test_total_table_width_three_cols() {
        // 5 + 3 + 5 + 3 + 5 = 21
        assert_eq!(total_table_width(&[5, 5, 5]), 21);
    }

    #[test]
    fn test_render_table_no_outer_borders() {
        let table = TableBuffer {
            headers: vec![ml("A"), ml("B")],
            rows: vec![vec![ml("1"), ml("2")]],
            current_row: vec![],
            in_head: false,
        };
        let lines = render_table(&table, Style::default(), None);
        let text_lines: Vec<String> = lines
            .iter()
            .map(|l| l.segments.iter().map(|s| s.text.as_str()).collect())
            .collect();
        // No outer │ borders
        assert!(!text_lines[0].starts_with("│"), "Should not start with │");
        assert!(!text_lines[0].ends_with("│"), "Should not end with │");
        // Inner separator present
        assert!(text_lines[0].contains("│"), "Header should have inner │");
    }

    #[test]
    fn test_render_table_with_max_width() {
        let table = TableBuffer {
            headers: vec![ml("Name"), ml("Description")],
            rows: vec![vec![ml("foo"), ml("bar")]],
            current_row: vec![],
            in_head: false,
        };
        let lines = render_table(&table, Style::default(), Some(40));
        for line in &lines {
            let text: String = line.segments.iter().map(|s| s.text.as_str()).collect();
            assert!(
                text.chars().count() <= 40,
                "Line exceeds 40 chars: {:?}",
                text
            );
        }
    }

    #[test]
    fn test_wrap_markdown_line_no_wrap_when_fits() {
        let line = ml("short text");
        let wrapped = wrap_markdown_line(&line, 20);
        assert_eq!(wrapped.len(), 1);
    }

    #[test]
    fn test_wrap_markdown_line_wraps_at_word_boundary() {
        let line = ml("hello world foo bar");
        let wrapped = wrap_markdown_line(&line, 12);
        assert_eq!(wrapped.len(), 2);
        let t0: String = wrapped[0].segments.iter().map(|s| s.text.as_str()).collect();
        let t1: String = wrapped[1].segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(t0, "hello world");
        assert_eq!(t1, "foo bar");
    }

    #[test]
    fn test_wrap_markdown_line_grapheme_fallback() {
        let line = ml("abcdefghij");
        let wrapped = wrap_markdown_line(&line, 5);
        assert_eq!(wrapped.len(), 2);
        let t0: String = wrapped[0].segments.iter().map(|s| s.text.as_str()).collect();
        let t1: String = wrapped[1].segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(t0, "abcde");
        assert_eq!(t1, "fghij");
    }

    #[test]
    fn test_render_table_wraps_long_cells() {
        let table = TableBuffer {
            headers: vec![ml("A"), ml("B")],
            rows: vec![vec![ml("short"), ml("this is a long cell value")]],
            current_row: vec![],
            in_head: false,
        };
        let lines = render_table(&table, Style::default(), Some(30));
        for line in &lines {
            let text: String = line.segments.iter().map(|s| s.text.as_str()).collect();
            assert!(
                text.chars().count() <= 30,
                "Line exceeds 30 chars: {:?}",
                text
            );
        }
    }

    #[test]
    fn test_render_table_wrapped_rows_are_aligned() {
        let table = TableBuffer {
            headers: vec![ml("H1"), ml("H2")],
            rows: vec![vec![
                ml("ab"),
                ml("this is a long value that wraps"),
            ]],
            current_row: vec![],
            in_head: false,
        };
        let lines = render_table(&table, Style::default(), Some(25));
        // Header + separator + multiple data lines
        assert!(lines.len() >= 4, "Expected wrapped rows, got {}", lines.len());
        // Each line should be within max_width
        for line in &lines {
            let text: String = line.segments.iter().map(|s| s.text.as_str()).collect();
            assert!(text.chars().count() <= 25, "Line too wide: {:?}", text);
        }
    }
}
