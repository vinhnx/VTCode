use super::MarkdownLine;
use anstyle::Style;
use std::cmp::max;

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
    let header_widths: Vec<usize> = table.headers.iter().map(MarkdownLine::width).collect();
    let row_widths: Vec<Vec<usize>> = table
        .rows
        .iter()
        .map(|row| row.iter().map(MarkdownLine::width).collect())
        .collect();

    for (col_width, width) in col_widths.iter_mut().zip(header_widths.iter()) {
        *col_width = max(*col_width, *width);
    }

    for widths in &row_widths {
        for (col_width, width) in col_widths.iter_mut().zip(widths.iter()) {
            *col_width = max(*col_width, *width);
        }
    }

    if let Some(mw) = max_width {
        scale_columns_to_fit(&mut col_widths, mw);
    }

    let border_style = base_style.dimmed();

    if !table.headers.is_empty() {
        lines.push(render_table_row(
            &table.headers,
            &header_widths,
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

    for (row, widths) in table.rows.iter().zip(row_widths.iter()) {
        lines.push(render_table_row(
            row,
            widths,
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

fn render_table_row(
    cells: &[MarkdownLine],
    cell_widths: &[usize],
    col_widths: &[usize],
    border_style: Style,
    base_style: Style,
    bold: bool,
) -> MarkdownLine {
    let mut line = MarkdownLine::default();
    for (i, width) in col_widths.iter().enumerate() {
        if let Some(c) = cells.get(i) {
            for seg in &c.segments {
                let style = if bold { seg.style.bold() } else { seg.style };
                line.push_segment(style, &seg.text);
            }
            let cell_width = cell_widths.get(i).copied().unwrap_or(0);
            let padding = width.saturating_sub(cell_width);
            if padding > 0 {
                line.push_segment(base_style, &" ".repeat(padding));
            }
        } else {
            line.push_segment(base_style, &" ".repeat(*width));
        }
        if i < col_widths.len() - 1 {
            line.push_segment(border_style, " │ ");
        }
    }
    line
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
}
