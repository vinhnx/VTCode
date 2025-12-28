use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Clear, List, ListItem},
};

use super::super::types::InlineMessageKind;
use super::{Session, message::MessageLine};
use crate::config::constants::ui;

impl Session {
    pub(super) fn render_navigation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 || area.width == 0 {
            return;
        }

        // Only render timeline, plan functionality removed
        self.render_timeline_pane(frame, area);
    }

    fn render_timeline_pane(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::new()
            .title(self.timeline_block_title())
            .borders(Borders::LEFT)
            .border_type(BorderType::Plain)
            .style(self.styles.default_style())
            .border_style(self.styles.border_style());
        let inner = block.inner(area);
        if inner.height == 0 {
            frame.render_widget(block, area);
            return;
        }

        let has_items = !self.lines.is_empty();

        // Build list items efficiently
        let items: Vec<ListItem> = if has_items {
            self.lines
                .iter()
                .enumerate()
                .map(|(index, line)| self.navigation_list_item(line, index))
                .collect()
        } else {
            vec![ListItem::new("No messages yet")]
        };

        let list = List::new(items)
            .block(block)
            .style(self.styles.default_style())
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        frame.render_stateful_widget(list, area, &mut self.navigation_state);
    }

    fn navigation_list_item(&self, line: &MessageLine, index: usize) -> ListItem<'static> {
        let (label, style) = match line.kind {
            InlineMessageKind::User => {
                (ui::NAVIGATION_LABEL_USER, Style::default().fg(Color::Blue))
            }
            InlineMessageKind::Agent => (
                ui::NAVIGATION_LABEL_AGENT,
                Style::default().fg(Color::Green),
            ),
            InlineMessageKind::Tool => (
                ui::NAVIGATION_LABEL_TOOL,
                Style::default().fg(Color::Yellow),
            ),
            InlineMessageKind::Error => {
                (ui::NAVIGATION_LABEL_ERROR, Style::default().fg(Color::Red))
            }
            InlineMessageKind::Info => {
                (ui::NAVIGATION_LABEL_INFO, Style::default().fg(Color::Cyan))
            }
            InlineMessageKind::Policy => (
                ui::NAVIGATION_LABEL_POLICY,
                Style::default().fg(Color::Magenta),
            ),
            InlineMessageKind::Pty => (ui::NAVIGATION_LABEL_PTY, Style::default().fg(Color::Gray)),
        };

        let index_label = format!("{}{}", ui::NAVIGATION_INDEX_PREFIX, index + 1);
        let spans = vec![
            Span::styled(label, style),
            Span::raw(" "),
            Span::styled(index_label, Style::default().add_modifier(Modifier::DIM)),
        ];

        ListItem::new(Line::from(spans))
    }

    fn timeline_block_title(&self) -> Line<'static> {
        Line::from(vec![
            Span::styled(
                ui::NAVIGATION_BLOCK_TITLE,
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {}", ui::NAVIGATION_BLOCK_SHORTCUT_NOTE),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ])
    }
}
