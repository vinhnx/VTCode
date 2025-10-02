use std::{cmp::min, mem, ptr};

use anstyle::{AnsiColor, Color as AnsiColorEnum, RgbColor};
use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use tokio::sync::mpsc::UnboundedSender;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::types::{
    InlineCommand, InlineEvent, InlineHeaderContext, InlineMessageKind, InlineSegment,
    InlineTextStyle, InlineTheme,
};
use crate::config::constants::ui;
use crate::ui::slash::{SlashCommandInfo, suggestions_for};

const USER_PREFIX: &str = "❯ ";
const PLACEHOLDER_COLOR: RgbColor = RgbColor(0xD3, 0xD3, 0xD3);

#[derive(Clone)]
struct MessageLine {
    kind: InlineMessageKind,
    segments: Vec<InlineSegment>,
}

#[derive(Clone, Default)]
struct MessageLabels {
    agent: Option<String>,
    user: Option<String>,
}

#[derive(Clone)]
struct ModalState {
    title: String,
    lines: Vec<String>,
    restore_input: bool,
    restore_cursor: bool,
}

fn ratatui_color_from_ansi(color: AnsiColorEnum) -> Color {
    match color {
        AnsiColorEnum::Ansi(base) => match base {
            AnsiColor::Black => Color::Black,
            AnsiColor::Red => Color::Red,
            AnsiColor::Green => Color::Green,
            AnsiColor::Yellow => Color::Yellow,
            AnsiColor::Blue => Color::Blue,
            AnsiColor::Magenta => Color::Magenta,
            AnsiColor::Cyan => Color::Cyan,
            AnsiColor::White => Color::White,
            AnsiColor::BrightBlack => Color::DarkGray,
            AnsiColor::BrightRed => Color::LightRed,
            AnsiColor::BrightGreen => Color::LightGreen,
            AnsiColor::BrightYellow => Color::LightYellow,
            AnsiColor::BrightBlue => Color::LightBlue,
            AnsiColor::BrightMagenta => Color::LightMagenta,
            AnsiColor::BrightCyan => Color::LightCyan,
            AnsiColor::BrightWhite => Color::Gray,
        },
        AnsiColorEnum::Ansi256(value) => Color::Indexed(value.index()),
        AnsiColorEnum::Rgb(RgbColor(red, green, blue)) => Color::Rgb(red, green, blue),
    }
}

fn ratatui_style_from_inline(style: &InlineTextStyle, fallback: Option<AnsiColorEnum>) -> Style {
    let mut resolved = Style::default();
    if let Some(color) = style.color.or(fallback) {
        resolved = resolved.fg(ratatui_color_from_ansi(color));
    }
    if style.bold {
        resolved = resolved.add_modifier(Modifier::BOLD);
    }
    if style.italic {
        resolved = resolved.add_modifier(Modifier::ITALIC);
    }
    resolved
}

pub struct Session {
    lines: Vec<MessageLine>,
    theme: InlineTheme,
    header_context: InlineHeaderContext,
    labels: MessageLabels,
    prompt_prefix: String,
    prompt_style: InlineTextStyle,
    placeholder: Option<String>,
    placeholder_style: Option<InlineTextStyle>,
    input: String,
    cursor: usize,
    slash_suggestions: Vec<&'static SlashCommandInfo>,
    slash_selected: Option<usize>,
    slash_list_state: ListState,
    slash_visible_rows: usize,
    navigation_state: ListState,
    input_enabled: bool,
    cursor_visible: bool,
    needs_redraw: bool,
    should_exit: bool,
    view_rows: u16,
    scroll_offset: usize,
    transcript_rows: u16,
    transcript_width: u16,
    transcript_state: ListState,
    cached_max_scroll_offset: usize,
    scroll_metrics_dirty: bool,
    modal: Option<ModalState>,
}

impl Session {
    pub fn new(theme: InlineTheme, placeholder: Option<String>, view_rows: u16) -> Self {
        let resolved_rows = view_rows.max(2);
        let reserved_rows = ui::INLINE_HEADER_HEIGHT + ui::INLINE_INPUT_HEIGHT;
        let initial_transcript_rows = resolved_rows.saturating_sub(reserved_rows).max(1);
        Self {
            lines: Vec::new(),
            theme,
            header_context: InlineHeaderContext::default(),
            labels: MessageLabels::default(),
            prompt_prefix: USER_PREFIX.to_string(),
            prompt_style: InlineTextStyle::default(),
            placeholder,
            placeholder_style: None,
            input: String::new(),
            cursor: 0,
            slash_suggestions: Vec::new(),
            slash_selected: None,
            slash_list_state: ListState::default(),
            slash_visible_rows: 0,
            navigation_state: ListState::default(),
            input_enabled: true,
            cursor_visible: true,
            needs_redraw: true,
            should_exit: false,
            view_rows: resolved_rows,
            scroll_offset: 0,
            transcript_rows: initial_transcript_rows,
            transcript_width: 0,
            transcript_state: ListState::default(),
            cached_max_scroll_offset: 0,
            scroll_metrics_dirty: true,
            modal: None,
        }
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    pub fn request_exit(&mut self) {
        self.should_exit = true;
    }

    pub fn take_redraw(&mut self) -> bool {
        if self.needs_redraw {
            self.needs_redraw = false;
            true
        } else {
            false
        }
    }

    pub fn handle_command(&mut self, command: InlineCommand) {
        match command {
            InlineCommand::AppendLine { kind, segments } => {
                self.push_line(kind, segments);
            }
            InlineCommand::Inline { kind, segment } => {
                self.append_inline(kind, segment);
            }
            InlineCommand::ReplaceLast { count, kind, lines } => {
                self.replace_last(count, kind, lines);
            }
            InlineCommand::SetPrompt { prefix, style } => {
                self.prompt_prefix = prefix;
                self.prompt_style = style;
            }
            InlineCommand::SetPlaceholder { hint, style } => {
                self.placeholder = hint;
                self.placeholder_style = style;
            }
            InlineCommand::SetMessageLabels { agent, user } => {
                self.labels.agent = agent.filter(|label| !label.is_empty());
                self.labels.user = user.filter(|label| !label.is_empty());
            }
            InlineCommand::SetHeaderContext { context } => {
                self.header_context = context;
                self.needs_redraw = true;
            }
            InlineCommand::SetTheme { theme } => {
                self.theme = theme;
            }
            InlineCommand::SetCursorVisible(value) => {
                self.cursor_visible = value;
            }
            InlineCommand::SetInputEnabled(value) => {
                self.input_enabled = value;
                self.update_slash_suggestions();
            }
            InlineCommand::SetInput(content) => {
                self.input = content;
                self.cursor = self.input.len();
                self.update_slash_suggestions();
            }
            InlineCommand::ClearInput => {
                self.clear_input();
            }
            InlineCommand::ForceRedraw => {
                self.mark_dirty();
            }
            InlineCommand::ShowModal { title, lines } => {
                self.show_modal(title, lines);
            }
            InlineCommand::CloseModal => {
                self.close_modal();
            }
            InlineCommand::Shutdown => {
                self.request_exit();
            }
        }
        self.mark_dirty();
    }

    pub fn handle_event(&mut self, event: CrosstermEvent, events: &UnboundedSender<InlineEvent>) {
        match event {
            CrosstermEvent::Key(key) => {
                if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                    if let Some(outbound) = self.process_key(key) {
                        let _ = events.send(outbound);
                    }
                }
            }
            CrosstermEvent::Resize(_, rows) => {
                self.apply_view_rows(rows);
                self.mark_dirty();
            }
            _ => {}
        }
    }

    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let viewport = frame.area();
        if viewport.height == 0 || viewport.width == 0 {
            return;
        }

