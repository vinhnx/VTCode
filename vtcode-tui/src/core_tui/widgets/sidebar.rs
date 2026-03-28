use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Clear, Paragraph, StatefulWidget, Widget, Wrap},
};
use tui_widget_list::{ListBuilder, ListState as WidgetListState, ListView};

use super::layout_mode::LayoutMode;
use super::panel::{Panel, PanelStyles};
use crate::core_tui::types::LocalAgentEntry;
use crate::ui::tui::session::styling::SessionStyles;

/// Ellipsis character used to indicate truncated text (consistent with line_truncation module).
const ELLIPSIS: &str = "…";

#[derive(Clone)]
struct SidebarListItem {
    line: Line<'static>,
}

impl Widget for SidebarListItem {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.line)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

fn render_static_list(lines: Vec<Line<'static>>, area: Rect, buf: &mut Buffer) {
    if area.width == 0 || area.height == 0 || lines.is_empty() {
        return;
    }

    let rows = lines
        .into_iter()
        .map(|line| (SidebarListItem { line }, 1_u16))
        .collect::<Vec<_>>();
    let count = rows.len();
    let builder = ListBuilder::new(move |context| rows[context.index].clone());
    let widget = ListView::new(builder, count).infinite_scrolling(false);
    let mut state = WidgetListState::default();
    StatefulWidget::render(widget, area, buf, &mut state);
}

