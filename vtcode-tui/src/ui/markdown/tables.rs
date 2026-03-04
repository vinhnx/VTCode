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

pub(crate) fn render_table(table: &TableBuffer, base_style: Style) -> Vec<MarkdownLine> {
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

    for (i, width) in header_widths.iter().enumerate() {
        col_widths[i] = max(col_widths[i], *width);
    }

    for widths in &row_widths {
        for (i, width) in widths.iter().enumerate() {
            col_widths[i] = max(col_widths[i], *width);
        }
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
        sep.push_segment(border_style, "├─");
        for (i, width) in col_widths.iter().enumerate() {
            sep.push_segment(border_style, &"─".repeat(*width));
            sep.push_segment(
                border_style,
                if i < col_widths.len() - 1 {
                    "─┼─"
                } else {
                    "─┤"
                },
            );
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

fn render_table_row(
    cells: &[MarkdownLine],
    cell_widths: &[usize],
    col_widths: &[usize],
    border_style: Style,
    base_style: Style,
    bold: bool,
) -> MarkdownLine {
    let mut line = MarkdownLine::default();
    line.push_segment(border_style, "│ ");
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
        line.push_segment(border_style, " │ ");
    }
    line
}
