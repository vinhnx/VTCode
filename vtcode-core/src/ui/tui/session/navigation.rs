use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem},
};

use crate::{
    config::constants::ui,
    tools::{PlanCompletionState, PlanStep, StepStatus},
};

use super::super::types::{InlineMessageKind, InlineTextStyle};
use super::{Session, message::MessageLine, ratatui_color_from_ansi, ratatui_style_from_inline};

impl Session {
    pub(super) fn render_navigation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 || area.width == 0 {
            return;
        }

        if self.should_show_plan() {
            let plan_len = self.plan.steps.len();
            // Calculate desired plan height: steps + header + borders + padding
            // We cap it at 40% of the available height to ensure timeline remains visible
            let desired_plan_height = (plan_len as u16).saturating_add(4);
            let max_plan_height = (area.height as f32 * 0.4) as u16;
            let plan_height = desired_plan_height.min(max_plan_height).max(5);

            let chunks = Layout::vertical([
                Constraint::Min(5), // Timeline gets the rest, but at least 5 rows
                Constraint::Length(plan_height),
            ])
            .split(area);

            self.render_timeline_pane(frame, chunks[0]);
            self.render_plan_pane(frame, chunks[1]);
        } else {
            self.render_timeline_pane(frame, area);
        }
    }

    fn render_timeline_pane(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default()
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

        let items = self.timeline_navigation_items();
        let item_count = items.len();
        let viewport = inner.height as usize;

        if self.lines.is_empty() {
            self.navigation_state.select(None);
            *self.navigation_state.offset_mut() = 0;
        } else {
            let last_index = self.lines.len().saturating_sub(1);
            self.navigation_state.select(Some(last_index));
            let max_offset = item_count.saturating_sub(viewport);
            *self.navigation_state.offset_mut() = max_offset;
        }

        let list = List::new(items)
            .block(block)
            .style(self.styles.default_style())
            .highlight_style(self.navigation_highlight_style())
            .highlight_symbol(ui::MODAL_LIST_HIGHLIGHT_FULL)
            .repeat_highlight_symbol(true);

        frame.render_stateful_widget(list, area, &mut self.navigation_state);
    }

    fn render_plan_pane(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let block = Block::default()
            .title(self.plan_block_title())
            .borders(Borders::LEFT | Borders::TOP)
            .border_type(BorderType::Plain)
            .style(self.styles.default_style())
            .border_style(self.styles.border_style());
        let inner = block.inner(area);
        if inner.height == 0 {
            frame.render_widget(block, area);
            return;
        }

        let mut items = Vec::new();
        let mut offset = 0;

        // Add goal/explanation if it exists and we have space
        if let Some(explanation) = &self.plan.explanation
            && inner.height > 4
        {
            items.push(ListItem::new(Line::from(vec![
                Span::styled("Goal: ", self.navigation_index_style()),
                Span::styled(explanation.clone(), self.styles.default_style()),
            ])));
            items.push(ListItem::new(Line::from("")));
            offset = 2;
        }

        items.extend(self.plan_navigation_items());
        let item_count = items.len();
        let viewport = inner.height as usize;

        if item_count == 0 {
            self.plan_navigation_state.select(None);
            *self.plan_navigation_state.offset_mut() = 0;
        } else if let Some(selected) = self.plan_selected_index() {
            let adjusted_selected = selected + offset;
            self.plan_navigation_state.select(Some(adjusted_selected));
            let max_offset = item_count.saturating_sub(viewport);
            let desired_offset = adjusted_selected.saturating_sub(viewport.saturating_sub(1));
            *self.plan_navigation_state.offset_mut() = desired_offset.min(max_offset);
        } else {
            self.plan_navigation_state.select(None);
            *self.plan_navigation_state.offset_mut() = 0;
        }

        let list = List::new(items)
            .block(block)
            .style(self.styles.default_style())
            .highlight_style(self.navigation_highlight_style())
            .highlight_symbol(ui::MODAL_LIST_HIGHLIGHT_FULL)
            .repeat_highlight_symbol(true);

        frame.render_stateful_widget(list, area, &mut self.plan_navigation_state);
    }

    pub(super) fn timeline_block_title(&self) -> Line<'static> {
        let mut spans = Vec::new();
        spans.push(Span::styled(
            ui::NAVIGATION_BLOCK_TITLE.to_owned(),
            self.section_title_style(),
        ));
        spans.push(Span::styled(
            format!(" · {}", ui::NAVIGATION_BLOCK_SHORTCUT_NOTE),
            self.navigation_preview_style(),
        ));

        Line::from(spans)
    }

    pub(super) fn plan_block_title(&self) -> Line<'static> {
        let mut spans = Vec::new();
        spans.push(Span::styled(
            ui::PLAN_BLOCK_TITLE.to_owned(),
            self.section_title_style(),
        ));

        let status = self.plan_status_label();
        spans.push(Span::styled(
            format!(" · {}", status),
            self.navigation_preview_style(),
        ));

        if self.plan.summary.total_steps > 0 {
            spans.push(Span::styled(
                format!(
                    " · {}/{}",
                    self.plan.summary.completed_steps, self.plan.summary.total_steps
                ),
                self.navigation_preview_style(),
            ));
        }

        Line::from(spans)
    }

    fn plan_status_label(&self) -> &'static str {
        match self.plan.summary.status {
            PlanCompletionState::Done => ui::PLAN_STATUS_DONE,
            PlanCompletionState::InProgress => ui::PLAN_STATUS_IN_PROGRESS,
            PlanCompletionState::Empty => ui::PLAN_STATUS_EMPTY,
        }
    }

    fn timeline_navigation_items(&self) -> Vec<ListItem<'static>> {
        if self.lines.is_empty() {
            return vec![ListItem::new(Line::from(vec![Span::styled(
                ui::NAVIGATION_EMPTY_LABEL.to_owned(),
                self.navigation_placeholder_style(),
            )]))];
        }

        self.lines
            .iter()
            .enumerate()
            .map(|(index, line)| ListItem::new(Line::from(self.navigation_spans(index, line))))
            .collect()
    }

    fn plan_navigation_items(&self) -> Vec<ListItem<'static>> {
        self.plan
            .steps
            .iter()
            .enumerate()
            .map(|(index, step)| ListItem::new(Line::from(self.plan_step_spans(index, step))))
            .collect()
    }

    fn plan_step_spans(&self, index: usize, step: &PlanStep) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let sequence = format!("{}{:02}", ui::NAVIGATION_INDEX_PREFIX, index + 1);
        spans.push(Span::styled(sequence, self.navigation_index_style()));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            step.status.checkbox().to_owned(),
            self.plan_checkbox_style(step.status.clone()),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            step.step.clone(),
            self.plan_step_style(step.status.clone()),
        ));
        if matches!(step.status, StepStatus::InProgress) {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("({})", ui::PLAN_IN_PROGRESS_NOTE),
                self.plan_status_note_style(),
            ));
        }
        spans
    }

    fn navigation_spans(&self, index: usize, line: &MessageLine) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let sequence = format!("{}{:02}", ui::NAVIGATION_INDEX_PREFIX, index + 1);
        spans.push(Span::styled(sequence, self.navigation_index_style()));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            self.navigation_label(line.kind).to_owned(),
            self.navigation_label_style(line.kind),
        ));
        let preview = self.navigation_preview_text(line);
        if !preview.is_empty() {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(preview, self.navigation_preview_style()));
        }
        spans
    }

    fn navigation_label(&self, kind: InlineMessageKind) -> &'static str {
        match kind {
            InlineMessageKind::Agent => ui::NAVIGATION_LABEL_AGENT,
            InlineMessageKind::Error => ui::NAVIGATION_LABEL_ERROR,
            InlineMessageKind::Info => ui::NAVIGATION_LABEL_INFO,
            InlineMessageKind::Policy => ui::NAVIGATION_LABEL_POLICY,
            InlineMessageKind::Tool => ui::NAVIGATION_LABEL_TOOL,
            InlineMessageKind::User => ui::NAVIGATION_LABEL_USER,
            InlineMessageKind::Pty => ui::NAVIGATION_LABEL_PTY,
        }
    }

    fn navigation_preview_text(&self, line: &MessageLine) -> String {
        let mut preview = String::new();
        let mut char_count = 0usize;
        let mut truncated = false;
        for segment in &line.segments {
            let sanitized: String = segment
                .text
                .chars()
                .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
                .collect();
            let trimmed = sanitized.trim();
            if trimmed.is_empty() {
                continue;
            }
            if char_count > 0 {
                if char_count + 1 > ui::INLINE_PREVIEW_MAX_CHARS {
                    truncated = true;
                    break;
                }
                preview.push(' ');
                char_count += 1;
            }
            for character in trimmed.chars() {
                if char_count == ui::INLINE_PREVIEW_MAX_CHARS {
                    truncated = true;
                    break;
                }
                preview.push(character);
                char_count += 1;
            }
            if truncated {
                break;
            }
        }

        if truncated {
            preview.push_str(ui::INLINE_PREVIEW_ELLIPSIS);
        }

        preview
    }

    fn navigation_index_style(&self) -> Style {
        self.header_secondary_style().add_modifier(Modifier::DIM)
    }

    fn navigation_label_style(&self, kind: InlineMessageKind) -> Style {
        let mut style = InlineTextStyle::default()
            .with_color(self.text_fallback(kind).or(self.theme.foreground));
        if matches!(kind, InlineMessageKind::Agent | InlineMessageKind::User) {
            style = style.bold();
        }
        ratatui_style_from_inline(&style, self.theme.foreground)
    }

    fn navigation_preview_style(&self) -> Style {
        self.styles.default_style().add_modifier(Modifier::DIM)
    }

    fn navigation_placeholder_style(&self) -> Style {
        self.styles.default_style().add_modifier(Modifier::DIM)
    }

    fn plan_checkbox_style(&self, status: StepStatus) -> Style {
        match status {
            StepStatus::Completed => self.navigation_preview_style(),
            StepStatus::InProgress => self.styles.accent_style().add_modifier(Modifier::BOLD),
            StepStatus::Pending => self.styles.default_style(),
        }
    }

    fn plan_step_style(&self, status: StepStatus) -> Style {
        match status {
            StepStatus::Completed => self.navigation_preview_style(),
            StepStatus::InProgress => self.styles.accent_style().add_modifier(Modifier::BOLD),
            StepStatus::Pending => self.styles.default_style(),
        }
    }

    fn plan_status_note_style(&self) -> Style {
        self.navigation_preview_style()
    }

    fn navigation_highlight_style(&self) -> Style {
        let mut style = Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        if let Some(primary) = self.theme.primary.or(self.theme.secondary) {
            style = style.fg(ratatui_color_from_ansi(primary));
        }
        style
    }

    fn plan_selected_index(&self) -> Option<usize> {
        if self.plan.steps.is_empty() {
            return None;
        }

        if let Some(index) = self
            .plan
            .steps
            .iter()
            .position(|step| matches!(step.status, StepStatus::InProgress))
        {
            return Some(index);
        }

        if let Some(index) = self
            .plan
            .steps
            .iter()
            .position(|step| matches!(step.status, StepStatus::Pending))
        {
            return Some(index);
        }

        Some(self.plan.steps.len().saturating_sub(1))
    }

    pub(super) fn should_show_plan(&self) -> bool {
        self.plan.summary.status != PlanCompletionState::Empty && !self.plan.steps.is_empty()
    }
}
