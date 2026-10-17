//! Feature-gated ANSI and Ratatui renderer adapters.

#[cfg(feature = "ansi")]
mod ansi_adapter {
    use crate::DiffRow;
    use anstyle::{Reset, Style};

    /// Caller-supplied foreground styles for ANSI output.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct AnsiDiffPalette {
        /// Hunk and omission style.
        pub header: Style,
        /// Context style.
        pub context: Style,
        /// Addition style.
        pub addition: Style,
        /// Deletion style.
        pub deletion: Style,
        /// Extra intraline emphasis.
        pub emphasis: Style,
    }

    /// Renders semantic rows as ANSI lines.
    #[must_use]
    pub fn render_ansi_rows(rows: &[DiffRow], palette: AnsiDiffPalette, color: bool) -> Vec<String> {
        rows.iter()
            .map(|row| {
                let mut output = String::new();
                output.push(row.marker);
                output.push(' ');
                if let Some(cell) = &row.left {
                    render_cell(&mut output, cell, palette, color);
                }
                if let Some(cell) = &row.right {
                    output.push_str(" │ ");
                    render_cell(&mut output, cell, palette, color);
                }
                output
            })
            .collect()
    }

    fn render_cell(output: &mut String, cell: &crate::DiffCell, palette: AnsiDiffPalette, color: bool) {
        let style = match cell.marker {
            '+' => palette.addition,
            '-' => palette.deletion,
            '@' | '…' => palette.header,
            _ => palette.context,
        };
        for segment in &cell.segments {
            if color {
                let selected = if segment.emphasized { palette.emphasis } else { style };
                output.push_str(&selected.render().to_string());
                output.push_str(&segment.text);
                output.push_str(&Reset.render().to_string());
            } else {
                output.push_str(&segment.text);
            }
        }
    }
}

#[cfg(feature = "ansi")]
pub use ansi_adapter::{AnsiDiffPalette, render_ansi_rows};

#[cfg(feature = "ratatui")]
mod ratatui_adapter {
    use crate::DiffRow;
    use ratatui::text::{Line, Span};

    /// Converts semantic rows to unstyled Ratatui lines for caller styling.
    #[must_use]
    pub fn to_ratatui_lines(rows: &[DiffRow]) -> Vec<Line<'static>> {
        rows.iter()
            .map(|row| {
                let mut spans = vec![Span::raw(format!("{} ", row.marker))];
                if let Some(cell) = &row.left {
                    spans.extend(cell.segments.iter().map(|segment| Span::raw(segment.text.clone())));
                }
                if let Some(cell) = &row.right {
                    spans.push(Span::raw(" │ "));
                    spans.extend(cell.segments.iter().map(|segment| Span::raw(segment.text.clone())));
                }
                Line::from(spans)
            })
            .collect()
    }
}

#[cfg(feature = "ratatui")]
pub use ratatui_adapter::to_ratatui_lines;
