use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear as ClearWidget, List, ListItem, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use std::cmp;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::ui::slash::SlashCommandInfo;

use super::state::{
    AppLayout, InputDisplay, InputLayout, MAX_SLASH_SUGGESTIONS, MESSAGE_INDENT, MessageBlock,
    PTY_CONTENT_VIEW_LINES, PtyPlacement, RatatuiLoop, RatatuiMessageKind, RatatuiSegment,
    RatatuiTextStyle, StyledLine, TranscriptDisplay,
};
use super::ui::PtyBlockBuilder;

impl RatatuiLoop {
    fn render_slash_suggestions(&mut self, frame: &mut Frame, area: Rect) {
        if !self.slash_suggestions.is_visible() {
            return;
        }
        if area.width <= 2 || area.height < 3 {
            return;
        }

        let capacity = cmp::min(
            MAX_SLASH_SUGGESTIONS,
            area.height.saturating_sub(2) as usize,
        );
        if capacity == 0 {
            return;
        }

        let items: Vec<&SlashCommandInfo> = self
            .slash_suggestions
            .items()
            .iter()
            .take(capacity)
            .copied()
            .collect();
        if items.is_empty() {
            return;
        }

        if let Some(selected) = self.slash_suggestions.selected_index() {
            if selected >= items.len() {
                let clamped = items.len().saturating_sub(1);
                self.slash_suggestions.list_state().select(Some(clamped));
            }
        }

        let max_name_len = items.iter().map(|info| info.name.len()).max().unwrap_or(0);
        let entries: Vec<String> = items
            .iter()
            .map(|info| {
                let mut line = format!("/{:<width$}", info.name, width = max_name_len);
                line.push(' ');
                line.push_str(info.description);
                line
            })
            .collect();

        let max_width = entries
            .iter()
            .map(|value| UnicodeWidthStr::width(value.as_str()))
            .max()
            .unwrap_or(0);
        let visible_height = entries.len().min(capacity) as u16 + 2;
        let height = visible_height.min(area.height);
        let required_width = cmp::max(4, cmp::min(area.width as usize, max_width + 4)) as u16;
        let suggestion_area = Rect::new(area.x, area.y, required_width, height);
        frame.render_widget(ClearWidget, suggestion_area);

        let list_items: Vec<ListItem> = entries.into_iter().map(ListItem::new).collect();
        let border_style = Style::default().fg(self.theme.primary.unwrap_or(Color::LightBlue));
        let list = List::new(list_items)
            .block(
                Block::default()
                    .title(Line::from("? help · / commands"))
                    .borders(Borders::ALL)
                    .border_style(border_style),
            )
            .highlight_style(
                Style::default()
                    .fg(self.theme.primary.unwrap_or(Color::LightBlue))
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_stateful_widget(list, suggestion_area, self.slash_suggestions.list_state());
    }

    fn highlight_transcript(
        &self,
        lines: Vec<Line<'static>>,
        _offset: usize,
    ) -> Vec<Line<'static>> {
        if self.selection.range().is_none() {
            return lines;
        }

        lines
    }

    fn update_pty_area(&mut self, text_area: Rect) {
        let Some(placement) = self.pty_block else {
            self.pty_area = None;
            return;
        };
        if placement.height == 0 || text_area.height == 0 {
            self.pty_area = None;
            return;
        }

        let offset = self.transcript_scroll.offset();
        let viewport = self.transcript_scroll.viewport_height();
        let start = placement.top;
        let end = placement.top + placement.height;
        let view_start = offset;
        let view_end = offset + viewport;
        if end <= view_start || start >= view_end {
            self.pty_area = None;
            return;
        }

        let visible_start = start.max(view_start) - view_start;
        let visible_end = end.min(view_end) - view_start;
        if visible_end <= visible_start {
            self.pty_area = None;
            return;
        }

        let indent = placement.indent.min(text_area.width as usize) as u16;
        let x = text_area.x.saturating_add(indent);
        let width = text_area.width.saturating_sub(indent);
        let y = text_area.y + visible_start as u16;
        let height = (visible_end - visible_start) as u16;
        if width == 0 || height == 0 {
            self.pty_area = None;
            return;
        }

        self.pty_area = Some(Rect::new(x, y, width, height));
    }

    pub(crate) fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        if area.width == 0 || area.height == 0 {
            return;
        }

        let AppLayout {
            message: message_area,
            input: input_layout,
            status: status_area,
        } = self.build_app_layout(area);

        let foreground_style = self
            .theme
            .foreground
            .map(|fg| Style::default().fg(fg))
            .unwrap_or_default();

        let mut scrollbar_area = None;

