use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Clear, List, ListItem, Paragraph, Widget, Wrap},
};

use super::layout_mode::LayoutMode;
use super::panel::{Panel, PanelStyles};
use crate::ui::tui::session::styling::SessionStyles;

/// Sidebar section types for organizing content
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarSection {
    Queue,
    Context,
    Tools,
    Info,
}

/// Widget for rendering the sidebar in wide mode
///
/// The sidebar provides quick access to:
/// - Queued inputs/tasks
/// - Context information
/// - Recent tool calls
/// - Session info
///
/// # Example
/// ```ignore
/// SidebarWidget::new(&styles)
///     .queue_items(&queue)
///     .context_info("12K tokens | 45% context")
///     .active_section(SidebarSection::Queue)
///     .render(sidebar_area, buf);
/// ```
pub struct SidebarWidget<'a> {
    styles: &'a SessionStyles,
    queue_items: Vec<String>,
    context_info: Option<&'a str>,
    recent_tools: Vec<String>,
    active_section: Option<SidebarSection>,
    mode: LayoutMode,
}

impl<'a> SidebarWidget<'a> {
    /// Create a new sidebar widget
    pub fn new(styles: &'a SessionStyles) -> Self {
        Self {
            styles,
            queue_items: Vec::new(),
            context_info: None,
            recent_tools: Vec::new(),
            active_section: None,
            mode: LayoutMode::Wide,
        }
    }

    /// Set queued items to display
    #[must_use]
    pub fn queue_items(mut self, items: Vec<String>) -> Self {
        self.queue_items = items;
        self
    }

    /// Set context info text
    #[must_use]
    pub fn context_info(mut self, info: &'a str) -> Self {
        self.context_info = Some(info);
        self
    }

    /// Set recent tool calls
    #[must_use]
    pub fn recent_tools(mut self, tools: Vec<String>) -> Self {
        self.recent_tools = tools;
        self
    }

    /// Set the active/focused section
    #[must_use]
    pub fn active_section(mut self, section: SidebarSection) -> Self {
        self.active_section = Some(section);
        self
    }

    /// Set the layout mode
    #[must_use]
    pub fn mode(mut self, mode: LayoutMode) -> Self {
        self.mode = mode;
        self
    }

    fn render_queue_section(&self, area: Rect, buf: &mut Buffer) {
        let is_active = self.active_section == Some(SidebarSection::Queue);
        let inner = Panel::new(self.styles)
            .title("Queue")
            .active(is_active)
            .mode(self.mode)
            .render_and_get_inner(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        if self.queue_items.is_empty() {
            let empty_text = Paragraph::new("No queued items").style(self.styles.muted_style());
            empty_text.render(inner, buf);
        } else {
            let items: Vec<ListItem> = self
                .queue_items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let prefix = format!("{}. ", i + 1);
                    let line = Line::from(vec![
                        Span::styled(prefix, self.styles.accent_style()),
                        Span::styled(
                            truncate_string(item, inner.width.saturating_sub(4) as usize),
                            self.styles.default_style(),
                        ),
                    ]);
                    ListItem::new(line)
                })
                .collect();

            let list = List::new(items);
            list.render(inner, buf);
        }
    }

    fn render_context_section(&self, area: Rect, buf: &mut Buffer) {
        let is_active = self.active_section == Some(SidebarSection::Context);
        let inner = Panel::new(self.styles)
            .title("Context")
            .active(is_active)
            .mode(self.mode)
            .render_and_get_inner(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let text = self.context_info.unwrap_or("No context info");
        let paragraph = Paragraph::new(text)
            .style(self.styles.default_style())
            .wrap(Wrap { trim: true });
        paragraph.render(inner, buf);
    }

    fn render_tools_section(&self, area: Rect, buf: &mut Buffer) {
        let is_active = self.active_section == Some(SidebarSection::Tools);
        let inner = Panel::new(self.styles)
            .title("Recent Tools")
            .active(is_active)
            .mode(self.mode)
            .render_and_get_inner(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        if self.recent_tools.is_empty() {
            let empty_text = Paragraph::new("No recent tools").style(self.styles.muted_style());
            empty_text.render(inner, buf);
        } else {
            let items: Vec<ListItem> = self
                .recent_tools
                .iter()
                .map(|tool| {
                    let line = Line::from(Span::styled(
                        format!(
                            "▸ {}",
                            truncate_string(tool, inner.width.saturating_sub(3) as usize)
                        ),
                        self.styles.default_style(),
                    ));
                    ListItem::new(line)
                })
                .collect();

            let list = List::new(items);
            list.render(inner, buf);
        }
    }
}

impl Widget for SidebarWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        if !self.mode.allow_sidebar() {
            return;
        }

        Clear.render(area, buf);

        // Split sidebar into sections
        let has_queue = !self.queue_items.is_empty();
        let has_tools = !self.recent_tools.is_empty();

        let constraints = match (has_queue, has_tools) {
            (true, true) => vec![
                Constraint::Percentage(40),
                Constraint::Percentage(30),
                Constraint::Percentage(30),
            ],
            (true, false) | (false, true) => {
                vec![Constraint::Percentage(50), Constraint::Percentage(50)]
            }
            (false, false) => vec![Constraint::Percentage(100)],
        };

        let chunks = Layout::vertical(constraints).split(area);

        let mut chunk_idx = 0;

        if has_queue && chunk_idx < chunks.len() {
            self.render_queue_section(chunks[chunk_idx], buf);
            chunk_idx += 1;
        }

        if chunk_idx < chunks.len() {
            self.render_context_section(chunks[chunk_idx], buf);
            chunk_idx += 1;
        }

        if has_tools && chunk_idx < chunks.len() {
            self.render_tools_section(chunks[chunk_idx], buf);
        }
    }
}

/// Truncate a string to fit within a given width
fn truncate_string(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else if max_width <= 3 {
        s.chars().take(max_width).collect()
    } else {
        let target = max_width.saturating_sub(3);
        let end = s
            .char_indices()
            .map(|(i, _)| i)
            .rfind(|&i| i <= target)
            .unwrap_or(0);
        format!("{}...", &s[..end])
    }
}