        self.apply_view_rows(viewport.height);

        let show_suggestions = self.should_render_slash_suggestions();
        let suggestion_height = self.slash_suggestion_height();
        let mut constraints = vec![
            Constraint::Length(ui::INLINE_HEADER_HEIGHT),
            Constraint::Min(1),
        ];
        if show_suggestions {
            constraints.push(Constraint::Length(suggestion_height));
        }
        constraints.push(Constraint::Length(ui::INLINE_INPUT_HEIGHT));

        let segments = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(viewport);

        let header_area = segments[0];
        let main_area = segments[1];
        let input_index = segments.len().saturating_sub(1);
        let input_area = segments[input_index];
        let suggestion_area = if show_suggestions {
            Some(segments[input_index.saturating_sub(1)])
        } else {
            None
        };

        let nav_percent = ui::INLINE_NAVIGATION_PERCENT as u32;
        let mut nav_width = ((main_area.width as u32 * nav_percent) / 100) as u16;
        nav_width = nav_width.max(ui::INLINE_NAVIGATION_MIN_WIDTH);
        let max_allowed = main_area.width.saturating_sub(ui::INLINE_CONTENT_MIN_WIDTH);
        nav_width = nav_width.min(max_allowed);

        let navigation_constraint = Constraint::Length(nav_width);
        let content_constraint = Constraint::Min(ui::INLINE_CONTENT_MIN_WIDTH);
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([content_constraint, navigation_constraint])
            .split(main_area);

        let transcript_area = main_chunks[0];
        let navigation_area = main_chunks[1];

