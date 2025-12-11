//! Professional table formatting for terminal output with automatic column sizing.
//!
//! This module provides utilities to render markdown tables with proper alignment,
//! column width calculation, and capability-aware box-drawing characters.

use unicode_width::UnicodeWidthStr;

/// Horizontal alignment for table cells
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    /// Left-aligned content with right padding
    Left,
    /// Center-aligned content with equal padding on both sides
    Center,
    /// Right-aligned content with left padding
    Right,
}

/// Information about a single table column
#[derive(Clone, Debug)]
pub struct TableColumn {
    /// The minimum width needed for this column
    pub width: usize,
    /// Cell alignment direction
    pub alignment: Alignment,
    /// Column header text
    pub header: String,
}

impl TableColumn {
    /// Create a new column with header and calculate width
    pub fn new(header: impl Into<String>, alignment: Alignment) -> Self {
        let header_str = header.into();
        let width = UnicodeWidthStr::width(header_str.as_str());
        Self {
            width,
            alignment,
            header: header_str,
        }
    }

    /// Update column width based on cell content, keeping maximum
    pub fn measure_cell(&mut self, content: &str) {
        let cell_width = UnicodeWidthStr::width(content);
        self.width = self.width.max(cell_width);
    }
}

/// Table formatter with column width detection and alignment
#[derive(Clone, Debug)]
pub struct TableFormatter {
    /// Column definitions with calculated widths
    pub columns: Vec<TableColumn>,
    /// Whether to use Unicode box-drawing characters (vs ASCII)
    pub use_unicode: bool,
}

impl TableFormatter {
    /// Create a new table formatter with specified columns
    pub fn new(columns: Vec<TableColumn>, use_unicode: bool) -> Self {
        Self {
            columns,
            use_unicode,
        }
    }

    /// Measure all content and update column widths
    pub fn measure_content(&mut self, rows: &[Vec<String>]) {
        for row in rows {
            for (col_idx, cell) in row.iter().enumerate() {
                if let Some(column) = self.columns.get_mut(col_idx) {
                    column.measure_cell(cell);
                }
            }
        }
    }

    /// Render a header separator line
    pub fn render_separator(&self) -> String {
        let (left, junction, right, line) = if self.use_unicode {
            ('├', '┼', '┤', '─')
        } else {
            ('+', '+', '+', '-')
        };

        let mut separator = String::from(left);
        for (idx, column) in self.columns.iter().enumerate() {
            separator.push_str(&line.to_string().repeat(column.width + 2));
            if idx < self.columns.len() - 1 {
                separator.push(junction);
            }
        }
        separator.push(right);
        separator
    }

    /// Format a single cell with alignment and padding
    fn format_cell(&self, content: &str, alignment: Alignment, width: usize) -> String {
        let content_width = UnicodeWidthStr::width(content);
        if content_width >= width {
            return content.to_string();
        }

        let padding = width - content_width;
        match alignment {
            Alignment::Left => {
                format!("{}{}", content, " ".repeat(padding))
            }
            Alignment::Center => {
                let left_pad = padding / 2;
                let right_pad = padding - left_pad;
                format!(
                    "{}{}{}",
                    " ".repeat(left_pad),
                    content,
                    " ".repeat(right_pad)
                )
            }
            Alignment::Right => {
                format!("{}{}", " ".repeat(padding), content)
            }
        }
    }

    /// Render a single row of cells
    pub fn render_row(&self, cells: &[String]) -> String {
        let (sep, left, right) = if self.use_unicode {
            ("│", "│", "│")
        } else {
            ("|", "|", "|")
        };

        let mut row = String::from(left);
        for (idx, (cell, column)) in cells.iter().zip(self.columns.iter()).enumerate() {
            let formatted = self.format_cell(cell, column.alignment, column.width);
            row.push(' ');
            row.push_str(&formatted);
            row.push(' ');
            if idx < self.columns.len() - 1 {
                row.push_str(sep);
            }
        }
        row.push_str(right);
        row
    }

    /// Render header row with separator line
    pub fn render_header(&self) -> Vec<String> {
        let headers: Vec<String> = self.columns.iter().map(|col| col.header.clone()).collect();

        vec![self.render_row(&headers), self.render_separator()]
    }