        if message_area.width > 0 && message_area.height > 0 {
            let viewport_height = usize::from(message_area.height);
            let mut display = self.build_display(message_area.width);
            let preserve_transcript = !self.transcript_autoscroll;
            self.transcript_scroll.update_bounds(
                display.total_height,
                viewport_height,
                preserve_transcript,
            );
            if !self.transcript_scroll.is_at_bottom() {
                self.transcript_autoscroll = false;
            }
            if self.transcript_autoscroll {
                self.transcript_scroll.scroll_to_bottom();
                self.transcript_autoscroll = false;
            }

            let mut needs_scrollbar =
                self.transcript_scroll.has_overflow() && message_area.width > 1;
            let text_area = if needs_scrollbar {
                let adjusted_width = message_area.width.saturating_sub(1);
                display = self.build_display(adjusted_width);
                let preserve_transcript = !self.transcript_autoscroll;
                self.transcript_scroll.update_bounds(
                    display.total_height,
                    viewport_height,
                    preserve_transcript,
                );
                if !self.transcript_scroll.is_at_bottom() {
                    self.transcript_autoscroll = false;
                }
                if self.transcript_autoscroll {
                    self.transcript_scroll.scroll_to_bottom();
                    self.transcript_autoscroll = false;
                }
                needs_scrollbar = self.transcript_scroll.has_overflow();
                if needs_scrollbar {
                    let segments = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Length(adjusted_width), Constraint::Length(1)])
                        .split(message_area);
                    scrollbar_area = Some(segments[1]);
                    segments[0]
                } else {
                    message_area
                }
            } else {
                message_area
            };

            self.transcript_area = Some(text_area);

            let offset = self.transcript_scroll.offset();
            let highlighted = self.highlight_transcript(display.lines.clone(), offset);
            let mut paragraph = Paragraph::new(highlighted).alignment(Alignment::Left);
            if offset > 0 {
                paragraph = paragraph.scroll((offset as u16, 0));
            }
            paragraph = paragraph.style(foreground_style);
            frame.render_widget(paragraph, text_area);
            self.update_pty_area(text_area);

            if let Some(scroll_area) = scrollbar_area {
                if self.transcript_scroll.has_overflow() && scroll_area.width > 0 {
                    let mut scrollbar_state =
                        ScrollbarState::new(self.transcript_scroll.content_height())
                            .viewport_content_length(self.transcript_scroll.viewport_height())
                            .position(self.transcript_scroll.offset());
                    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
                    frame.render_stateful_widget(scrollbar, scroll_area, &mut scrollbar_state);
                }
            }
        } else {
            self.transcript_scroll.update_bounds(0, 0, false);
            self.transcript_area = Some(message_area);
        }

        if let Some(layout) = input_layout {
            let InputLayout {
                block_area,
                suggestion_area,
                display,
            } = layout;
            if block_area.width > 2 && block_area.height >= 3 {
                let accent = self
                    .theme
                    .secondary
                    .or(self.theme.foreground)
                    .unwrap_or(Color::DarkGray);
                let line_style = Style::default().fg(accent).add_modifier(Modifier::DIM);
                let horizontal = "─".repeat(block_area.width as usize);

                let top_area = Rect::new(block_area.x, block_area.y, block_area.width, 1);
                let bottom_y = block_area.y + block_area.height.saturating_sub(1);
                let bottom_area = Rect::new(block_area.x, bottom_y, block_area.width, 1);
                let top_line = Paragraph::new(Line::from(vec![Span::styled(
                    horizontal.clone(),
                    line_style,
                )]));
                frame.render_widget(top_line, top_area);
                let bottom_line =
                    Paragraph::new(Line::from(vec![Span::styled(horizontal, line_style)]));
                frame.render_widget(bottom_line, bottom_area);

                let input_height = block_area.height.saturating_sub(2);
                if input_height > 0 {
                    let input_area = Rect::new(
                        block_area.x,
                        block_area.y + 1,
                        block_area.width,
                        input_height,
                    );
                    let paragraph = Paragraph::new(display.lines.clone())
                        .wrap(Wrap { trim: false })
                        .style(foreground_style);
                    frame.render_widget(paragraph, input_area);

                    if let Some(area) = suggestion_area {
                        if area.width > 0 && area.height > 0 {
                            self.render_slash_suggestions(frame, area);
                        }
                    } else if input_area.width > 0 && input_area.height > 0 {
                        self.render_slash_suggestions(frame, input_area);
                    }

                    if self.cursor_visible {
                        if let Some((row, col)) = display.cursor {
                            if row < input_area.height && col < input_area.width {
                                let cursor_x = input_area.x + col;
                                let cursor_y = input_area.y + row;
                                frame.set_cursor_position((cursor_x, cursor_y));
                            }
                        }
                    }
                }
            }
        }

        if let Some(status_area) = status_area {
            if status_area.width > 0 {
                let left_text = self.status_bar.left.clone();
                let center_text = self.status_bar.center.clone();
                let right_text = self.status_bar.right.clone();

                let mut left_len = UnicodeWidthStr::width(left_text.as_str()) as u16;
                let mut right_len = UnicodeWidthStr::width(right_text.as_str()) as u16;
                if left_len > status_area.width {
                    left_len = status_area.width;
                }
                if right_len > status_area.width.saturating_sub(left_len) {
                    right_len = status_area.width.saturating_sub(left_len);
                }
                let center_len = status_area.width.saturating_sub(left_len + right_len);
                let sections = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(left_len),
                        Constraint::Length(center_len),
                        Constraint::Length(right_len),
                    ])
                    .split(status_area);

                let mut status_style = Style::default()
                    .fg(self.theme.foreground.unwrap_or(Color::Gray))
                    .add_modifier(Modifier::DIM);
                if let Some(background) = self.theme.background {
                    status_style = status_style.bg(background);
                }

                if let Some(area) = sections.get(0) {
                    if area.width > 0 {
                        let left = Paragraph::new(Line::from(left_text.clone()))
                            .alignment(Alignment::Left)
                            .style(status_style);
                        frame.render_widget(left, *area);
                    }
                }
                if let Some(area) = sections.get(1) {
                    if area.width > 0 {
                        let center = Paragraph::new(Line::from(center_text.clone()))
                            .alignment(Alignment::Center)
                            .style(status_style);
                        frame.render_widget(center, *area);
                    }
                }
                if let Some(area) = sections.get(2) {
                    if area.width > 0 {
                        let right = Paragraph::new(Line::from(right_text.clone()))
                            .alignment(Alignment::Right)
                            .style(status_style);
                        frame.render_widget(right, *area);
                    }
                }
            }
        }

        if self.pty_block.is_none() {
            self.pty_area = None;
            self.pty_scroll.update_bounds(0, 0, false);
        }
    }

    fn build_app_layout(&self, area: Rect) -> AppLayout {
        if area.width == 0 || area.height == 0 {
            return AppLayout {
                message: Rect::new(area.x, area.y, area.width, 0),
                input: None,
                status: None,
            };
        }

        let (body_area, status_area) = if area.height > 1 {
            let segments = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(1)])
                .split(area);
            (segments[0], Some(segments[1]))
        } else {
            (area, None)
        };

        if body_area.height == 0 {
            return AppLayout {
                message: Rect::new(body_area.x, body_area.y, body_area.width, 0),
                input: None,
                status: status_area,
            };
        }

        let inner_width = body_area.width.saturating_sub(2);
        let display = self.build_input_display(inner_width);
        let block_height = cmp::max(display.height.saturating_add(2), 3);
        let available_for_suggestions = body_area.height.saturating_sub(block_height);
        let suggestion_height = self
            .slash_suggestions
            .visible_height(available_for_suggestions);
        let input_total_height = block_height
            .saturating_add(suggestion_height)
            .min(body_area.height);
        let message_height = body_area.height.saturating_sub(input_total_height);
        let message_area = Rect::new(body_area.x, body_area.y, body_area.width, message_height);

        if input_total_height == 0 {
            return AppLayout {
                message: message_area,
                input: None,
                status: status_area,
            };
        }

        let input_y = body_area.y.saturating_add(message_height);
        let input_container = Rect::new(body_area.x, input_y, body_area.width, input_total_height);
        let block_area_height = block_height.min(input_container.height);
        let block_area = Rect::new(
            input_container.x,
            input_container.y,
            input_container.width,
            block_area_height,
        );
        let suggestion_area = if suggestion_height > 0 && input_container.height > block_area_height
        {
            Some(Rect::new(
                input_container.x,
                input_container.y + block_area_height,
                input_container.width,
                input_container.height.saturating_sub(block_area_height),
            ))
        } else {
            None
        };

        AppLayout {
            message: message_area,
            input: Some(InputLayout {
                block_area,
                suggestion_area,
                display,
            }),
            status: status_area,
        }
    }

    fn build_display(&mut self, width: u16) -> TranscriptDisplay {
        if width == 0 {
            return TranscriptDisplay {
                lines: Vec::new(),
                total_height: 0,
            };
        }

        self.pty_block = None;
        let mut lines = Vec::new();
        let mut total_height = 0usize;
        let width_usize = width as usize;
        let indent_width = MESSAGE_INDENT.min(width_usize);
        let mut first_rendered = true;

        let mut conversation_line_offsets = Vec::new();
        let mut next_conversation = 0usize;

        while next_conversation < self.conversation_offsets.len() {
            let start_index = self.conversation_offsets[next_conversation].min(self.messages.len());
            if start_index == 0 {
                conversation_line_offsets.push(total_height);
                next_conversation += 1;
            } else {
                break;
            }
        }

        for index in 0..self.messages.len() {
            while next_conversation < self.conversation_offsets.len() {
                let start_index =
                    self.conversation_offsets[next_conversation].min(self.messages.len());
                if start_index == index {
                    conversation_line_offsets.push(total_height);
                    next_conversation += 1;
                } else {
                    break;
                }
            }

            let kind = self.messages[index].kind;
            let has_visible = {
                let block = &self.messages[index];
                self.block_has_visible_content(block)
            };
            if !has_visible {
                continue;
            }

            let mut placement = None;
            let mut block_lines = if kind == RatatuiMessageKind::Pty {
                if let Some(lines) = self.build_pty_panel_lines(width_usize, indent_width) {
                    placement = Some(PtyPlacement {
                        top: 0,
                        height: lines.len(),
                        indent: indent_width,
                    });
                    lines
                } else {
                    Vec::new()
                }
            } else {
                let block = &self.messages[index];
                match kind {
                    RatatuiMessageKind::User => self.build_user_block(block, width_usize),
                    RatatuiMessageKind::Info
                    | RatatuiMessageKind::Policy
                    | RatatuiMessageKind::Tool => {
                        self.build_panel_block(block, width_usize, self.kind_color(kind))
                    }
                    _ => self.build_response_block(block, width_usize, kind),
                }
            };

            if block_lines.is_empty() {
                continue;
            }

            if !first_rendered {
                lines.push(Line::default());
                total_height += 1;
            }

            let block_top = total_height;
            total_height += block_lines.len();
            lines.append(&mut block_lines);

            if let Some(mut placement) = placement {
                placement.top = block_top;
                placement.height = total_height.saturating_sub(block_top);
                self.pty_block = Some(placement);
            }

            first_rendered = false;
        }

        while next_conversation < self.conversation_offsets.len() {
            conversation_line_offsets.push(total_height);
            next_conversation += 1;
        }

        if conversation_line_offsets.is_empty() {
            conversation_line_offsets.push(0);
        }

        if !lines.is_empty() {
            lines.push(Line::default());
            total_height += 1;
        }

        if let Some(header) = self.conversation_header() {
            lines.insert(0, header);
            lines.insert(1, Line::default());
            total_height += 2;
            for offset in &mut conversation_line_offsets {
                *offset = offset.saturating_add(2);
            }
        }

        self.conversation_line_offsets = conversation_line_offsets;

        TranscriptDisplay {
            lines,
            total_height,
        }
    }

    fn build_input_display(&self, width: u16) -> InputDisplay {
        if width == 0 {
            return InputDisplay {
                lines: vec![Line::default()],
                cursor: None,
                height: 1,
            };
        }

        let width_usize = width as usize;
        let mut lines = self.wrap_segments(
            &self.prompt_segments(),
            width_usize,
            0,
            self.theme.foreground,
        );
        if lines.is_empty() {
            lines.push(Line::default());
        }

        let prefix_width = UnicodeWidthStr::width(self.prompt_prefix.as_str());
        let input_width = if self.show_placeholder {
            0
        } else {
            self.input.width_before_cursor()
        };
        let placeholder_width = if self.show_placeholder {
            self.placeholder_hint
                .as_deref()
                .map(UnicodeWidthStr::width)
                .unwrap_or(0)
        } else {
            0
        };
        let cursor_width = prefix_width + input_width + placeholder_width;
        let line_width = width_usize.max(1);
        let cursor_row = (cursor_width / line_width) as u16;
        let cursor_col = (cursor_width % line_width) as u16;
        let height = lines.len().max(1) as u16;

        InputDisplay {
            lines,
            cursor: Some((cursor_row, cursor_col)),
            height,
        }
    }

    fn block_has_visible_content(&self, block: &MessageBlock) -> bool {
        match block.kind {
            RatatuiMessageKind::Pty | RatatuiMessageKind::Tool | RatatuiMessageKind::Agent => {
                block.lines.iter().any(StyledLine::has_visible_content)
            }
            _ => true,
        }
    }

    fn build_user_block(&self, block: &MessageBlock, width: usize) -> Vec<Line<'static>> {
        let indent = MESSAGE_INDENT.min(width);
        if let Some(panel) = self.build_chat_panel(
            block,
            width,
            indent,
            &self.user_label,
            RatatuiMessageKind::User,
        ) {
            return panel;
        }
        let mut prefix_style = RatatuiTextStyle::default();
        prefix_style.color = Some(self.kind_color(RatatuiMessageKind::User));
        prefix_style.bold = true;
        self.build_prefixed_block(block, width, "❯ ", prefix_style, self.theme.foreground)
    }

    fn build_response_block(
        &self,
        block: &MessageBlock,
        width: usize,
        kind: RatatuiMessageKind,
    ) -> Vec<Line<'static>> {
        let marker = match kind {
            RatatuiMessageKind::Agent | RatatuiMessageKind::Tool => "✦",
            RatatuiMessageKind::Error => "!",
            RatatuiMessageKind::Policy => "ⓘ",
            RatatuiMessageKind::User => "❯",
            _ => "✻",
        };
        let indent = if kind == RatatuiMessageKind::Agent {
            0
        } else {
            MESSAGE_INDENT
        };
        if kind == RatatuiMessageKind::Agent {
            return self.build_agent_block(block, width, marker);
        }
        let prefix = if indent == 0 {
            format!("{} ", marker)
        } else {
            format!("{}{} ", " ".repeat(indent), marker)
        };
        let mut style = RatatuiTextStyle::default();
        style.color = Some(self.kind_color(kind));
        if matches!(kind, RatatuiMessageKind::Agent | RatatuiMessageKind::Error) {
            style.bold = true;
        }
        self.build_prefixed_block(block, width, &prefix, style, self.theme.foreground)
    }

    fn build_agent_block(
        &self,
        block: &MessageBlock,
        width: usize,
        marker: &str,
    ) -> Vec<Line<'static>> {
        let mut header_style = RatatuiTextStyle::default();
        header_style.color = Some(self.kind_color(RatatuiMessageKind::Agent));
        header_style.bold = true;

        let header_text = format!("{} {}", marker, self.agent_label);
        let mut lines = Vec::new();
        lines.push(Line::from(vec![Span::styled(
            header_text,
            header_style.to_style(self.theme.foreground),
        )]));

        let mut body = Vec::new();
        for line in &block.lines {
            let mut wrapped = self.wrap_segments(&line.segments, width, 0, self.theme.foreground);
            if wrapped.is_empty() {
                body.push(Line::default());
            } else {
                body.append(&mut wrapped);
            }
        }
        if body.is_empty() {
            body.push(Line::default());
        }

        lines.extend(body);
        lines
    }

    fn build_chat_panel(
        &self,
        block: &MessageBlock,
        width: usize,
        indent: usize,
        label: &str,
        kind: RatatuiMessageKind,
    ) -> Option<Vec<Line<'static>>> {
        if width == 0 {
            return None;
        }
        let indent = indent.min(width);
        let available = width.saturating_sub(indent);
        if available < 4 {
            return None;
        }
        let inner_width = available.saturating_sub(2);
        if inner_width == 0 {
            return None;
        }
        let accent = self.kind_color(kind);
        let border_style = Style::default().fg(accent);
        let header_style = Style::default().fg(accent).add_modifier(Modifier::BOLD);
        let indent_prefix = if indent > 0 {
            Some(" ".repeat(indent))
        } else {
            None
        };

        let mut wrapped_lines = Vec::new();
        for line in &block.lines {
            let mut lines =
                self.wrap_segments(&line.segments, inner_width, 0, self.theme.foreground);
            if lines.is_empty() {
                wrapped_lines.push(Line::default());
            } else {
                wrapped_lines.append(&mut lines);
            }
        }
        if wrapped_lines.is_empty() {
            wrapped_lines.push(Line::default());
        }

        let mut rendered = Vec::with_capacity(wrapped_lines.len() + 2);
        let mut top_spans = Vec::new();
        if let Some(prefix) = &indent_prefix {
            top_spans.push(Span::raw(prefix.clone()));
        }
        top_spans.push(Span::styled("╭".to_string(), border_style));
        top_spans.extend(self.chat_title_segments(inner_width, label, border_style, header_style));
        top_spans.push(Span::styled("╮".to_string(), border_style));
        rendered.push(Line::from(top_spans));

        for line in wrapped_lines {
            let mut spans = Vec::new();
            if let Some(prefix) = &indent_prefix {
                spans.push(Span::raw(prefix.clone()));
            }
            spans.push(Span::styled("│".to_string(), border_style));
            let mut content_spans = line.spans;
            let mut occupied = 0usize;
            for span in &content_spans {
                occupied += UnicodeWidthStr::width(span.content.as_ref());
            }
            if occupied < inner_width {
                content_spans.push(Span::raw(" ".repeat(inner_width - occupied)));
            }
            spans.extend(content_spans);
            spans.push(Span::styled("│".to_string(), border_style));
            rendered.push(Line::from(spans));
        }

        let mut bottom_spans = Vec::new();
        if let Some(prefix) = indent_prefix {
            bottom_spans.push(Span::raw(prefix));
        }
        bottom_spans.push(Span::styled("╰".to_string(), border_style));
        bottom_spans.push(Span::styled("─".repeat(inner_width), border_style));
        bottom_spans.push(Span::styled("╯".to_string(), border_style));
        rendered.push(Line::from(bottom_spans));

        Some(rendered)
    }

    fn chat_title_segments(
        &self,
        inner_width: usize,
        label: &str,
        border_style: Style,
        header_style: Style,
    ) -> Vec<Span<'static>> {
        if inner_width == 0 {
            return Vec::new();
        }
        let trimmed = label.trim();
        if trimmed.is_empty() || inner_width < 2 {
            return vec![Span::styled("─".repeat(inner_width), border_style)];
        }
        let available = inner_width.saturating_sub(2);
        if available == 0 {
            return vec![Span::styled("─".repeat(inner_width), border_style)];
        }
        let truncated = Self::truncate_to_width(trimmed, available);
        let decorated = format!(" {} ", truncated);
        let decorated_width = UnicodeWidthStr::width(decorated.as_str()).min(inner_width);
        let remaining = inner_width.saturating_sub(decorated_width);
        let left = remaining / 2;
        let right = remaining - left;
        let mut spans = Vec::new();
        if left > 0 {
            spans.push(Span::styled("─".repeat(left), border_style));
        }
        spans.push(Span::styled(decorated, header_style));
        if right > 0 {
            spans.push(Span::styled("─".repeat(right), border_style));
        }
        if spans.is_empty() {
            spans.push(Span::styled("─".repeat(inner_width), border_style));
        }
        spans
    }

    fn build_panel_block(
        &self,
        block: &MessageBlock,
        width: usize,
        accent: Color,
    ) -> Vec<Line<'static>> {
        if width < 4 {
            let mut fallback = Vec::new();
            for line in &block.lines {
                let wrapped = self.wrap_segments(&line.segments, width, 0, self.theme.foreground);
                fallback.extend(wrapped);
            }
            return fallback;
        }

        let border_style = Style::default().fg(accent);
        let horizontal = "─".repeat(width.saturating_sub(2));
        let mut rendered = Vec::new();
        rendered.push(Line::from(vec![Span::styled(
            format!("╭{}╮", horizontal),
            border_style,
        )]));

        let content_width = width.saturating_sub(4);
        let mut emitted = false;
        for line in &block.lines {
            let wrapped =
                self.wrap_segments(&line.segments, content_width, 0, self.theme.foreground);
            if wrapped.is_empty() {
                let mut spans = Vec::new();
                spans.push(Span::styled("│ ", border_style));
                spans.push(Span::raw(" ".repeat(content_width)));
                spans.push(Span::styled(" │", border_style));
                rendered.push(Line::from(spans));
                continue;
            }

            for wrapped_line in wrapped {
                emitted = true;
                let mut spans = Vec::new();
                spans.push(Span::styled("│ ", border_style));
                let mut content_spans = wrapped_line.spans.clone();
                let mut occupied = 0usize;
                for span in &content_spans {
                    occupied += UnicodeWidthStr::width(span.content.as_ref());
                }
                if occupied < content_width {
                    content_spans.push(Span::raw(" ".repeat(content_width - occupied)));
                }
                spans.extend(content_spans);
                spans.push(Span::styled(" │", border_style));
                rendered.push(Line::from(spans));
            }
        }

        if !emitted {
            let mut spans = Vec::new();
            spans.push(Span::styled("│ ", border_style));
            spans.push(Span::raw(" ".repeat(content_width)));
            spans.push(Span::styled(" │", border_style));
            rendered.push(Line::from(spans));
        }

        rendered.push(Line::from(vec![Span::styled(
            format!("╰{}╯", horizontal),
            border_style,
        )]));
        rendered
    }

    fn build_prefixed_block(
        &self,
        block: &MessageBlock,
        width: usize,
        prefix: &str,
        prefix_style: RatatuiTextStyle,
        fallback: Option<Color>,
    ) -> Vec<Line<'static>> {
        if width == 0 {
            return Vec::new();
        }
        let prefix_width = UnicodeWidthStr::width(prefix);
        if prefix_width >= width {
            let mut lines = Vec::new();
            for line in &block.lines {
                let mut spans = Vec::new();
                spans.push(Span::styled(
                    prefix.to_string(),
                    prefix_style.to_style(fallback),
                ));
                lines.push(Line::from(spans));
                let wrapped = self.wrap_segments(&line.segments, width, 0, fallback);
                lines.extend(wrapped);
            }
            if lines.is_empty() {
                lines.push(Line::from(vec![Span::styled(
                    prefix.to_string(),
                    prefix_style.to_style(fallback),
                )]));
            }
            return lines;
        }

        let content_width = width - prefix_width;
        let continuation = " ".repeat(prefix_width);
        let mut rendered = Vec::new();
        let mut first = true;
        for line in &block.lines {
            let wrapped = self.wrap_segments(&line.segments, content_width, 0, fallback);
            if wrapped.is_empty() {
                let prefix_text = if first { prefix } else { continuation.as_str() };
                rendered.push(Line::from(vec![Span::styled(
                    prefix_text.to_string(),
                    prefix_style.to_style(fallback),
                )]));
                first = false;
                continue;
            }

            for (index, wrapped_line) in wrapped.into_iter().enumerate() {
                let prefix_text = if first && index == 0 {
                    prefix
                } else {
                    continuation.as_str()
                };
                let mut spans = Vec::new();
                spans.push(Span::styled(
                    prefix_text.to_string(),
                    prefix_style.to_style(fallback),
                ));
                spans.extend(wrapped_line.spans);
                rendered.push(Line::from(spans));
            }
            first = false;
        }

        if rendered.is_empty() {
            rendered.push(Line::from(vec![Span::styled(
                prefix.to_string(),
                prefix_style.to_style(fallback),
            )]));
        }

        rendered
    }

    fn prompt_segments(&self) -> Vec<RatatuiSegment> {
        let mut segments = Vec::new();
        segments.push(RatatuiSegment {
            text: self.prompt_prefix.clone(),
            style: self.prompt_style.clone(),
        });

        if self.show_placeholder {
            if let Some(hint) = &self.placeholder_hint {
                segments.push(RatatuiSegment {
                    text: hint.clone(),
                    style: self.placeholder_style.clone(),
                });
            }
        } else {
            segments.push(RatatuiSegment {
                text: self.input.value().to_string(),
                style: RatatuiTextStyle::default(),
            });
        }

        segments
    }

    fn wrap_segments(
        &self,
        segments: &[RatatuiSegment],
        width: usize,
        indent: usize,
        fallback: Option<Color>,
    ) -> Vec<Line<'static>> {
        if width == 0 {
            return vec![Line::default()];
        }

        let mut lines = Vec::new();
        let indent_width = indent.min(width);
        let indent_text = " ".repeat(indent_width);
        let mut current = Vec::new();
        let mut current_width = indent_width;

        if indent_width > 0 {
            current.push(Span::raw(indent_text.clone()));
        }

        for segment in segments {
            let style = segment.style.to_style(fallback);
            let mut buffer = String::new();
            let mut buffer_width = 0usize;

            for ch in segment.text.chars() {
                if ch == '\n' {
                    if !buffer.is_empty() {
                        current.push(Span::styled(buffer.clone(), style));
                        buffer.clear();
                        buffer_width = 0;
                    }
                    lines.push(Line::from(current));
                    current = Vec::new();
                    if indent_width > 0 {
                        current.push(Span::raw(indent_text.clone()));
                    }
                    current_width = indent_width;
                    continue;
                }

                let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
                if ch_width == 0 {
                    buffer.push(ch);
                    continue;
                }

                if current_width + buffer_width + ch_width > width
                    && current_width + buffer_width > indent_width
                {
                    if !buffer.is_empty() {
                        current.push(Span::styled(buffer.clone(), style));
                        buffer.clear();
                        buffer_width = 0;
                    }
                    lines.push(Line::from(current));
                    current = Vec::new();
                    if indent_width > 0 {
                        current.push(Span::raw(indent_text.clone()));
                    }
                    current_width = indent_width;
                }

                buffer.push(ch);
                buffer_width += ch_width;
            }

            if !buffer.is_empty() {
                current.push(Span::styled(buffer.clone(), style));
                current_width += buffer_width;
                buffer.clear();
            }
        }

        if current.is_empty() {
            if indent_width > 0 {
                current.push(Span::raw(indent_text));
            }
        }

        lines.push(Line::from(current));
        lines
    }

    fn build_pty_panel_lines(&mut self, width: usize, indent: usize) -> Option<Vec<Line<'static>>> {
        let Some(panel) = self.pty_panel.as_mut() else {
            self.pty_scroll.update_bounds(0, 0, false);
            return None;
        };
        if !panel.has_content() {
            self.pty_scroll.update_bounds(0, 0, false);
            return None;
        }
        if width <= indent + 2 {
            self.pty_scroll.update_bounds(0, 0, false);
            return None;
        }
        let available = width - indent;
        if available < 3 {
            self.pty_scroll.update_bounds(0, 0, false);
            return None;
        }
        let inner_width = available.saturating_sub(2);
        if inner_width == 0 {
            self.pty_scroll.update_bounds(0, 0, false);
            return None;
        }

        let title = panel.block_title_text();
        let text = panel.view_text();
        let mut wrapped = self.wrap_pty_text(&text, inner_width);
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        let total_content = wrapped.len().max(1);
        let viewport = cmp::min(total_content, cmp::max(PTY_CONTENT_VIEW_LINES, 1));
        let preserve_pty = !self.pty_autoscroll;
        self.pty_scroll
            .update_bounds(total_content, viewport, preserve_pty);
        if self.pty_autoscroll {
            self.pty_scroll.scroll_to_bottom();
            self.pty_autoscroll = false;
        }

        let offset = self.pty_scroll.offset();
        let mut visible: Vec<Line<'static>> =
            wrapped.into_iter().skip(offset).take(viewport).collect();
        while visible.len() < viewport {
            visible.push(Line::default());
        }

        let title_bar = self.compose_pty_title_bar(inner_width, &title);
        let builder = PtyBlockBuilder::new(&self.theme, indent, inner_width).title(title_bar);
        Some(builder.build(visible))
    }

    fn wrap_pty_text(&self, text: &Text<'static>, inner_width: usize) -> Vec<Line<'static>> {
        if inner_width == 0 {
            return vec![Line::default()];
        }
        if text.lines.is_empty() {
            return vec![Line::default()];
        }

        let mut wrapped = Vec::new();
        for raw in &text.lines {
            let segments: Vec<RatatuiSegment> = raw
                .spans
                .iter()
                .map(|span| RatatuiSegment {
                    text: span.content.to_string(),
                    style: Self::style_to_text_style(span.style),
                })
                .collect();
            let mut lines = self.wrap_segments(&segments, inner_width, 0, self.theme.foreground);
            if lines.is_empty() {
                lines.push(Line::default());
            }
            wrapped.append(&mut lines);
        }

        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }
        wrapped
    }

    fn compose_pty_title_bar(&self, inner_width: usize, title: &str) -> String {
        if inner_width == 0 {
            return String::new();
        }
        let trimmed = title.trim();
        if trimmed.is_empty() || inner_width < 2 {
            return "─".repeat(inner_width);
        }
        let available = inner_width.saturating_sub(2);
        if available == 0 {
            return "─".repeat(inner_width);
        }
        let truncated = Self::truncate_to_width(trimmed, available);
        let decorated = format!(" {} ", truncated);
        let decorated_width = UnicodeWidthStr::width(decorated.as_str()).min(inner_width);
        let remaining = inner_width.saturating_sub(decorated_width);
        let left = remaining / 2;
        let right = remaining - left;
        format!("{}{}{}", "─".repeat(left), decorated, "─".repeat(right),)
    }

    fn truncate_to_width(text: &str, max_width: usize) -> String {
        if max_width == 0 {
            return String::new();
        }
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return String::new();
        }
        if UnicodeWidthStr::width(trimmed) <= max_width {
            return trimmed.to_string();
        }
        let mut result = String::new();
        let mut width_used = 0usize;
        let limit = max_width.saturating_sub(1);
        for ch in trimmed.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if ch_width == 0 {
                continue;
            }
            if width_used + ch_width > limit {
                break;
            }
            result.push(ch);
            width_used += ch_width;
        }
        if result.is_empty() {
            "…".to_string()
        } else {
            result.push('…');
            result
        }
    }

    fn style_to_text_style(style: Style) -> RatatuiTextStyle {
        let mut text_style = RatatuiTextStyle::default();
        text_style.color = style.fg;
        if style.add_modifier.contains(Modifier::BOLD) {
            text_style.bold = true;
        }
        if style.add_modifier.contains(Modifier::ITALIC) {
            text_style.italic = true;
        }
        text_style
    }

    fn kind_color(&self, kind: RatatuiMessageKind) -> Color {
        match kind {
            RatatuiMessageKind::Agent => self.theme.primary.unwrap_or(Color::LightCyan),
            RatatuiMessageKind::User => self.theme.secondary.unwrap_or(Color::LightGreen),
            RatatuiMessageKind::Tool => self.theme.foreground.unwrap_or(Color::LightMagenta),
            RatatuiMessageKind::Pty => self.theme.primary.unwrap_or(Color::LightBlue),
            RatatuiMessageKind::Info => self.theme.foreground.unwrap_or(Color::Yellow),
            RatatuiMessageKind::Policy => self.theme.secondary.unwrap_or(Color::LightYellow),
            RatatuiMessageKind::Error => Color::LightRed,
        }
    }
}