        self.render_header(frame, header_area);
        self.render_navigation(frame, navigation_area);
        self.render_transcript(frame, transcript_area);
        if let Some(area) = suggestion_area {
            self.render_slash_suggestions(frame, area);
        }
        self.render_input(frame, input_area);
        self.render_modal(frame, viewport);
    }

    fn render_header(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 || area.width == 0 {
            return;
        }

        let block = Block::default()
            .title(self.header_block_title())
            .borders(Borders::ALL)
            .style(self.default_style());
        let content = Paragraph::new(vec![
            self.header_title_line(),
            self.header_meta_line(),
            self.header_hint_line(),
        ])
        .style(self.default_style())
        .wrap(Wrap { trim: true })
        .block(block);

        frame.render_widget(content, area);
    }

    fn render_navigation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 || area.width == 0 {
            return;
        }

        let block = Block::default()
            .title(self.navigation_block_title())
            .borders(Borders::ALL)
            .style(self.default_style());
        let inner = block.inner(area);
        if inner.height == 0 {
            frame.render_widget(block, area);
            return;
        }

        let items = self.navigation_items();
        let item_count = items.len();
        if self.lines.is_empty() {
            self.navigation_state.select(None);
            *self.navigation_state.offset_mut() = 0;
        } else {
            let last_index = self.lines.len().saturating_sub(1);
            self.navigation_state.select(Some(last_index));
            let viewport = inner.height as usize;
            let max_offset = item_count.saturating_sub(viewport);
            *self.navigation_state.offset_mut() = max_offset;
        }

        let list = List::new(items)
            .block(block)
            .style(self.default_style())
            .highlight_style(self.navigation_highlight_style());

        frame.render_stateful_widget(list, area, &mut self.navigation_state);
    }

    fn header_block_title(&self) -> Line<'static> {
        let fallback = InlineHeaderContext::default();
        let label = if self.header_context.version.trim().is_empty() {
            fallback.version
        } else {
            self.header_context.version.clone()
        };
        Line::from(vec![Span::styled(label, self.section_title_style())])
    }

    fn header_title_line(&self) -> Line<'static> {
        let mut spans = Vec::new();
        spans.push(Span::styled(
            self.header_mode_label(),
            self.header_primary_style().add_modifier(Modifier::BOLD),
        ));

        if let Some(reasoning) = self.header_reasoning_value() {
            spans.push(Span::styled(
                ui::HEADER_MODE_PRIMARY_SEPARATOR.to_string(),
                self.header_secondary_style(),
            ));
            spans.push(Span::styled(reasoning, self.header_primary_style()));
        }

        for value in self.header_chain_values() {
            spans.push(Span::styled(
                ui::HEADER_MODE_SECONDARY_SEPARATOR.to_string(),
                self.header_secondary_style(),
            ));
            spans.push(Span::styled(value, self.header_primary_style()));
        }

        Line::from(spans)
    }

    fn header_mode_label(&self) -> String {
        let trimmed = self.header_context.mode.trim();
        if trimmed.is_empty() {
            InlineHeaderContext::default().mode
        } else {
            self.header_context.mode.clone()
        }
    }

    fn header_reasoning_value(&self) -> Option<String> {
        let trimmed = self.header_context.reasoning.trim();
        let value = if trimmed.is_empty() {
            InlineHeaderContext::default().reasoning
        } else {
            self.header_context.reasoning.clone()
        };
        if value.trim().is_empty() {
            None
        } else {
            Some(value)
        }
    }

    fn header_chain_values(&self) -> Vec<String> {
        let defaults = InlineHeaderContext::default();
        let fields = [
            (
                &self.header_context.workspace_trust,
                defaults.workspace_trust,
            ),
            (&self.header_context.tools, defaults.tools),
            (&self.header_context.languages, defaults.languages),
            (&self.header_context.mcp, defaults.mcp),
        ];

        fields
            .into_iter()
            .filter_map(|(value, fallback)| {
                let selected = if value.trim().is_empty() {
                    fallback
                } else {
                    value.clone()
                };
                if selected.trim().is_empty() {
                    None
                } else {
                    Some(selected)
                }
            })
            .collect()
    }

    fn header_meta_line(&self) -> Line<'static> {
        let entries = vec![
            (
                ui::HEADER_STATUS_LABEL,
                self.header_status_value().to_string(),
            ),
            (ui::HEADER_MESSAGES_LABEL, self.lines.len().to_string()),
            (
                ui::HEADER_INPUT_LABEL,
                self.header_input_value().to_string(),
            ),
        ];

        let mut spans = Vec::new();
        for (index, (label, value)) in entries.into_iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw(ui::HEADER_META_SEPARATOR.to_string()));
            }
            self.push_meta_entry(&mut spans, label, value.as_str());
        }

        Line::from(spans)
    }

    fn header_hint_line(&self) -> Line<'static> {
        Line::from(vec![Span::styled(
            ui::HEADER_SHORTCUT_HINT.to_string(),
            self.header_hint_style(),
        )])
    }

    fn push_meta_entry(&self, spans: &mut Vec<Span<'static>>, label: &str, value: &str) {
        spans.push(Span::styled(
            format!("{label}: "),
            self.header_meta_label_style(),
        ));
        spans.push(Span::styled(
            value.to_string(),
            self.header_meta_value_style(),
        ));
    }

    fn header_status_value(&self) -> &'static str {
        if self.input_enabled {
            ui::HEADER_STATUS_ACTIVE
        } else {
            ui::HEADER_STATUS_PAUSED
        }
    }

    fn header_input_value(&self) -> &'static str {
        if self.input_enabled {
            ui::HEADER_INPUT_ENABLED
        } else {
            ui::HEADER_INPUT_DISABLED
        }
    }

    fn section_title_style(&self) -> Style {
        let mut style = self.default_style().add_modifier(Modifier::BOLD);
        if let Some(primary) = self.theme.primary.or(self.theme.foreground) {
            style = style.fg(ratatui_color_from_ansi(primary));
        }
        style
    }

    fn header_primary_style(&self) -> Style {
        let mut style = self.default_style();
        if let Some(primary) = self.theme.primary.or(self.theme.foreground) {
            style = style.fg(ratatui_color_from_ansi(primary));
        }
        style
    }

    fn header_secondary_style(&self) -> Style {
        let mut style = self.default_style();
        if let Some(secondary) = self.theme.secondary.or(self.theme.foreground) {
            style = style.fg(ratatui_color_from_ansi(secondary));
        }
        style
    }

    fn header_hint_style(&self) -> Style {
        self.header_secondary_style().add_modifier(Modifier::DIM)
    }

    fn header_meta_label_style(&self) -> Style {
        self.header_secondary_style().add_modifier(Modifier::BOLD)
    }

    fn header_meta_value_style(&self) -> Style {
        self.header_primary_style()
    }

    fn suggestion_block_title(&self) -> Line<'static> {
        Line::from(vec![Span::styled(
            ui::SUGGESTION_BLOCK_TITLE.to_string(),
            self.section_title_style(),
        )])
    }

    fn navigation_block_title(&self) -> Line<'static> {
        Line::from(vec![Span::styled(
            ui::NAVIGATION_BLOCK_TITLE.to_string(),
            self.section_title_style(),
        )])
    }

    fn navigation_items(&self) -> Vec<ListItem<'static>> {
        if self.lines.is_empty() {
            return vec![ListItem::new(Line::from(vec![Span::styled(
                ui::NAVIGATION_EMPTY_LABEL.to_string(),
                self.navigation_placeholder_style(),
            )]))];
        }

        self.lines
            .iter()
            .enumerate()
            .map(|(index, line)| ListItem::new(Line::from(self.navigation_spans(index, line))))
            .collect()
    }

    fn navigation_spans(&self, index: usize, line: &MessageLine) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let sequence = format!("{}{:02}", ui::NAVIGATION_INDEX_PREFIX, index + 1);
        spans.push(Span::styled(sequence, self.navigation_index_style()));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            self.navigation_label(line.kind).to_string(),
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
            let sanitized = segment.text.replace('\n', " ").replace('\r', " ");
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
        let mut style = InlineTextStyle::default();
        style.color = self.text_fallback(kind).or(self.theme.foreground);
        style.bold = matches!(kind, InlineMessageKind::Agent | InlineMessageKind::User);
        ratatui_style_from_inline(&style, self.theme.foreground)
    }

    fn navigation_preview_style(&self) -> Style {
        self.default_style().add_modifier(Modifier::DIM)
    }

    fn navigation_placeholder_style(&self) -> Style {
        self.default_style().add_modifier(Modifier::DIM)
    }

    fn navigation_highlight_style(&self) -> Style {
        let mut style = Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        if let Some(primary) = self.theme.primary.or(self.theme.secondary) {
            style = style.fg(ratatui_color_from_ansi(primary));
        }
        style
    }

    fn apply_view_rows(&mut self, rows: u16) {
        let resolved = rows.max(2);
        if self.view_rows != resolved {
            self.view_rows = resolved;
            self.invalidate_scroll_metrics();
        }
        self.recalculate_transcript_rows();
        self.enforce_scroll_bounds();
    }

    #[cfg(test)]
    fn force_view_rows(&mut self, rows: u16) {
        self.apply_view_rows(rows);
    }

    fn apply_transcript_rows(&mut self, rows: u16) {
        let resolved = rows.max(1);
        if self.transcript_rows != resolved {
            self.transcript_rows = resolved;
            self.invalidate_scroll_metrics();
        }
    }

    fn apply_transcript_width(&mut self, width: u16) {
        if self.transcript_width != width {
            self.transcript_width = width;
            self.invalidate_scroll_metrics();
        }
    }

    fn render_transcript(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 || area.width == 0 {
            return;
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .style(self.default_style());
        let inner = block.inner(area);
        if inner.height == 0 || inner.width == 0 {
            frame.render_widget(block, area);
            return;
        }

        self.apply_transcript_rows(inner.height);
        self.apply_transcript_width(inner.width);

        let viewport_rows = inner.height as usize;
        let (items, top_offset) = self.prepare_transcript_list(inner.width, viewport_rows);
        let vertical_offset = top_offset.min(self.cached_max_scroll_offset);
        *self.transcript_state.offset_mut() = vertical_offset;

        let list = List::new(items).block(block).style(self.default_style());
        frame.render_stateful_widget(list, area, &mut self.transcript_state);
    }

    fn render_slash_suggestions(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 || self.visible_slash_suggestions().is_empty() {
            return;
        }
        let block = Block::default()
            .title(self.suggestion_block_title())
            .borders(Borders::ALL)
            .style(self.default_style());
        let inner = block.inner(area);
        if inner.height == 0 {
            frame.render_widget(block, area);
            return;
        }

        self.slash_visible_rows = inner.height as usize;
        self.sync_slash_state();

        let list = List::new(self.slash_list_items())
            .block(block)
            .style(self.default_style())
            .highlight_style(self.slash_highlight_style());

        frame.render_stateful_widget(list, area, &mut self.slash_list_state);
    }

    fn render_input(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 {
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(self.default_style());
        let inner = block.inner(area);
        let paragraph = Paragraph::new(self.render_input_line())
            .style(self.default_style())
            .wrap(Wrap { trim: false })
            .block(block);
        frame.render_widget(paragraph, area);

        if self.cursor_should_be_visible() && inner.width > 0 {
            let (x, y) = self.cursor_position(inner);
            frame.set_cursor_position((x, y));
        }
    }

    fn transcript_lines(&self) -> Vec<Line<'static>> {
        if self.lines.is_empty() {
            return vec![Line::default()];
        }

        self.lines
            .iter()
            .map(|line| Line::from(self.render_message_spans(line)))
            .collect()
    }

    fn render_input_line(&self) -> Line<'static> {
        let mut spans = Vec::new();
        let prompt_style = ratatui_style_from_inline(&self.prompt_style, self.theme.foreground);
        spans.push(Span::styled(self.prompt_prefix.clone(), prompt_style));

        if self.input.is_empty() {
            if let Some(placeholder) = &self.placeholder {
                let placeholder_style =
                    self.placeholder_style
                        .clone()
                        .unwrap_or_else(|| InlineTextStyle {
                            color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                            ..InlineTextStyle::default()
                        });
                let style = ratatui_style_from_inline(
                    &placeholder_style,
                    Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                );
                spans.push(Span::styled(placeholder.clone(), style));
            }
        } else {
            let style =
                ratatui_style_from_inline(&InlineTextStyle::default(), self.theme.foreground);
            spans.push(Span::styled(self.input.clone(), style));
        }

        Line::from(spans)
    }

    fn should_render_slash_suggestions(&self) -> bool {
        !self.visible_slash_suggestions().is_empty()
    }

    fn slash_suggestion_height(&self) -> u16 {
        let suggestions = self.visible_slash_suggestions();
        if suggestions.is_empty() {
            0
        } else {
            let visible = min(suggestions.len(), ui::SLASH_SUGGESTION_LIMIT);
            visible as u16 + 2
        }
    }

    fn visible_slash_suggestions(&self) -> &[&'static SlashCommandInfo] {
        &self.slash_suggestions
    }

    fn slash_list_items(&self) -> Vec<ListItem<'static>> {
        let command_style = self.slash_name_style();
        let description_style = self.slash_description_style();
        self.visible_slash_suggestions()
            .iter()
            .map(|info| {
                let mut spans = Vec::new();
                spans.push(Span::styled(format!("/{}", info.name), command_style));
                if !info.description.is_empty() {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        info.description.to_string(),
                        description_style,
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect()
    }

    fn slash_highlight_style(&self) -> Style {
        let highlight = self
            .theme
            .primary
            .or(self.theme.secondary)
            .or(self.theme.foreground);
        let mut style = Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED);
        if let Some(color) = highlight {
            style = style.fg(ratatui_color_from_ansi(color));
        }
        style
    }

    fn slash_name_style(&self) -> Style {
        let color = self.theme.primary.or(self.theme.foreground);
        let mut style = Style::default().add_modifier(Modifier::BOLD);
        if let Some(color) = color {
            style = style.fg(ratatui_color_from_ansi(color));
        }
        style
    }

    fn slash_description_style(&self) -> Style {
        let color = self.theme.secondary.or(self.theme.foreground);
        let mut style = Style::default().add_modifier(Modifier::DIM);
        if let Some(color) = color {
            style = style.fg(ratatui_color_from_ansi(color));
        }
        style
    }

    fn input_reserved_rows(&self) -> u16 {
        ui::INLINE_HEADER_HEIGHT + ui::INLINE_INPUT_HEIGHT + self.slash_suggestion_height()
    }

    fn recalculate_transcript_rows(&mut self) {
        let reserved = self.input_reserved_rows().saturating_add(2); // account for transcript block borders
        let available = self.view_rows.saturating_sub(reserved).max(1);
        self.apply_transcript_rows(available);
    }

    fn clear_slash_suggestions(&mut self) {
        if self.slash_suggestions.is_empty() && self.slash_selected.is_none() {
            return;
        }
        self.slash_suggestions.clear();
        self.apply_slash_selection(None, false);
        self.slash_visible_rows = 0;
        self.recalculate_transcript_rows();
        self.enforce_scroll_bounds();
        self.mark_dirty();
    }

    fn update_slash_suggestions(&mut self) {
        if !self.input_enabled {
            self.clear_slash_suggestions();
            return;
        }

        let Some(prefix) = self.current_slash_prefix() else {
            self.clear_slash_suggestions();
            return;
        };

        let mut new_suggestions = suggestions_for(prefix);
        if !prefix.is_empty() {
            new_suggestions.truncate(ui::SLASH_SUGGESTION_LIMIT);
        }

        let changed = self.slash_suggestions.len() != new_suggestions.len()
            || self
                .slash_suggestions
                .iter()
                .zip(&new_suggestions)
                .any(|(current, candidate)| !ptr::eq(*current, *candidate));

        if changed {
            self.slash_suggestions = new_suggestions;
        }

        let selection_changed = self.ensure_slash_selection();
        if changed && !selection_changed {
            self.sync_slash_state();
        }
        if changed || selection_changed {
            self.recalculate_transcript_rows();
            self.enforce_scroll_bounds();
            self.mark_dirty();
        }
    }

    fn current_slash_prefix(&self) -> Option<&str> {
        if !self.input.starts_with('/') || self.cursor == 0 {
            return None;
        }

        let mut end = self.input.len();
        for (index, ch) in self.input.char_indices().skip(1) {
            if ch.is_whitespace() {
                end = index;
                break;
            }
        }

        if self.cursor > end {
            return None;
        }

        Some(&self.input[1..end])
    }

    fn slash_command_range(&self) -> Option<(usize, usize)> {
        if !self.input.starts_with('/') {
            return None;
        }

        let mut end = self.input.len();
        for (index, ch) in self.input.char_indices().skip(1) {
            if ch.is_whitespace() {
                end = index;
                break;
            }
        }

        if self.cursor > end {
            return None;
        }

        Some((0, end))
    }

    fn slash_navigation_available(&self) -> bool {
        self.input_enabled && !self.slash_suggestions.is_empty()
    }

    fn ensure_slash_selection(&mut self) -> bool {
        if self.slash_suggestions.is_empty() {
            if self.slash_selected.is_some() {
                self.apply_slash_selection(None, false);
                return true;
            }
            return false;
        }

        let visible_len = self.slash_suggestions.len();
        let new_index = self
            .slash_selected
            .filter(|index| *index < visible_len)
            .unwrap_or(0);

        if self.slash_selected == Some(new_index) {
            false
        } else {
            self.apply_slash_selection(Some(new_index), false);
            true
        }
    }

    fn move_slash_selection_up(&mut self) -> bool {
        if self.slash_suggestions.is_empty() {
            return false;
        }

        let visible_len = self.slash_suggestions.len();
        let new_index = match self.slash_selected {
            Some(0) | None => visible_len.saturating_sub(1),
            Some(index) => index.saturating_sub(1),
        };

        if self.slash_selected == Some(new_index) {
            false
        } else {
            self.apply_slash_selection(Some(new_index), true);
            self.mark_dirty();
            true
        }
    }

    fn move_slash_selection_down(&mut self) -> bool {
        if self.slash_suggestions.is_empty() {
            return false;
        }

        let visible_len = self.slash_suggestions.len();
        let new_index = match self.slash_selected {
            Some(index) if index + 1 < visible_len => index + 1,
            _ => 0,
        };

        if self.slash_selected == Some(new_index) {
            false
        } else {
            self.apply_slash_selection(Some(new_index), true);
            self.mark_dirty();
            true
        }
    }

    fn apply_slash_selection(&mut self, index: Option<usize>, preview: bool) {
        self.slash_selected = index;
        self.sync_slash_state();
        if preview {
            self.preview_selected_slash_suggestion();
        }
    }

    fn sync_slash_state(&mut self) {
        self.slash_list_state.select(self.slash_selected);
        if self.slash_selected.is_none() {
            *self.slash_list_state.offset_mut() = 0;
            return;
        }
        self.ensure_slash_list_visible();
    }

    fn ensure_slash_list_visible(&mut self) {
        if self.slash_visible_rows == 0 {
            return;
        }

        let Some(selected) = self.slash_selected else {
            return;
        };

        let visible_rows = self.slash_visible_rows;
        let offset_ref = self.slash_list_state.offset_mut();
        let offset = *offset_ref;
        if selected < offset {
            *offset_ref = selected;
        } else if selected >= offset + visible_rows {
            *offset_ref = selected + 1 - visible_rows;
        }
    }

    fn preview_selected_slash_suggestion(&mut self) {
        let Some(command) = self.selected_slash_command() else {
            return;
        };
        let Some((start, end)) = self.slash_command_range() else {
            return;
        };

        let current_input = self.input.clone();
        let prefix = &current_input[..start];
        let suffix = &current_input[end..];

        let mut new_input = String::new();
        new_input.push_str(prefix);
        new_input.push('/');
        new_input.push_str(command.name);
        let cursor_position = new_input.len();

        if !suffix.is_empty() {
            if !suffix.chars().next().map_or(false, char::is_whitespace) {
                new_input.push(' ');
            }
            new_input.push_str(suffix);
        }

        self.input = new_input;
        self.cursor = cursor_position.min(self.input.len());
        self.mark_dirty();
    }

    fn selected_slash_command(&self) -> Option<&'static SlashCommandInfo> {
        self.slash_selected
            .and_then(|index| self.slash_suggestions.get(index).copied())
    }

    fn apply_selected_slash_suggestion(&mut self) -> bool {
        let Some(command) = self.selected_slash_command() else {
            return false;
        };
        let Some((_, end)) = self.slash_command_range() else {
            return false;
        };

        let suffix = self.input[end..].to_string();
        let mut new_input = format!("/{}", command.name);

        let cursor_position = if suffix.is_empty() {
            new_input.push(' ');
            new_input.len()
        } else {
            if !suffix.chars().next().map_or(false, char::is_whitespace) {
                new_input.push(' ');
            }
            let position = new_input.len();
            new_input.push_str(&suffix);
            position
        };

        self.input = new_input;
        self.cursor = cursor_position;
        self.update_slash_suggestions();
        self.mark_dirty();
        true
    }

    fn try_handle_slash_navigation(
        &mut self,
        key: &KeyEvent,
        has_control: bool,
        has_alt: bool,
    ) -> bool {
        if !self.slash_navigation_available() || has_control || has_alt {
            return false;
        }

        match key.code {
            KeyCode::Up => self.move_slash_selection_up(),
            KeyCode::Down => self.move_slash_selection_down(),
            KeyCode::Tab => self.apply_selected_slash_suggestion(),
            KeyCode::BackTab => self.move_slash_selection_up(),
            _ => false,
        }
    }

    fn render_message_spans(&self, line: &MessageLine) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        if let Some(prefix) = self.prefix_text(line.kind) {
            let style = self.prefix_style(line);
            spans.push(Span::styled(
                prefix,
                ratatui_style_from_inline(&style, self.theme.foreground),
            ));
        }

        if line.segments.is_empty() {
            if spans.is_empty() {
                spans.push(Span::raw(String::new()));
            }
            return spans;
        }

        let fallback = self.text_fallback(line.kind).or(self.theme.foreground);
        for segment in &line.segments {
            let style = ratatui_style_from_inline(&segment.style, fallback);
            spans.push(Span::styled(segment.text.clone(), style));
        }

        if spans.is_empty() {
            spans.push(Span::raw(String::new()));
        }

        spans
    }

    fn default_style(&self) -> Style {
        let mut style = Style::default();
        if let Some(foreground) = self.theme.foreground.map(ratatui_color_from_ansi) {
            style = style.fg(foreground);
        }
        style
    }

    fn cursor_position(&self, area: Rect) -> (u16, u16) {
        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let before_cursor = &self.input[..self.cursor];
        let cursor_width = UnicodeWidthStr::width(before_cursor) as u16;
        (area.x + prompt_width + cursor_width, area.y)
    }

    fn cursor_should_be_visible(&self) -> bool {
        self.cursor_visible && self.input_enabled
    }

    pub fn mark_dirty(&mut self) {
        self.needs_redraw = true;
    }

    fn show_modal(&mut self, title: String, lines: Vec<String>) {
        let state = ModalState {
            title,
            lines,
            restore_input: self.input_enabled,
            restore_cursor: self.cursor_visible,
        };
        self.input_enabled = false;
        self.cursor_visible = false;
        self.modal = Some(state);
        self.mark_dirty();
    }

    fn close_modal(&mut self) {
        if let Some(state) = self.modal.take() {
            self.input_enabled = state.restore_input;
            self.cursor_visible = state.restore_cursor;
            self.mark_dirty();
        }
    }

    fn render_modal(&self, frame: &mut Frame<'_>, viewport: Rect) {
        let Some(modal) = &self.modal else {
            return;
        };
        if viewport.width == 0 || viewport.height == 0 {
            return;
        }

        let content_width = modal
            .lines
            .iter()
            .map(|line| UnicodeWidthStr::width(line.as_str()) as u16)
            .max()
            .unwrap_or(0);
        let horizontal_padding = 6;
        let vertical_padding = 4;
        let min_width = 20u16.min(viewport.width);
        let min_height = 5u16.min(viewport.height);
        let width = (content_width + horizontal_padding)
            .min(viewport.width.saturating_sub(2).max(min_width))
            .max(min_width);
        let height = (modal.lines.len() as u16 + vertical_padding)
            .min(viewport.height)
            .max(min_height);
        let x = viewport.x + (viewport.width.saturating_sub(width)) / 2;
        let y = viewport.y + (viewport.height.saturating_sub(height)) / 2;
        let area = Rect::new(x, y, width, height);

        frame.render_widget(Clear, area);
        let block = Block::default().borders(Borders::ALL).title(Span::styled(
            modal.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        frame.render_widget(block.clone(), area);
        let inner = block.inner(area);

        let lines: Vec<Line<'static>> = modal
            .lines
            .iter()
            .map(|line| Line::from(Span::raw(line.clone())))
            .collect();
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.update_slash_suggestions();
        self.mark_dirty();
    }

    fn process_key(&mut self, key: KeyEvent) -> Option<InlineEvent> {
        let modifiers = key.modifiers;
        let has_control = modifiers.contains(KeyModifiers::CONTROL);
        let raw_alt = modifiers.contains(KeyModifiers::ALT);
        let raw_meta = modifiers.contains(KeyModifiers::META);
        let has_super = modifiers.contains(KeyModifiers::SUPER);
        let has_alt = raw_alt || (!has_super && raw_meta);
        let has_command = has_super || (raw_meta && !has_alt);

        if self.try_handle_slash_navigation(&key, has_control, has_alt) {
            return None;
        }

        match key.code {
            KeyCode::Char('c') if has_control => {
                self.mark_dirty();
                Some(InlineEvent::Interrupt)
            }
            KeyCode::Char('d') if has_control => {
                self.mark_dirty();
                Some(InlineEvent::Exit)
            }
            KeyCode::Esc => {
                if self.modal.is_some() {
                    self.close_modal();
                    None
                } else {
                    self.mark_dirty();
                    Some(InlineEvent::Cancel)
                }
            }
            KeyCode::PageUp => {
                self.scroll_page_up();
                self.mark_dirty();
                Some(InlineEvent::ScrollPageUp)
            }
            KeyCode::PageDown => {
                self.scroll_page_down();
                self.mark_dirty();
                Some(InlineEvent::ScrollPageDown)
            }
            KeyCode::Up => {
                self.scroll_line_up();
                self.mark_dirty();
                Some(InlineEvent::ScrollLineUp)
            }
            KeyCode::Down => {
                self.scroll_line_down();
                self.mark_dirty();
                Some(InlineEvent::ScrollLineDown)
            }
            KeyCode::Enter => {
                if self.input_enabled {
                    let submitted = std::mem::take(&mut self.input);
                    self.cursor = 0;
                    self.update_slash_suggestions();
                    self.mark_dirty();
                    Some(InlineEvent::Submit(submitted))
                } else {
                    None
                }
            }
            KeyCode::Backspace => {
                if self.input_enabled {
                    self.delete_char();
                    self.mark_dirty();
                }
                None
            }
            KeyCode::Left => {
                if self.input_enabled {
                    if has_command {
                        self.move_to_start();
                    } else if has_alt {
                        self.move_left_word();
                    } else {
                        self.move_left();
                    }
                    self.mark_dirty();
                }
                None
            }
            KeyCode::Right => {
                if self.input_enabled {
                    if has_command {
                        self.move_to_end();
                    } else if has_alt {
                        self.move_right_word();
                    } else {
                        self.move_right();
                    }
                    self.mark_dirty();
                }
                None
            }
            KeyCode::Home => {
                if self.input_enabled {
                    self.move_to_start();
                    self.mark_dirty();
                }
                None
            }
            KeyCode::End => {
                if self.input_enabled {
                    self.move_to_end();
                    self.mark_dirty();
                }
                None
            }
            KeyCode::Char(ch) => {
                if !self.input_enabled {
                    return None;
                }

                if has_command {
                    match ch {
                        'a' | 'A' => {
                            self.move_to_start();
                            self.mark_dirty();
                            return None;
                        }
                        'e' | 'E' => {
                            self.move_to_end();
                            self.mark_dirty();
                            return None;
                        }
                        _ => {
                            return None;
                        }
                    }
                }

                if has_alt {
                    match ch {
                        'b' | 'B' => {
                            self.move_left_word();
                            self.mark_dirty();
                        }
                        'f' | 'F' => {
                            self.move_right_word();
                            self.mark_dirty();
                        }
                        _ => {}
                    }
                    return None;
                }

                if !has_control {
                    self.insert_char(ch);
                    self.mark_dirty();
                }
                None
            }
            _ => None,
        }
    }

    fn insert_char(&mut self, ch: char) {
        if ch == '\u{7f}' {
            return;
        }
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.update_slash_suggestions();
    }

    fn delete_char(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((index, _)) = self
            .input
            .char_indices()
            .take_while(|(idx, _)| *idx < self.cursor)
            .last()
        {
            self.input.drain(index..self.cursor);
            self.cursor = index;
            self.update_slash_suggestions();
        }
    }

    fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((index, _)) = self
            .input
            .char_indices()
            .take_while(|(idx, _)| *idx < self.cursor)
            .last()
        {
            self.cursor = index;
            self.update_slash_suggestions();
        }
    }

    fn move_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        let slice = &self.input[self.cursor..];
        if let Some((_, ch)) = slice.char_indices().next() {
            self.cursor += ch.len_utf8();
            self.update_slash_suggestions();
        } else {
            self.cursor = self.input.len();
            self.update_slash_suggestions();
        }
    }

    fn move_left_word(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let graphemes: Vec<(usize, &str)> =
            self.input[..self.cursor].grapheme_indices(true).collect();

        if graphemes.is_empty() {
            self.cursor = 0;
            return;
        }

        let mut index = graphemes.len();

        while index > 0 {
            let (_, grapheme) = graphemes[index - 1];
            if grapheme.chars().all(char::is_whitespace) {
                index -= 1;
            } else {
                break;
            }
        }

        while index > 0 {
            let (_, grapheme) = graphemes[index - 1];
            if grapheme.chars().all(char::is_whitespace) {
                break;
            }
            index -= 1;
        }

        if index < graphemes.len() {
            self.cursor = graphemes[index].0;
        } else {
            self.cursor = 0;
        }
    }

    fn move_right_word(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }

        let graphemes: Vec<(usize, &str)> =
            self.input[self.cursor..].grapheme_indices(true).collect();

        if graphemes.is_empty() {
            self.cursor = self.input.len();
            return;
        }

        let mut index = 0;
        let mut skipped_whitespace = false;

        while index < graphemes.len() {
            let (_, grapheme) = graphemes[index];
            if grapheme.chars().all(char::is_whitespace) {
                index += 1;
                skipped_whitespace = true;
            } else {
                break;
            }
        }

        if index >= graphemes.len() {
            self.cursor = self.input.len();
            return;
        }

        if skipped_whitespace {
            self.cursor += graphemes[index].0;
            return;
        }

        while index < graphemes.len() {
            let (_, grapheme) = graphemes[index];
            if grapheme.chars().all(char::is_whitespace) {
                break;
            }
            index += 1;
        }

        if index < graphemes.len() {
            self.cursor += graphemes[index].0;
        } else {
            self.cursor = self.input.len();
        }
    }

    fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    fn move_to_end(&mut self) {
        self.cursor = self.input.len();
    }

    fn prefix_text(&self, kind: InlineMessageKind) -> Option<String> {
        match kind {
            InlineMessageKind::User => Some(
                self.labels
                    .user
                    .clone()
                    .unwrap_or_else(|| USER_PREFIX.to_string()),
            ),
            InlineMessageKind::Agent | InlineMessageKind::Policy => self.labels.agent.clone(),
            InlineMessageKind::Tool | InlineMessageKind::Pty | InlineMessageKind::Error => None,
            InlineMessageKind::Info => None,
        }
    }

    fn prefix_style(&self, line: &MessageLine) -> InlineTextStyle {
        let fallback = self.text_fallback(line.kind).or(self.theme.foreground);

        let color = line
            .segments
            .iter()
            .find_map(|segment| segment.style.color)
            .or(fallback);

        InlineTextStyle {
            color,
            ..InlineTextStyle::default()
        }
    }

    fn text_fallback(&self, kind: InlineMessageKind) -> Option<AnsiColorEnum> {
        match kind {
            InlineMessageKind::Agent | InlineMessageKind::Policy => {
                self.theme.primary.or(self.theme.foreground)
            }
            InlineMessageKind::User => self.theme.secondary.or(self.theme.foreground),
            InlineMessageKind::Tool | InlineMessageKind::Pty | InlineMessageKind::Error => {
                self.theme.primary.or(self.theme.foreground)
            }
            InlineMessageKind::Info => self.theme.foreground,
        }
    }

    fn push_line(&mut self, kind: InlineMessageKind, segments: Vec<InlineSegment>) {
        let previous_max_offset = self.current_max_scroll_offset();
        self.lines.push(MessageLine { kind, segments });
        self.invalidate_scroll_metrics();
        self.adjust_scroll_after_change(previous_max_offset);
    }

    fn append_inline(&mut self, kind: InlineMessageKind, segment: InlineSegment) {
        let previous_max_offset = self.current_max_scroll_offset();
        let mut remaining = segment.text.as_str();
        let style = segment.style.clone();

        while !remaining.is_empty() {
            if let Some((index, control)) = remaining
                .char_indices()
                .find(|(_, ch)| matches!(ch, '\n' | '\r'))
            {
                let (text, _) = remaining.split_at(index);
                if !text.is_empty() {
                    self.append_text(kind, text, &style);
                }

                let control_char = control;
                let next_index = index + control_char.len_utf8();
                remaining = &remaining[next_index..];

                match control_char {
                    '\n' => self.start_line(kind),
                    '\r' => {
                        if remaining.starts_with('\n') {
                            remaining = &remaining[1..];
                            self.start_line(kind);
                        } else {
                            self.reset_line(kind);
                        }
                    }
                    _ => {}
                }
            } else {
                if !remaining.is_empty() {
                    self.append_text(kind, remaining, &style);
                }
                break;
            }
        }

        self.invalidate_scroll_metrics();
        self.adjust_scroll_after_change(previous_max_offset);
    }

    fn replace_last(
        &mut self,
        count: usize,
        kind: InlineMessageKind,
        lines: Vec<Vec<InlineSegment>>,
    ) {
        let previous_max_offset = self.current_max_scroll_offset();
        let remove_count = min(count, self.lines.len());
        for _ in 0..remove_count {
            self.lines.pop();
        }
        for segments in lines {
            self.lines.push(MessageLine { kind, segments });
        }
        self.invalidate_scroll_metrics();
        self.adjust_scroll_after_change(previous_max_offset);
    }

    fn append_text(&mut self, kind: InlineMessageKind, text: &str, style: &InlineTextStyle) {
        if text.is_empty() {
            return;
        }

        let mut appended = false;

        if let Some(line) = self.lines.last_mut() {
            if line.kind == kind {
                if let Some(last) = line.segments.last_mut() {
                    if last.style == *style {
                        last.text.push_str(text);
                        appended = true;
                    }
                }
                if !appended {
                    line.segments.push(InlineSegment {
                        text: text.to_string(),
                        style: style.clone(),
                    });
                    appended = true;
                }
            }
        }

        if !appended {
            self.lines.push(MessageLine {
                kind,
                segments: vec![InlineSegment {
                    text: text.to_string(),
                    style: style.clone(),
                }],
            });
        }

        self.invalidate_scroll_metrics();
    }

    fn start_line(&mut self, kind: InlineMessageKind) {
        self.push_line(kind, Vec::new());
    }

    fn reset_line(&mut self, kind: InlineMessageKind) {
        if let Some(line) = self.lines.last_mut() {
            if line.kind == kind {
                line.segments.clear();
                self.invalidate_scroll_metrics();
                return;
            }
        }
        self.start_line(kind);
    }

    fn scroll_line_up(&mut self) {
        let max_offset = self.current_max_scroll_offset();
        if max_offset == 0 {
            self.scroll_offset = 0;
            return;
        }

        self.scroll_offset = min(self.scroll_offset + 1, max_offset);
    }

    fn scroll_line_down(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    fn scroll_page_up(&mut self) {
        let max_offset = self.current_max_scroll_offset();
        if max_offset == 0 {
            self.scroll_offset = 0;
            return;
        }

        let page = self.viewport_height().max(1);
        self.scroll_offset = min(self.scroll_offset + page, max_offset);
    }

    fn scroll_page_down(&mut self) {
        let page = self.viewport_height();
        if self.scroll_offset > page {
            self.scroll_offset -= page;
        } else {
            self.scroll_offset = 0;
        }
    }

    fn viewport_height(&self) -> usize {
        self.transcript_rows.max(1) as usize
    }

    fn current_max_scroll_offset(&mut self) -> usize {
        self.ensure_scroll_metrics();
        self.cached_max_scroll_offset
    }

    fn enforce_scroll_bounds(&mut self) {
        let max_offset = self.current_max_scroll_offset();
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }

    fn invalidate_scroll_metrics(&mut self) {
        self.scroll_metrics_dirty = true;
    }

    fn ensure_scroll_metrics(&mut self) {
        if !self.scroll_metrics_dirty {
            return;
        }

        let viewport_rows = self.viewport_height();
        if self.transcript_width == 0 || viewport_rows == 0 {
            self.cached_max_scroll_offset = self.lines.len().saturating_sub(viewport_rows.max(1));
            self.scroll_metrics_dirty = false;
            return;
        }

        let wrapped = self.reflow_transcript_lines(self.transcript_width);
        let total_rows = wrapped.len();
        let max_offset = total_rows.saturating_sub(viewport_rows);
        self.cached_max_scroll_offset = max_offset;
        self.scroll_metrics_dirty = false;
    }

    fn reflow_transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 {
            return self.transcript_lines();
        }

        let mut wrapped_lines = Vec::new();
        let max_width = width as usize;

        for line in self.transcript_lines() {
            wrapped_lines.extend(self.wrap_line(line, max_width));
        }

        if wrapped_lines.is_empty() {
            wrapped_lines.push(Line::default());
        }

        wrapped_lines
    }

    fn wrap_line(&self, line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
        if max_width == 0 {
            return vec![Line::default()];
        }

        let mut rows = Vec::new();
        let mut current_spans: Vec<Span<'static>> = Vec::new();
        let mut current_width = 0usize;

        let flush_current =
            |spans: &mut Vec<Span<'static>>, width: &mut usize, rows: &mut Vec<Line<'static>>| {
                if spans.is_empty() {
                    rows.push(Line::default());
                } else {
                    rows.push(Line::from(mem::take(spans)));
                }
                *width = 0;
            };

        for span in line.spans.into_iter() {
            let style = span.style;
            let content = span.content.into_owned();
            if content.is_empty() {
                continue;
            }

            for grapheme in UnicodeSegmentation::graphemes(content.as_str(), true) {
                if grapheme.is_empty() {
                    continue;
                }

                if grapheme.chars().any(|c| c == '\n') {
                    flush_current(&mut current_spans, &mut current_width, &mut rows);
                    continue;
                }

                let grapheme_width = UnicodeWidthStr::width(grapheme);
                if grapheme_width == 0 {
                    continue;
                }

                if grapheme_width > max_width {
                    continue;
                }

                if current_width + grapheme_width > max_width && current_width > 0 {
                    flush_current(&mut current_spans, &mut current_width, &mut rows);
                }

                let text = grapheme.to_string();
                if let Some(last) = current_spans.last_mut() {
                    if last.style == style {
                        last.content.to_mut().push_str(&text);
                        current_width += grapheme_width;
                        continue;
                    }
                }

                current_spans.push(Span::styled(text, style));
                current_width += grapheme_width;
            }
        }

        if current_spans.is_empty() {
            if rows.is_empty() {
                rows.push(Line::default());
            }
        } else {
            rows.push(Line::from(current_spans));
        }

        rows
    }

    fn prepare_transcript_list(
        &mut self,
        width: u16,
        viewport_rows: usize,
    ) -> (Vec<ListItem<'static>>, usize) {
        let viewport = viewport_rows.max(1);
        let wrapped_lines = self.reflow_transcript_lines(width);
        let total_rows = wrapped_lines.len();
        let max_offset = total_rows.saturating_sub(viewport);
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
        self.cached_max_scroll_offset = max_offset;
        self.scroll_metrics_dirty = false;

        let top_offset = max_offset.saturating_sub(self.scroll_offset);
        let items = if wrapped_lines.is_empty() {
            vec![ListItem::new(Line::default())]
        } else {
            wrapped_lines.into_iter().map(ListItem::new).collect()
        };

        (items, top_offset)
    }

    fn adjust_scroll_after_change(&mut self, previous_max_offset: usize) {
        let new_max_offset = self.current_max_scroll_offset();
        if self.scroll_offset >= previous_max_offset && new_max_offset > previous_max_offset {
            self.scroll_offset = new_max_offset;
        } else if self.scroll_offset > 0 && new_max_offset > previous_max_offset {
            let delta = new_max_offset - previous_max_offset;
            self.scroll_offset = min(self.scroll_offset + delta, new_max_offset);
        }
        self.enforce_scroll_bounds();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};

    const VIEW_ROWS: u16 = 14;
    const VIEW_WIDTH: u16 = 100;
    const LINE_COUNT: usize = 10;
    const LABEL_PREFIX: &str = "line";
    const EXTRA_SEGMENT: &str = "\nextra-line";

    fn make_segment(text: &str) -> InlineSegment {
        InlineSegment {
            text: text.to_string(),
            style: InlineTextStyle::default(),
        }
    }

    fn session_with_input(input: &str, cursor: usize) -> Session {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
        session.input = input.to_string();
        session.cursor = cursor;
        session
    }

    fn visible_transcript(session: &mut Session) -> Vec<String> {
        let backend = TestBackend::new(VIEW_WIDTH, VIEW_ROWS);
        let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
        terminal
            .draw(|frame| session.render(frame))
            .expect("failed to render test session");

        let width = session.transcript_width;
        let viewport = session.viewport_height();
        let offset = session.transcript_state.offset();
        let lines = session.reflow_transcript_lines(width);

        lines
            .into_iter()
            .skip(offset)
            .take(viewport)
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn move_left_word_from_end_moves_to_word_start() {
        let text = "hello world";
        let mut session = session_with_input(text, text.len());

        session.move_left_word();
        assert_eq!(session.cursor, 6);

        session.move_left_word();
        assert_eq!(session.cursor, 0);
    }

    #[test]
    fn move_left_word_skips_trailing_whitespace() {
        let text = "hello  world";
        let mut session = session_with_input(text, text.len());

        session.move_left_word();
        assert_eq!(session.cursor, 7);
    }

    #[test]
    fn alt_arrow_left_moves_cursor_by_word() {
        let text = "hello world";
        let mut session = session_with_input(text, text.len());

        let event = KeyEvent::new(KeyCode::Left, KeyModifiers::ALT);
        session.process_key(event);

        assert_eq!(session.cursor, 6);
    }

    #[test]
    fn alt_b_moves_cursor_by_word() {
        let text = "hello world";
        let mut session = session_with_input(text, text.len());

        let event = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT);
        session.process_key(event);

        assert_eq!(session.cursor, 6);
    }

    #[test]
    fn move_right_word_advances_to_word_boundaries() {
        let text = "hello  world";
        let mut session = session_with_input(text, 0);

        session.move_right_word();
        assert_eq!(session.cursor, 5);

        session.move_right_word();
        assert_eq!(session.cursor, 7);

        session.move_right_word();
        assert_eq!(session.cursor, text.len());
    }

    #[test]
    fn move_right_word_from_whitespace_moves_to_next_word_start() {
        let text = "hello  world";
        let mut session = session_with_input(text, 5);

        session.move_right_word();
        assert_eq!(session.cursor, 7);
    }

    #[test]
    fn super_arrow_right_moves_cursor_to_end() {
        let text = "hello world";
        let mut session = session_with_input(text, 0);

        let event = KeyEvent::new(KeyCode::Right, KeyModifiers::SUPER);
        session.process_key(event);

        assert_eq!(session.cursor, text.len());
    }

    #[test]
    fn super_a_moves_cursor_to_start() {
        let text = "hello world";
        let mut session = session_with_input(text, text.len());

        let event = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::SUPER);
        session.process_key(event);

        assert_eq!(session.cursor, 0);
    }

    #[test]
    fn streaming_new_lines_preserves_scrolled_view() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

        for index in 1..=LINE_COUNT {
            let label = format!("{LABEL_PREFIX}-{index}");
            session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
        }

        session.scroll_page_up();
        let before = visible_transcript(&mut session);

        session.append_inline(InlineMessageKind::Agent, make_segment(EXTRA_SEGMENT));

        let after = visible_transcript(&mut session);
        assert_eq!(before.len(), after.len());
        assert!(
            after.iter().all(|line| !line.contains("extra-line")),
            "appended lines should not appear when scrolled up"
        );
    }

    #[test]
    fn page_up_reveals_prior_lines_until_buffer_start() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

        for index in 1..=LINE_COUNT {
            let label = format!("{LABEL_PREFIX}-{index}");
            session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
        }

        let mut transcripts = Vec::new();
        let mut iterations = 0;
        loop {
            transcripts.push(visible_transcript(&mut session));
            let previous_offset = session.scroll_offset;
            session.scroll_page_up();
            if session.scroll_offset == previous_offset {
                break;
            }
            iterations += 1;
            assert!(
                iterations <= LINE_COUNT,
                "scroll_page_up did not converge within expected bounds"
            );
        }

        assert!(transcripts.len() > 1);

        for window in transcripts.windows(2) {
            assert_ne!(window[0], window[1]);
        }

        let top_view = transcripts
            .last()
            .expect("a top-of-buffer page should exist after scrolling");
        let first_label = format!("{LABEL_PREFIX}-1");
        let last_label = format!("{LABEL_PREFIX}-{LINE_COUNT}");

        assert!(top_view.iter().any(|line| line.contains(&first_label)));
        assert!(top_view.iter().all(|line| !line.contains(&last_label)));
        let scroll_offset = session.scroll_offset;
        let max_offset = session.current_max_scroll_offset();
        assert_eq!(scroll_offset, max_offset);
    }

    #[test]
    fn resizing_viewport_clamps_scroll_offset() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);

        for index in 1..=LINE_COUNT {
            let label = format!("{LABEL_PREFIX}-{index}");
            session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
        }

        session.scroll_page_up();
        assert!(session.scroll_offset > 0);

        session.force_view_rows(
            (LINE_COUNT as u16) + ui::INLINE_HEADER_HEIGHT + ui::INLINE_INPUT_HEIGHT + 2,
        );

        assert_eq!(session.scroll_offset, 0);
        let max_offset = session.current_max_scroll_offset();
        assert_eq!(max_offset, 0);
    }
}