    /// Render entire table with header, rows, and footer
    pub fn render_table(&self, rows: &[Vec<String>]) -> Vec<String> {
        let mut output = Vec::new();

        // Top border
        let (left, right, line) = if self.use_unicode {
            ('┌', '┐', '─')
        } else {
            ('+', '+', '-')
        };
        let mut top_border = String::from(left);
        for (idx, column) in self.columns.iter().enumerate() {
            top_border.push_str(&line.to_string().repeat(column.width + 2));
            if idx < self.columns.len() - 1 {
                let junction = if self.use_unicode { '┬' } else { '+' };
                top_border.push(junction);
            }
        }
        top_border.push(right);
        output.push(top_border);

        // Header with separator
        output.extend(self.render_header());

        // Data rows
        for row in rows {
            output.push(self.render_row(row));
        }

        // Bottom border
        let (left, right) = if self.use_unicode {
            ('└', '┘')
        } else {
            ('+', '+')
        };
        let mut bottom_border = String::from(left);
        for (idx, column) in self.columns.iter().enumerate() {
            bottom_border.push_str(&line.to_string().repeat(column.width + 2));
            if idx < self.columns.len() - 1 {
                let junction = if self.use_unicode { '┴' } else { '+' };
                bottom_border.push(junction);
            }
        }
        bottom_border.push(right);
        output.push(bottom_border);

        output
    }

    /// Get total table width including borders and padding
    pub fn total_width(&self) -> usize {
        // Left border (1) + cells with padding (each 2) + separators (n-1) + right border (1)
        let content_width: usize = self.columns.iter().map(|c| c.width + 2).sum();
        let separators = if self.columns.is_empty() {
            0
        } else {
            self.columns.len() - 1
        };
        1 + content_width + separators + 1
    }

    /// Wrap table to fit within maximum width if needed
    pub fn wrap_to_width(&mut self, max_width: usize) {
        if self.total_width() <= max_width || self.columns.is_empty() {
            return;
        }

        // Proportionally reduce column widths
        let total_content: usize = self.columns.iter().map(|c| c.width).sum();
        let available = max_width.saturating_sub(self.columns.len() + 3); // Borders + padding

        let scale = (available as f64) / (total_content as f64);
        for column in &mut self.columns {
            column.width = ((column.width as f64) * scale).max(3.0) as usize;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_width_calculation() {
        let mut col = TableColumn::new("Name", Alignment::Left);
        assert_eq!(col.width, 4);
        col.measure_cell("Alexander");
        assert_eq!(col.width, 9);
    }

    #[test]
    fn test_format_cell_left_align() {
        let formatter = TableFormatter::new(vec![], false);
        let result = formatter.format_cell("Hi", Alignment::Left, 5);
        assert_eq!(result, "Hi   ");
    }

    #[test]
    fn test_format_cell_center_align() {
        let formatter = TableFormatter::new(vec![], false);
        let result = formatter.format_cell("Hi", Alignment::Center, 5);
        assert_eq!(result, " Hi  ");
    }

    #[test]
    fn test_format_cell_right_align() {
        let formatter = TableFormatter::new(vec![], false);
        let result = formatter.format_cell("Hi", Alignment::Right, 5);
        assert_eq!(result, "   Hi");
    }

    #[test]
    fn test_separator_rendering() {
        let formatter = TableFormatter::new(
            vec![
                TableColumn::new("A", Alignment::Left),
                TableColumn::new("B", Alignment::Left),
            ],
            false,
        );
        let sep = formatter.render_separator();
        assert!(sep.starts_with('+'));
        assert!(sep.ends_with('+'));
        assert!(sep.contains("+"));
    }

    #[test]
    fn test_total_width() {
        let formatter = TableFormatter::new(
            vec![
                TableColumn::new("Col1", Alignment::Left),
                TableColumn::new("Col2", Alignment::Left),
            ],
            false,
        );
        // 1 (left border) + (4+2) + 1 (separator) + (4+2) + 1 (right border) = 15
        assert_eq!(formatter.total_width(), 15);
    }

    #[test]
    fn test_unicode_vs_ascii() {
        let unicode_fmt = TableFormatter::new(vec![TableColumn::new("A", Alignment::Left)], true);
        let ascii_fmt = TableFormatter::new(vec![TableColumn::new("A", Alignment::Left)], false);

        let unicode_sep = unicode_fmt.render_separator();
        let ascii_sep = ascii_fmt.render_separator();

        assert!(unicode_sep.contains("├"));
        assert!(ascii_sep.contains("+"));
    }
}