/// Sidebar section types for organizing content
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarSection {
    LocalAgents,
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
    local_agents: Vec<LocalAgentEntry>,
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
            local_agents: Vec::new(),
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

    #[must_use]
    pub fn local_agents(mut self, entries: Vec<LocalAgentEntry>) -> Self {
        self.local_agents = entries;
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
            let lines = self
                .queue_items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let prefix = format!("{}. ", i + 1);
                    Line::from(vec![
                        Span::styled(prefix, self.styles.accent_style()),
                        Span::styled(
                            truncate_string(item, inner.width.saturating_sub(4) as usize),
                            self.styles.default_style(),
                        ),
                    ])
                })
                .collect();

            render_static_list(lines, inner, buf);
        }
    }

    fn render_local_agents_section(&self, area: Rect, buf: &mut Buffer) {
        let is_active = self.active_section == Some(SidebarSection::LocalAgents);
        let inner = Panel::new(self.styles)
            .title("Local Agents")
            .active(is_active)
            .mode(self.mode)
            .render_and_get_inner(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        if self.local_agents.is_empty() {
            let empty_text = Paragraph::new("No local agents").style(self.styles.muted_style());
            empty_text.render(inner, buf);
        } else {
            let mut lines = self
                .local_agents
                .iter()
                .take(4)
                .map(|entry| {
                    Line::from(Span::styled(
                        truncate_string(
                            &format!(
                                "{} · {} · {}",
                                entry.display_label,
                                entry.kind.as_str(),
                                entry.status
                            ),
                            inner.width.saturating_sub(2) as usize,
                        ),
                        self.styles.default_style(),
                    ))
                })
                .collect::<Vec<_>>();

            if let Some(entry) = self.local_agents.first() {
                lines.push(Line::from(String::new()));
                lines.push(Line::from(Span::styled(
                    truncate_string(&entry.preview, inner.width.saturating_sub(2) as usize),
                    self.styles.muted_style(),
                )));
            }

            render_static_list(lines, inner, buf);
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
            let lines = self
                .recent_tools
                .iter()
                .map(|tool| {
                    Line::from(Span::styled(
                        format!(
                            "▸ {}",
                            truncate_string(tool, inner.width.saturating_sub(3) as usize)
                        ),
                        self.styles.default_style(),
                    ))
                })
                .collect();

            render_static_list(lines, inner, buf);
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
        let has_local_agents = !self.local_agents.is_empty();
        let has_queue = !self.queue_items.is_empty();
        let has_tools = !self.recent_tools.is_empty();

        let mut sections = Vec::<(SidebarSection, u32)>::new();
        if has_local_agents {
            sections.push((SidebarSection::LocalAgents, 7));
        }
        if has_queue {
            sections.push((SidebarSection::Queue, 3));
        }
        sections.push((SidebarSection::Context, 2));
        if has_tools {
            sections.push((SidebarSection::Tools, 2));
        }

        let total_weight = sections
            .iter()
            .map(|(_, weight)| *weight)
            .sum::<u32>()
            .max(1);
        let constraints = sections
            .iter()
            .map(|(_, weight)| Constraint::Ratio(*weight, total_weight))
            .collect::<Vec<_>>();
        let chunks = Layout::vertical(constraints).split(area);

        for ((section, _), chunk) in sections.into_iter().zip(chunks.iter()) {
            match section {
                SidebarSection::LocalAgents => self.render_local_agents_section(*chunk, buf),
                SidebarSection::Queue => self.render_queue_section(*chunk, buf),
                SidebarSection::Context => self.render_context_section(*chunk, buf),
                SidebarSection::Tools => self.render_tools_section(*chunk, buf),
                SidebarSection::Info => {}
            }
        }
    }
}

/// Truncate a string to fit within a given width
fn truncate_string(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else if max_width <= ELLIPSIS.len() {
        s.chars().take(max_width).collect()
    } else {
        let target = max_width.saturating_sub(ELLIPSIS.len());
        let end = s
            .char_indices()
            .map(|(i, _)| i)
            .rfind(|&i| i <= target)
            .unwrap_or(0);
        format!("{}{}", &s[..end], ELLIPSIS)
    }
}

#[cfg(test)]
mod tests {
    use super::SidebarWidget;
    use crate::core_tui::session::styling::SessionStyles;
    use crate::ui::tui::types::InlineTheme;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;

    #[test]
    fn sidebar_renders_local_agent_entries() {
        let styles = SessionStyles::new(InlineTheme::default());
        let area = Rect::new(0, 0, 60, 16);
        let mut buf = Buffer::empty(area);

        SidebarWidget::new(&styles)
            .local_agents(vec![
                crate::core_tui::types::LocalAgentEntry {
                    id: "thread-1".to_string(),
                    display_label: "rust-engineer".to_string(),
                    agent_name: "rust-engineer".to_string(),
                    color: Some("cyan".to_string()),
                    kind: crate::core_tui::types::LocalAgentKind::Delegated,
                    status: "running".to_string(),
                    summary: None,
                    preview: "assistant: reviewing the workspace".to_string(),
                    transcript_path: None,
                },
                crate::core_tui::types::LocalAgentEntry {
                    id: "bg-1".to_string(),
                    display_label: "reviewer".to_string(),
                    agent_name: "reviewer".to_string(),
                    color: None,
                    kind: crate::core_tui::types::LocalAgentKind::Background,
                    status: "starting".to_string(),
                    summary: None,
                    preview: "waiting for output".to_string(),
                    transcript_path: None,
                },
            ])
            .context_info("Ready")
            .render(area, &mut buf);

        let rendered = (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|col| buf[(col, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Local Agents"));
        assert!(rendered.contains("rust-engineer"));
        assert!(rendered.contains("reviewer"));
    }

    #[test]
    fn sidebar_renders_local_agent_preview() {
        let styles = SessionStyles::new(InlineTheme::default());
        let area = Rect::new(0, 0, 60, 16);
        let mut buf = Buffer::empty(area);

        SidebarWidget::new(&styles)
            .local_agents(vec![crate::core_tui::types::LocalAgentEntry {
                id: "thread-1".to_string(),
                display_label: "rust-engineer".to_string(),
                agent_name: "rust-engineer".to_string(),
                color: Some("cyan".to_string()),
                kind: crate::core_tui::types::LocalAgentKind::Delegated,
                status: "running".to_string(),
                summary: None,
                preview: "thinking: Inspecting the diff carefully".to_string(),
                transcript_path: None,
            }])
            .context_info("Ready")
            .render(area, &mut buf);

        let rendered = (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|col| buf[(col, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Local Agents"));
        assert!(rendered.contains("rust-engineer"));
        assert!(rendered.contains("Inspecting the diff carefully"));
    }
}
