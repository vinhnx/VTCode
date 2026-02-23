use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Widget},
};

use super::layout_mode::LayoutMode;
use super::panel::PanelStyles;
use crate::ui::tui::session::styling::SessionStyles;
use crate::ui::tui::session::terminal_capabilities;
use tui_shimmer::shimmer_spans_with_style_at_phase;

use crate::ui::tui::session::status_requires_shimmer;

/// Widget for rendering the footer area with status and hints
///
/// The footer provides a stable region for:
/// - Left status (git branch, model info)
/// - Right status (token count, context usage)
/// - Help hints (shown conditionally)
///
/// # Example
/// ```ignore
/// FooterWidget::new(&styles)
///     .left_status("main ✓")
///     .right_status("claude-4 | 12K tokens")
///     .hint("? for help")
///     .mode(LayoutMode::Standard)
///     .render(footer_area, buf);
/// ```
pub struct FooterWidget<'a> {
    styles: &'a SessionStyles,
    left_status: Option<&'a str>,
    right_status: Option<&'a str>,
    hint: Option<&'a str>,
    mode: LayoutMode,
    show_border: bool,
    spinner: Option<&'a str>,
    shimmer_phase: Option<f32>,
}

impl<'a> FooterWidget<'a> {
    /// Create a new footer widget
    pub fn new(styles: &'a SessionStyles) -> Self {
        Self {
            styles,
            left_status: None,
            right_status: None,
            hint: None,
            mode: LayoutMode::Standard,
            show_border: false,
            spinner: None,
            shimmer_phase: None,
        }
    }

    /// Set the left status text (e.g., git branch)
    #[must_use]
    pub fn left_status(mut self, status: &'a str) -> Self {
        self.left_status = Some(status);
        self
    }

    /// Set the right status text (e.g., model info)
    #[must_use]
    pub fn right_status(mut self, status: &'a str) -> Self {
        self.right_status = Some(status);
        self
    }

    /// Set the hint text (shown when idle)
    #[must_use]
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = Some(hint);
        self
    }

    /// Set the layout mode
    #[must_use]
    pub fn mode(mut self, mode: LayoutMode) -> Self {
        self.mode = mode;
        self
    }

    /// Show a top border
    #[must_use]
    pub fn show_border(mut self, show: bool) -> Self {
        self.show_border = show;
        self
    }

    /// Set spinner text (shown when processing)
    #[must_use]
    pub fn spinner(mut self, spinner: &'a str) -> Self {
        self.spinner = Some(spinner);
        self
    }

    /// Set shimmer phase for animated status text
    #[must_use]
    pub fn shimmer_phase(mut self, phase: f32) -> Self {
        self.shimmer_phase = Some(phase);
        self
    }

    fn build_status_line(&self, width: u16) -> Line<'static> {
        let mut spans = Vec::new();

        // Left status
        if let Some(left) = self.left_status {
            if status_requires_shimmer(left) {
                if let Some(phase) = self.shimmer_phase {
                    spans.extend(shimmer_spans_with_style_at_phase(
                        left,
                        self.styles.muted_style(),
                        phase,
                    ));
                } else {
                    spans.push(Span::styled(left.to_string(), self.styles.muted_style()));
                }
            } else {
                spans.push(Span::styled(left.to_string(), self.styles.accent_style()));
            }
        }

        // Spinner (if active)
        if let Some(spinner) = self.spinner {
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(spinner.to_string(), self.styles.muted_style()));
        }

        // Calculate space needed for right status
        let right_text = self.right_status.unwrap_or("");
        let left_len: usize = spans.iter().map(|s| s.content.len()).sum();
        let right_len = right_text.len();
        let available = width as usize;

        // Add padding and right status if there's room
        if left_len + right_len + 2 <= available {
            let padding = available.saturating_sub(left_len + right_len);
            spans.push(Span::raw(" ".repeat(padding)));
            spans.push(Span::styled(
                right_text.to_string(),
                self.styles.muted_style(),
            ));
        }

        Line::from(spans)
    }

    fn build_hint_line(&self) -> Option<Line<'static>> {
        match self.mode {
            LayoutMode::Compact => None,
            _ => self
                .hint
                .map(|hint| Line::from(Span::styled(hint.to_string(), self.styles.muted_style()))),
        }
    }
}

impl Widget for FooterWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        Clear.render(area, buf);

        let inner = if self.show_border && self.mode.show_borders() {
            let block = Block::bordered()
                .border_type(terminal_capabilities::get_border_type())
                .border_style(self.styles.border_style());
            let inner = block.inner(area);
            block.render(area, buf);
            inner
        } else {
            area
        };

        if inner.height == 0 {
            return;
        }

        let status_line = self.build_status_line(inner.width);
        let hint_line = self.build_hint_line();

        let lines: Vec<Line<'static>> = if inner.height >= 2 {
            if let Some(hint) = hint_line {
                vec![status_line, hint]
            } else {
                vec![status_line]
            }
        } else {
            vec![status_line]
        };

        let paragraph = Paragraph::new(lines);
        paragraph.render(inner, buf);
    }
}

/// Default keybind hints for different contexts
pub mod hints {
    pub const IDLE: &str = "? help • / command • @ file";
    pub const PROCESSING: &str = "Ctrl+C cancel";
    pub const MODAL: &str = "↑↓ navigate • Enter select • Esc close";
    pub const EDITING: &str = "Enter send • Ctrl+C cancel • ↑ history";
}
