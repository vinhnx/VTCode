use std::{cmp::min, mem, ptr, sync::OnceLock};

use anstyle::{AnsiColor, Color as AnsiColorEnum, RgbColor};
use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Widget, Wrap,
    },
};
use terminal_size::{Height, Width, terminal_size};
use tokio::sync::mpsc::UnboundedSender;
use tui_scrollview::ScrollViewState;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::types::{
    InlineCommand, InlineEvent, InlineHeaderContext, InlineHeaderHighlight, InlineListItem,
    InlineListSelection, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme,
};
use crate::config::constants::ui;
use crate::ui::slash::{SlashCommandInfo, suggestions_for};

const USER_PREFIX: &str = "❯ ";
const PLACEHOLDER_COLOR: RgbColor = RgbColor(0x88, 0x88, 0x88);

#[derive(Clone)]
struct MessageLine {
    kind: InlineMessageKind,
    segments: Vec<InlineSegment>,
    revision: u64,
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
    list: Option<ModalListState>,
    restore_input: bool,
    restore_cursor: bool,
}

#[derive(Clone)]
struct ModalListState {
    items: Vec<ModalListItem>,
    list_state: ListState,
}

#[derive(Clone)]
struct ModalListItem {
    title: String,
    subtitle: Option<String>,
    badge: Option<String>,
    indent: u8,
    selection: Option<InlineListSelection>,
}

struct ModalRenderStyles {
    border: Style,
    highlight: Style,
    badge: Style,
    header: Style,
    selectable: Style,
    detail: Style,
    title: Style,
}

struct ModalListLayout {
    text_area: Option<Rect>,
    list_area: Rect,
}

impl ModalListLayout {
    fn new(area: Rect, text_line_count: usize) -> Self {
        if text_line_count == 0 {
            let chunks = Layout::vertical(vec![Constraint::Min(3)]).split(area);
            return Self {
                text_area: None,
                list_area: chunks[0],
            };
        }

        let paragraph_height = (text_line_count.min(u16::MAX as usize) as u16).saturating_add(1);
        let chunks = Layout::vertical(vec![
            Constraint::Length(paragraph_height),
            Constraint::Min(3),
        ])
        .split(area);

        Self {
            text_area: Some(chunks[0]),
            list_area: chunks[1],
        }
    }
}

fn terminal_dimensions() -> Option<(u16, u16)> {
    terminal_size().map(|(Width(width), Height(height))| (width, height))
}

fn compute_modal_area(viewport: Rect, width_hint: u16, text_lines: usize, has_list: bool) -> Rect {
    if viewport.width == 0 || viewport.height == 0 {
        return Rect::new(viewport.x, viewport.y, 0, 0);
    }

    let (term_width, term_height) = terminal_dimensions()
        .map(|(w, h)| (w.max(1), h.max(1)))
        .unwrap_or((viewport.width, viewport.height));
    let available_width = viewport.width.min(term_width);
    let available_height = viewport.height.min(term_height);

    let ratio_width = ((available_width as f32) * ui::MODAL_WIDTH_RATIO).round() as u16;
    let ratio_height = ((available_height as f32) * ui::MODAL_HEIGHT_RATIO).round() as u16;
    let max_width = ((available_width as f32) * ui::MODAL_MAX_WIDTH_RATIO).round() as u16;
    let max_height = ((available_height as f32) * ui::MODAL_MAX_HEIGHT_RATIO).round() as u16;

    let min_width = ui::MODAL_MIN_WIDTH.min(available_width.max(1));
    let base_min_height = ui::MODAL_MIN_HEIGHT.min(available_height.max(1));
    let min_height = if has_list {
        ui::MODAL_LIST_MIN_HEIGHT
            .min(available_height.max(1))
            .max(base_min_height)
    } else {
        base_min_height
    };

    let mut width = width_hint
        .saturating_add(ui::MODAL_CONTENT_HORIZONTAL_PADDING)
        .max(min_width)
        .max(ratio_width);
    width = width.min(max_width.max(min_width)).min(available_width);

    let text_height = text_lines as u16;
    let mut height = text_height
        .saturating_add(ui::MODAL_CONTENT_VERTICAL_PADDING)
        .max(min_height)
        .max(ratio_height);
    if has_list {
        height = height.max(ui::MODAL_LIST_MIN_HEIGHT.min(available_height));
    }
    height = height.min(max_height.max(min_height)).min(available_height);

    let x = viewport.x + (viewport.width.saturating_sub(width)) / 2;
    let y = viewport.y + (viewport.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

fn modal_content_width(lines: &[String], list: Option<&ModalListState>) -> u16 {
    let mut width = lines
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_str()) as u16)
        .max()
        .unwrap_or(0);

    if let Some(list_state) = list {
        for item in &list_state.items {
            let badge_width = item
                .badge
                .as_ref()
                .map(|badge| UnicodeWidthStr::width(badge.as_str()).saturating_add(1))
                .unwrap_or(0);
            let title_width = UnicodeWidthStr::width(item.title.as_str());
            let subtitle_width = item
                .subtitle
                .as_ref()
                .map(|text| UnicodeWidthStr::width(text.as_str()))
                .unwrap_or(0);
            let indent_width = item.indent as usize;

            let primary_width = indent_width
                .saturating_add(badge_width)
                .saturating_add(title_width) as u16;
            let secondary_width = indent_width.saturating_add(subtitle_width) as u16;

            width = width.max(primary_width).max(secondary_width);
        }
    }

    width
}

fn measure_text_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text) as u16
}

fn render_modal_list(
    frame: &mut Frame<'_>,
    area: Rect,
    text_lines: &[Line<'static>],
    list: &mut ModalListState,
    styles: &ModalRenderStyles,
) {
    let layout = ModalListLayout::new(area, text_lines.len());
    if let Some(text_area) = layout.text_area {
        if text_area.height > 0 && !text_lines.is_empty() {
            let paragraph = Paragraph::new(text_lines.to_vec()).wrap(Wrap { trim: false });
            frame.render_widget(paragraph, text_area);
        }
    }

    list.ensure_visible(layout.list_area.height);
    let items = modal_list_items(list, styles);
    let widget = List::new(items)
        .block(Block::default())
        .highlight_style(styles.highlight.clone());
    frame.render_stateful_widget(widget, layout.list_area, &mut list.list_state);
}

fn modal_list_items(list: &ModalListState, styles: &ModalRenderStyles) -> Vec<ListItem<'static>> {
    list.items
        .iter()
        .map(|item| modal_list_item(item, styles))
        .collect()
}

fn modal_list_item(item: &ModalListItem, styles: &ModalRenderStyles) -> ListItem<'static> {
    let indent = " ".repeat(item.indent as usize);
    let mut spans = Vec::new();
    if let Some(badge) = &item.badge {
        spans.push(Span::styled(badge.clone(), styles.badge.clone()));
        spans.push(Span::raw(" "));
    }
    let primary = if item.selection.is_some() {
        styles.selectable.clone()
    } else {
        styles.header.clone()
    };
    spans.push(Span::styled(format!("{indent}{}", item.title), primary));
    let mut lines = vec![Line::from(spans)];
    if let Some(subtitle) = &item.subtitle {
        lines.push(Line::from(Span::styled(
            format!("{indent}{subtitle}"),
            styles.detail.clone(),
        )));
    }
    ListItem::new(lines)
}

impl ModalListState {
    fn new(items: Vec<InlineListItem>, selected: Option<InlineListSelection>) -> Self {
        let converted: Vec<ModalListItem> = items
            .into_iter()
            .map(|item| ModalListItem {
                title: item.title,
                subtitle: item.subtitle,
                badge: item.badge,
                indent: item.indent,
                selection: item.selection,
            })
            .collect();
        let mut list_state = ListState::default();
        let preferred = selected
            .and_then(|needle| {
                converted
                    .iter()
                    .position(|item| item.selection.as_ref() == Some(&needle))
            })
            .or_else(|| converted.iter().position(|item| item.selection.is_some()));
        if let Some(index) = preferred {
            list_state.select(Some(index));
        }
        Self {
            items: converted,
            list_state,
        }
    }

    fn current_selection(&self) -> Option<InlineListSelection> {
        self.list_state
            .selected()
            .and_then(|index| self.items.get(index))
            .and_then(|item| item.selection.clone())
    }

    fn select_previous(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let mut index = self.list_state.selected().unwrap_or_else(|| {
            self.items
                .iter()
                .rposition(|item| item.selection.is_some())
                .unwrap_or(0)
        });
        if index == 0 {
            if self.items[index].selection.is_none() {
                if let Some(first) = self.items.iter().position(|item| item.selection.is_some()) {
                    self.list_state.select(Some(first));
                }
            }
            return;
        }
        while index > 0 {
            index -= 1;
            if self.items[index].selection.is_some() {
                self.list_state.select(Some(index));
                break;
            }
        }
    }

    fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let mut index = self.list_state.selected().unwrap_or(usize::MAX);
        if index == usize::MAX {
            if let Some(first) = self.items.iter().position(|item| item.selection.is_some()) {
                self.list_state.select(Some(first));
            }
            return;
        }
        while index + 1 < self.items.len() {
            index += 1;
            if self.items[index].selection.is_some() {
                self.list_state.select(Some(index));
                break;
            }
        }
    }

    fn ensure_visible(&mut self, viewport: u16) {
        let Some(selected) = self.list_state.selected() else {
            return;
        };
        if viewport == 0 {
            return;
        }
        let visible = viewport as usize;
        let offset = self.list_state.offset();
        if selected < offset {
            *self.list_state.offset_mut() = selected;
        } else if selected >= offset + visible {
            *self.list_state.offset_mut() = selected + 1 - visible;
        }
    }
}

struct TranscriptReflowCache {
    width: u16,
    flattened: Vec<Line<'static>>,
    messages: Vec<CachedMessage>,
}

#[derive(Default)]
struct CachedMessage {
    revision: u64,
    lines: Vec<Line<'static>>,
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
    header_rows: u16,
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
    transcript_scroll: ScrollViewState,
    cached_max_scroll_offset: usize,
    scroll_metrics_dirty: bool,
    transcript_cache: Option<TranscriptReflowCache>,
    modal: Option<ModalState>,
    show_timeline_pane: bool,
    line_revision_counter: u64,
}

impl Session {
    fn next_revision(&mut self) -> u64 {
        self.line_revision_counter = self.line_revision_counter.wrapping_add(1);
        self.line_revision_counter
    }

    pub fn new(
        theme: InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_timeline_pane: bool,
    ) -> Self {
        let resolved_rows = view_rows.max(2);
        let initial_header_rows = ui::INLINE_HEADER_HEIGHT;
        let reserved_rows = initial_header_rows + ui::INLINE_INPUT_HEIGHT;
        let initial_transcript_rows = resolved_rows.saturating_sub(reserved_rows).max(1);
        let mut session = Self {
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
            transcript_scroll: ScrollViewState::default(),
            cached_max_scroll_offset: 0,
            scroll_metrics_dirty: true,
            transcript_cache: None,
            modal: None,
            show_timeline_pane,
            header_rows: initial_header_rows,
            line_revision_counter: 0,
        };
        session.ensure_prompt_style_color();
        session
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
                self.ensure_prompt_style_color();
            }
            InlineCommand::SetPlaceholder { hint, style } => {
                self.placeholder = hint;
                self.placeholder_style = style;
            }
            InlineCommand::SetMessageLabels { agent, user } => {
                self.labels.agent = agent.filter(|label| !label.is_empty());
                self.labels.user = user.filter(|label| !label.is_empty());
                self.invalidate_scroll_metrics();
            }
            InlineCommand::SetHeaderContext { context } => {
                self.header_context = context;
                self.needs_redraw = true;
            }
            InlineCommand::SetTheme { theme } => {
                self.theme = theme;
                self.ensure_prompt_style_color();
                self.invalidate_transcript_cache();
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
            InlineCommand::ShowListModal {
                title,
                lines,
                items,
                selected,
            } => {
                self.show_list_modal(title, lines, items, selected);
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

        let header_lines = self.header_lines();
        let header_height = self.header_height_from_lines(viewport.width, &header_lines);
        if header_height != self.header_rows {
            self.header_rows = header_height;
            self.recalculate_transcript_rows();
        }

        let mut constraints = vec![Constraint::Length(header_height), Constraint::Min(1)];
        constraints.push(Constraint::Length(ui::INLINE_INPUT_HEIGHT));

        let segments = Layout::vertical(constraints).split(viewport);

        let header_area = segments[0];
        let main_area = segments[1];
        let input_index = segments.len().saturating_sub(1);
        let input_area = segments[input_index];

        let available_width = main_area.width;
        let horizontal_minimum = ui::INLINE_CONTENT_MIN_WIDTH + ui::INLINE_NAVIGATION_MIN_WIDTH;

        let (transcript_area, navigation_area) = if self.show_timeline_pane {
            if available_width >= horizontal_minimum {
                let nav_percent = u32::from(ui::INLINE_NAVIGATION_PERCENT);
                let mut nav_width = ((available_width as u32 * nav_percent) / 100) as u16;
                nav_width = nav_width.max(ui::INLINE_NAVIGATION_MIN_WIDTH);
                let max_allowed = available_width.saturating_sub(ui::INLINE_CONTENT_MIN_WIDTH);
                nav_width = nav_width.min(max_allowed);

                let constraints = [
                    Constraint::Min(ui::INLINE_CONTENT_MIN_WIDTH),
                    Constraint::Length(nav_width),
                ];
                let main_chunks = Layout::horizontal(constraints).split(main_area);
                (main_chunks[0], main_chunks[1])
            } else {
                let nav_percent = ui::INLINE_STACKED_NAVIGATION_PERCENT.min(99);
                let transcript_percent = (100u16).saturating_sub(nav_percent).max(1u16);
                let constraints = [
                    Constraint::Percentage(transcript_percent),
                    Constraint::Percentage(nav_percent.max(1u16)),
                ];
                let main_chunks = Layout::vertical(constraints).split(main_area);
                (main_chunks[0], main_chunks[1])
            }
        } else {
            (main_area, Rect::new(main_area.x, main_area.y, 0, 0))
        };

        self.render_header(frame, header_area, &header_lines);
        if self.show_timeline_pane {
            self.render_navigation(frame, navigation_area);
        }
        self.render_transcript(frame, transcript_area);
        self.render_input(frame, input_area);
        self.render_modal(frame, viewport);
        self.render_slash_palette(frame, viewport);
    }

    fn render_header(&self, frame: &mut Frame<'_>, area: Rect, lines: &[Line<'static>]) {
        frame.render_widget(Clear, area);
        if area.height == 0 || area.width == 0 {
            return;
        }

        let paragraph = self.build_header_paragraph(lines);

        frame.render_widget(paragraph, area);
    }

    fn header_lines(&self) -> Vec<Line<'static>> {
        let mut lines = vec![self.header_title_line(), self.header_meta_line()];
        if !self.header_context.highlights.is_empty() {
            lines.push(Line::default());
        }

        for (index, highlight) in self.header_context.highlights.iter().enumerate() {
            lines.push(self.header_highlight_title_line(highlight));
            lines.extend(self.header_highlight_body_lines(highlight));
            if index + 1 < self.header_context.highlights.len() {
                lines.push(Line::default());
            }
        }

        lines
    }

    fn header_height_from_lines(&self, width: u16, lines: &[Line<'static>]) -> u16 {
        if width == 0 {
            return self.header_rows.max(ui::INLINE_HEADER_HEIGHT);
        }

        let paragraph = self.build_header_paragraph(lines);
        let measured = paragraph.line_count(width);
        let resolved = u16::try_from(measured).unwrap_or(u16::MAX);
        resolved.max(ui::INLINE_HEADER_HEIGHT)
    }

    fn build_header_paragraph(&self, lines: &[Line<'static>]) -> Paragraph<'static> {
        let block = Block::default()
            .title(self.header_block_title())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(self.default_style());

        Paragraph::new(lines.to_vec())
            .style(self.default_style())
            .wrap(Wrap { trim: true })
            .block(block)
    }

    #[cfg(test)]
    fn header_height_for_width(&self, width: u16) -> u16 {
        let lines = self.header_lines();
        self.header_height_from_lines(width, &lines)
    }

    fn render_navigation(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 || area.width == 0 {
            return;
        }

        let block = Block::default()
            .title(self.navigation_block_title())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(self.default_style())
            .border_style(self.border_style());
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
        let version = if self.header_context.version.trim().is_empty() {
            fallback.version
        } else {
            self.header_context.version.clone()
        };

        let prompt = format!(
            "{}{} ",
            ui::HEADER_VERSION_PROMPT,
            ui::HEADER_VERSION_PREFIX
        );
        let version_text = format!(
            "{}{}{}",
            ui::HEADER_VERSION_LEFT_DELIMITER,
            version.trim(),
            ui::HEADER_VERSION_RIGHT_DELIMITER
        );

        let prompt_style = self.section_title_style();
        let version_style = self.header_secondary_style().add_modifier(Modifier::DIM);

        Line::from(vec![
            Span::styled(prompt, prompt_style),
            Span::styled(version_text, version_style),
        ])
    }

    fn header_title_line(&self) -> Line<'static> {
        let mut spans = Vec::new();
        let mut entries: Vec<(String, bool)> = Vec::new();

        let provider = self.header_provider_value();
        if !provider.trim().is_empty() {
            entries.push((provider, true));
        }

        let model = self.header_model_value();
        if !model.trim().is_empty() {
            entries.push((model, false));
        }

        if let Some(reasoning) = self.header_reasoning_value() {
            if !reasoning.trim().is_empty() {
                entries.push((reasoning, false));
            }
        }

        for (index, (value, emphasize)) in entries.into_iter().enumerate() {
            if index > 0 {
                spans.push(Span::styled(
                    ui::HEADER_MODE_PRIMARY_SEPARATOR.to_string(),
                    self.header_secondary_style(),
                ));
            }
            let mut style = self.header_primary_style();
            if emphasize {
                style = style.add_modifier(Modifier::BOLD);
            }
            spans.push(Span::styled(value, style));
        }

        if spans.is_empty() {
            spans.push(Span::raw(String::new()));
        }

        Line::from(spans)
    }

    fn header_provider_value(&self) -> String {
        let trimmed = self.header_context.provider.trim();
        if trimmed.is_empty() {
            InlineHeaderContext::default().provider
        } else {
            self.header_context.provider.clone()
        }
    }

    fn header_model_value(&self) -> String {
        let trimmed = self.header_context.model.trim();
        if trimmed.is_empty() {
            InlineHeaderContext::default().model
        } else {
            self.header_context.model.clone()
        }
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
        let mut spans = Vec::new();

        let mut first_section = true;
        let mode_label = self.header_mode_label();
        if !mode_label.trim().is_empty() {
            spans.push(Span::styled(
                mode_label,
                self.header_primary_style().add_modifier(Modifier::BOLD),
            ));
            first_section = false;
        }

        for value in self.header_chain_values() {
            if !first_section {
                spans.push(Span::styled(
                    ui::HEADER_MODE_SECONDARY_SEPARATOR.to_string(),
                    self.header_secondary_style(),
                ));
            }
            spans.push(Span::styled(value, self.header_primary_style()));
            first_section = false;
        }

        if spans.is_empty() {
            spans.push(Span::raw(String::new()));
        }

        Line::from(spans)
    }

    fn header_highlight_title_line(&self, highlight: &InlineHeaderHighlight) -> Line<'static> {
        let mut style = self.header_secondary_style();
        style = style.add_modifier(Modifier::BOLD);
        Line::from(vec![Span::styled(highlight.title.clone(), style)])
    }

    fn header_highlight_body_lines(&self, highlight: &InlineHeaderHighlight) -> Vec<Line<'static>> {
        if highlight.lines.is_empty() {
            return vec![Line::default()];
        }

        highlight
            .lines
            .iter()
            .map(|line| {
                Line::from(vec![Span::styled(
                    line.clone(),
                    self.header_primary_style(),
                )])
            })
            .collect()
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

    fn modal_list_highlight_style(&self) -> Style {
        let mut style = Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        if let Some(primary) = self.theme.primary.or(self.theme.foreground) {
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
            .border_type(BorderType::Rounded)
            .style(self.default_style())
            .border_style(self.border_style());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height == 0 || inner.width == 0 {
            return;
        }

        self.apply_transcript_rows(inner.height);

        let available_padding =
            ui::INLINE_SCROLLBAR_EDGE_PADDING.min(inner.width.saturating_sub(1));
        let content_width = inner.width.saturating_sub(available_padding);
        if content_width == 0 {
            return;
        }
        self.apply_transcript_width(content_width);

        let viewport_rows = inner.height as usize;
        let padding = usize::from(ui::INLINE_TRANSCRIPT_BOTTOM_PADDING);
        let total_rows = {
            let lines = self.cached_transcript_lines(content_width);
            lines.len() + padding
        };
        let (top_offset, total_rows) = self.prepare_transcript_scroll(total_rows, viewport_rows);
        let vertical_offset = top_offset.min(self.cached_max_scroll_offset);
        let clamped_offset = vertical_offset.min(u16::MAX as usize) as u16;
        self.transcript_scroll.set_offset(Position {
            x: 0,
            y: clamped_offset,
        });

        let visible_start = vertical_offset;
        let visible_end = (visible_start + viewport_rows).min(total_rows);
        let scroll_area = Rect::new(inner.x, inner.y, content_width, inner.height);
        let mut visible_lines = {
            let transcript_lines = self.cached_transcript_lines(content_width);
            if visible_start < transcript_lines.len() {
                let end = visible_end.min(transcript_lines.len());
                transcript_lines[visible_start..end]
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };
        let fill_count = viewport_rows.saturating_sub(visible_lines.len());
        if fill_count > 0 {
            visible_lines.extend((0..fill_count).map(|_| Line::default()));
        }
        let paragraph = Paragraph::new(visible_lines)
            .style(self.default_style())
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, scroll_area);

        if inner.width > content_width {
            let padding_width = inner.width - content_width;
            let padding_area = Rect::new(
                scroll_area.x + content_width,
                scroll_area.y,
                padding_width,
                inner.height,
            );
            frame.render_widget(Clear, padding_area);
        }
    }

    fn render_slash_palette(&mut self, frame: &mut Frame<'_>, viewport: Rect) {
        if viewport.height == 0 || viewport.width == 0 || self.modal.is_some() {
            self.slash_visible_rows = 0;
            return;
        }
        let suggestions = self.visible_slash_suggestions();
        if suggestions.is_empty() {
            self.slash_visible_rows = 0;
            return;
        }

        let mut width_hint = measure_text_width(ui::SLASH_PALETTE_HINT_PRIMARY);
        width_hint = width_hint.max(measure_text_width(ui::SLASH_PALETTE_HINT_SECONDARY));
        for info in suggestions.iter().take(ui::SLASH_SUGGESTION_LIMIT) {
            let mut label = format!("/{}", info.name);
            if !info.description.is_empty() {
                label.push(' ');
                label.push_str(info.description);
            }
            width_hint = width_hint.max(measure_text_width(&label));
        }

        let instructions = self.slash_palette_instructions();
        let area = compute_modal_area(viewport, width_hint, instructions.len(), true);

        frame.render_widget(Clear, area);
        let block = Block::default()
            .title(self.suggestion_block_title())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(self.default_style())
            .border_style(self.border_style());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height == 0 || inner.width == 0 {
            self.slash_visible_rows = 0;
            return;
        }

        let layout = ModalListLayout::new(inner, instructions.len());
        if let Some(text_area) = layout.text_area {
            let paragraph = Paragraph::new(instructions).wrap(Wrap { trim: true });
            frame.render_widget(paragraph, text_area);
        }

        self.slash_visible_rows = layout.list_area.height as usize;
        self.sync_slash_state();

        let list = List::new(self.slash_list_items())
            .style(self.default_style())
            .highlight_style(self.slash_highlight_style());

        frame.render_stateful_widget(list, layout.list_area, &mut self.slash_list_state);
    }

    fn slash_palette_instructions(&self) -> Vec<Line<'static>> {
        vec![
            Line::from(Span::styled(
                ui::SLASH_PALETTE_HINT_PRIMARY.to_string(),
                self.default_style(),
            )),
            Line::from(Span::styled(
                ui::SLASH_PALETTE_HINT_SECONDARY.to_string(),
                self.default_style().add_modifier(Modifier::DIM),
            )),
        ]
    }

    fn render_input(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 {
            return;
        }

        let block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_type(BorderType::Rounded)
            .style(self.default_style())
            .border_style(self.accent_style());
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

    fn render_input_line(&self) -> Line<'static> {
        let mut spans = Vec::new();
        let mut prompt_style = self.prompt_style.clone();
        if prompt_style.color.is_none() {
            prompt_style.color = self.theme.primary.or(self.theme.foreground);
        }
        let prompt_style = ratatui_style_from_inline(&prompt_style, self.theme.foreground);
        spans.push(Span::styled(self.prompt_prefix.clone(), prompt_style));

        if self.input.is_empty() {
            if let Some(placeholder) = &self.placeholder {
                let placeholder_style =
                    self.placeholder_style
                        .clone()
                        .unwrap_or_else(|| InlineTextStyle {
                            color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                            italic: true,
                            ..InlineTextStyle::default()
                        });
                let style = ratatui_style_from_inline(
                    &placeholder_style,
                    Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                );
                spans.push(Span::styled(placeholder.clone(), style));
            }
        } else {
            let accent_style = self.accent_inline_style();
            let style = ratatui_style_from_inline(&accent_style, self.theme.foreground);
            spans.push(Span::styled(self.input.clone(), style));
        }

        Line::from(spans)
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

    fn header_reserved_rows(&self) -> u16 {
        self.header_rows.max(ui::INLINE_HEADER_HEIGHT)
    }

    fn input_reserved_rows(&self) -> u16 {
        self.header_reserved_rows() + ui::INLINE_INPUT_HEIGHT
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
        if line.kind == InlineMessageKind::Agent {
            spans.extend(self.agent_prefix_spans(line));
        } else if let Some(prefix) = self.prefix_text(line.kind) {
            let style = self.prefix_style(line);
            spans.push(Span::styled(
                prefix,
                ratatui_style_from_inline(&style, self.theme.foreground),
            ));
        }

        if line.kind == InlineMessageKind::Agent {
            spans.push(Span::raw(ui::INLINE_AGENT_MESSAGE_LEFT_PADDING));
        }

        if line.segments.is_empty() {
            if spans.is_empty() {
                spans.push(Span::raw(String::new()));
            }
            return spans;
        }

        if line.kind == InlineMessageKind::Tool {
            let tool_spans = self.render_tool_segments(line);
            if tool_spans.is_empty() {
                spans.push(Span::raw(String::new()));
            } else {
                spans.extend(tool_spans);
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

    fn agent_prefix_spans(&self, line: &MessageLine) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let prefix_style =
            ratatui_style_from_inline(&self.prefix_style(line), self.theme.foreground);
        if !ui::INLINE_AGENT_QUOTE_PREFIX.is_empty() {
            spans.push(Span::styled(
                ui::INLINE_AGENT_QUOTE_PREFIX.to_string(),
                prefix_style,
            ));
        }

        if let Some(label) = self.labels.agent.clone() {
            if !label.is_empty() {
                let label_style =
                    ratatui_style_from_inline(&self.prefix_style(line), self.theme.foreground);
                spans.push(Span::styled(label, label_style));
            }
        }

        spans
    }

    fn render_tool_segments(&self, line: &MessageLine) -> Vec<Span<'static>> {
        let mut combined = String::new();
        for segment in &line.segments {
            combined.push_str(segment.text.as_str());
        }

        if combined.is_empty() {
            return Vec::new();
        }

        let is_detail = line.segments.iter().any(|segment| segment.style.italic);
        if is_detail {
            return self.render_tool_detail_line(&combined);
        }

        self.render_tool_header_line(&combined)
    }

    fn render_tool_detail_line(&self, text: &str) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let border_style =
            ratatui_style_from_inline(&self.tool_border_style(), self.theme.foreground)
                .add_modifier(Modifier::DIM);
        spans.push(Span::styled(
            format!("{} ", Self::tool_border_symbol()),
            border_style,
        ));

        let mut body_style = InlineTextStyle::default();
        body_style.color = self.theme.tool_body.or(self.theme.foreground);
        body_style.italic = true;
        spans.push(Span::styled(
            text.trim_start().to_string(),
            ratatui_style_from_inline(&body_style, self.theme.foreground),
        ));

        spans
    }

    fn render_tool_header_line(&self, text: &str) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        let indent_len = text.chars().take_while(|ch| ch.is_whitespace()).count();
        let indent: String = text.chars().take(indent_len).collect();
        let mut remaining = if indent_len < text.len() {
            &text[indent_len..]
        } else {
            ""
        };

        if !indent.is_empty() {
            let mut indent_style = InlineTextStyle::default();
            indent_style.color = self.theme.tool_body.or(self.theme.foreground);
            spans.push(Span::styled(
                indent,
                ratatui_style_from_inline(&indent_style, self.theme.foreground),
            ));
            if indent_len < text.len() {
                remaining = &text[indent_len..];
            } else {
                remaining = "";
            }
        }

        if remaining.is_empty() {
            return spans;
        }

        let mut name_end = remaining.len();
        for (index, character) in remaining.char_indices() {
            if character.is_whitespace() {
                name_end = index;
                break;
            }
        }

        let (name, tail) = remaining.split_at(name_end);
        if !name.is_empty() {
            let mut name_style = InlineTextStyle::default();
            name_style.color = self
                .theme
                .tool_accent
                .or(self.theme.primary)
                .or(self.theme.foreground);
            name_style.bold = true;
            spans.push(Span::styled(
                name.to_string(),
                ratatui_style_from_inline(&name_style, self.theme.foreground),
            ));
        }

        if !tail.is_empty() {
            let mut body_style = InlineTextStyle::default();
            body_style.color = self.theme.tool_body.or(self.theme.foreground);
            body_style.italic = true;
            spans.push(Span::styled(
                tail.to_string(),
                ratatui_style_from_inline(&body_style, self.theme.foreground),
            ));
        }

        spans
    }

    fn tool_border_symbol() -> &'static str {
        static SYMBOL: OnceLock<String> = OnceLock::new();
        SYMBOL
            .get_or_init(|| {
                let block = Block::default().borders(Borders::LEFT);
                let area = Rect::new(0, 0, 1, 1);
                let mut buffer = Buffer::empty(area);
                block.render(area, &mut buffer);
                buffer
                    .cell((0, 0))
                    .map(|cell| cell.symbol().to_string())
                    .filter(|symbol| !symbol.is_empty())
                    .unwrap_or_else(|| "│".to_string())
            })
            .as_str()
    }

    fn tool_border_style(&self) -> InlineTextStyle {
        self.border_inline_style()
    }

    fn default_style(&self) -> Style {
        let mut style = Style::default();
        if let Some(foreground) = self.theme.foreground.map(ratatui_color_from_ansi) {
            style = style.fg(foreground);
        }
        style
    }

    fn ensure_prompt_style_color(&mut self) {
        if self.prompt_style.color.is_none() {
            self.prompt_style.color = self.theme.primary.or(self.theme.foreground);
        }
    }

    fn accent_inline_style(&self) -> InlineTextStyle {
        InlineTextStyle {
            color: self.theme.primary.or(self.theme.foreground),
            ..InlineTextStyle::default()
        }
    }

    fn accent_style(&self) -> Style {
        ratatui_style_from_inline(&self.accent_inline_style(), self.theme.foreground)
    }

    fn border_inline_style(&self) -> InlineTextStyle {
        InlineTextStyle {
            color: self.theme.secondary.or(self.theme.foreground),
            ..InlineTextStyle::default()
        }
    }

    fn border_style(&self) -> Style {
        ratatui_style_from_inline(&self.border_inline_style(), self.theme.foreground)
            .add_modifier(Modifier::DIM)
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
            list: None,
            restore_input: self.input_enabled,
            restore_cursor: self.cursor_visible,
        };
        self.input_enabled = false;
        self.cursor_visible = false;
        self.modal = Some(state);
        self.mark_dirty();
    }

    fn show_list_modal(
        &mut self,
        title: String,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
    ) {
        let list_state = ModalListState::new(items, selected);
        let state = ModalState {
            title,
            lines,
            list: Some(list_state),
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

    fn render_modal(&mut self, frame: &mut Frame<'_>, viewport: Rect) {
        if viewport.width == 0 || viewport.height == 0 {
            return;
        }

        let styles = self.modal_render_styles();
        let Some(modal) = self.modal.as_mut() else {
            return;
        };
        let width_hint = modal_content_width(&modal.lines, modal.list.as_ref());
        let area = compute_modal_area(
            viewport,
            width_hint,
            modal.lines.len(),
            modal.list.is_some(),
        );

        frame.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Span::styled(modal.title.clone(), styles.title.clone()))
            .border_style(styles.border.clone());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let text_lines: Vec<Line<'static>> = modal
            .lines
            .iter()
            .map(|line| Line::from(Span::raw(line.clone())))
            .collect();

        match modal.list.as_mut() {
            Some(list) => render_modal_list(frame, inner, &text_lines, list, &styles),
            None => {
                let paragraph = Paragraph::new(text_lines).wrap(Wrap { trim: false });
                frame.render_widget(paragraph, inner);
            }
        }
    }

    fn modal_render_styles(&self) -> ModalRenderStyles {
        ModalRenderStyles {
            border: self.border_style(),
            highlight: self.modal_list_highlight_style(),
            badge: self.section_title_style().add_modifier(Modifier::DIM),
            header: self.section_title_style(),
            selectable: self.default_style().add_modifier(Modifier::BOLD),
            detail: self.default_style().add_modifier(Modifier::DIM),
            title: Style::default().add_modifier(Modifier::BOLD),
        }
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

        if let Some(modal) = self.modal.as_mut() {
            if let Some(list) = modal.list.as_mut() {
                match key.code {
                    KeyCode::Up => {
                        list.select_previous();
                        self.mark_dirty();
                        return None;
                    }
                    KeyCode::Down => {
                        list.select_next();
                        self.mark_dirty();
                        return None;
                    }
                    KeyCode::Enter => {
                        if let Some(selection) = list.current_selection() {
                            let selection_clone = selection.clone();
                            self.close_modal();
                            return Some(InlineEvent::ListModalSubmit(selection_clone));
                        }
                        return None;
                    }
                    KeyCode::Esc => {
                        self.close_modal();
                        return Some(InlineEvent::ListModalCancel);
                    }
                    _ => {}
                }
            }
        }

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
            InlineMessageKind::Agent => None,
            InlineMessageKind::Policy => self.labels.agent.clone(),
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
        let revision = self.next_revision();
        self.lines.push(MessageLine {
            kind,
            segments,
            revision,
        });
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
            let revision = self.next_revision();
            self.lines.push(MessageLine {
                kind,
                segments,
                revision,
            });
        }
        self.invalidate_scroll_metrics();
        self.adjust_scroll_after_change(previous_max_offset);
    }

    fn append_text(&mut self, kind: InlineMessageKind, text: &str, style: &InlineTextStyle) {
        if text.is_empty() {
            return;
        }

        let mut appended = false;

        let mut mark_revision = false;
        {
            if let Some(line) = self.lines.last_mut() {
                if line.kind == kind {
                    if let Some(last) = line.segments.last_mut() {
                        if last.style == *style {
                            last.text.push_str(text);
                            appended = true;
                            mark_revision = true;
                        }
                    }
                    if !appended {
                        line.segments.push(InlineSegment {
                            text: text.to_string(),
                            style: style.clone(),
                        });
                        appended = true;
                        mark_revision = true;
                    }
                }
            }
        }

        if mark_revision {
            let revision = self.next_revision();
            if let Some(line) = self.lines.last_mut() {
                if line.kind == kind {
                    line.revision = revision;
                }
            }
        }

        if !appended {
            let revision = self.next_revision();
            self.lines.push(MessageLine {
                kind,
                segments: vec![InlineSegment {
                    text: text.to_string(),
                    style: style.clone(),
                }],
                revision,
            });
        }

        self.invalidate_scroll_metrics();
    }

    fn start_line(&mut self, kind: InlineMessageKind) {
        self.push_line(kind, Vec::new());
    }

    fn reset_line(&mut self, kind: InlineMessageKind) {
        let mut cleared = false;
        {
            if let Some(line) = self.lines.last_mut() {
                if line.kind == kind {
                    line.segments.clear();
                    cleared = true;
                }
            }
        }
        if cleared {
            let revision = self.next_revision();
            if let Some(line) = self.lines.last_mut() {
                if line.kind == kind {
                    line.revision = revision;
                }
            }
            self.invalidate_scroll_metrics();
            return;
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
        self.invalidate_transcript_cache();
    }

    fn invalidate_transcript_cache(&mut self) {
        self.transcript_cache = None;
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

        let padding = usize::from(ui::INLINE_TRANSCRIPT_BOTTOM_PADDING);
        let total_rows = self.cached_transcript_lines(self.transcript_width).len() + padding;
        let max_offset = total_rows.saturating_sub(viewport_rows);
        self.cached_max_scroll_offset = max_offset;
        self.scroll_metrics_dirty = false;
    }

    fn cached_transcript_lines(&mut self, width: u16) -> &[Line<'static>] {
        let width_mismatch = self
            .transcript_cache
            .as_ref()
            .map(|cache| cache.width != width)
            .unwrap_or(true);

        let mut updates: Vec<Option<Vec<Line<'static>>>> = Vec::with_capacity(self.lines.len());
        for (index, line) in self.lines.iter().enumerate() {
            let revision_matches = self
                .transcript_cache
                .as_ref()
                .and_then(|cache| cache.messages.get(index))
                .map(|message| message.revision == line.revision)
                .unwrap_or(false);

            if width_mismatch || !revision_matches {
                updates.push(Some(self.reflow_message_lines(line, width)));
            } else {
                updates.push(None);
            }
        }

        let cache = self
            .transcript_cache
            .get_or_insert_with(|| TranscriptReflowCache {
                width,
                flattened: Vec::new(),
                messages: Vec::new(),
            });

        cache.width = width;

        if cache.messages.len() > self.lines.len() {
            cache.messages.truncate(self.lines.len());
        }
        if cache.messages.len() < self.lines.len() {
            cache
                .messages
                .resize_with(self.lines.len(), CachedMessage::default);
        }

        cache.flattened.clear();
        for (index, line) in self.lines.iter().enumerate() {
            if let Some(new_lines) = updates[index].take() {
                let message_cache = &mut cache.messages[index];
                message_cache.revision = line.revision;
                message_cache.lines = new_lines;
            }
            let message_cache = &cache.messages[index];
            cache.flattened.extend(message_cache.lines.iter().cloned());
        }

        if cache.flattened.is_empty() {
            cache.flattened.push(Line::default());
        }

        cache.flattened.as_slice()
    }

    #[cfg(test)]
    fn reflow_transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        if width == 0 {
            let mut lines: Vec<Line<'static>> = self
                .lines
                .iter()
                .map(|line| Line::from(self.render_message_spans(line)))
                .collect();
            if lines.is_empty() {
                lines.push(Line::default());
            }
            return lines;
        }

        let mut wrapped_lines = Vec::new();
        for line in &self.lines {
            wrapped_lines.extend(self.reflow_message_lines(line, width));
        }

        if wrapped_lines.is_empty() {
            wrapped_lines.push(Line::default());
        }

        wrapped_lines
    }

    fn reflow_message_lines(&self, message: &MessageLine, width: u16) -> Vec<Line<'static>> {
        let spans = self.render_message_spans(message);
        let base_line = Line::from(spans);
        if width == 0 {
            return vec![base_line];
        }

        let mut wrapped = Vec::new();
        let max_width = width as usize;

        if message.kind == InlineMessageKind::User && max_width > 0 {
            wrapped.push(self.message_divider_line(max_width, message.kind));
        }

        let mut lines = self.wrap_line(base_line, max_width);
        if !lines.is_empty() {
            lines = self.justify_wrapped_lines(lines, max_width, message.kind);
        }
        if lines.is_empty() {
            lines.push(Line::default());
        }
        wrapped.extend(lines.into_iter());

        if message.kind == InlineMessageKind::User && max_width > 0 {
            wrapped.push(self.message_divider_line(max_width, message.kind));
        }

        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        wrapped
    }

    fn message_divider_line(&self, width: usize, kind: InlineMessageKind) -> Line<'static> {
        if width == 0 {
            return Line::default();
        }

        let content = ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(width);
        let style = self.message_divider_style(kind);
        Line::from(vec![Span::styled(content, style)])
    }

    fn message_divider_style(&self, kind: InlineMessageKind) -> Style {
        let mut style = InlineTextStyle::default();
        if kind == InlineMessageKind::User {
            style.color = self.theme.primary.or(self.theme.foreground);
        } else {
            style.color = self.text_fallback(kind).or(self.theme.foreground);
        }
        let resolved = ratatui_style_from_inline(&style, self.theme.foreground);
        if kind == InlineMessageKind::User {
            resolved
        } else {
            resolved.add_modifier(Modifier::DIM)
        }
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

    fn justify_wrapped_lines(
        &self,
        lines: Vec<Line<'static>>,
        max_width: usize,
        kind: InlineMessageKind,
    ) -> Vec<Line<'static>> {
        if max_width == 0 || kind != InlineMessageKind::Agent {
            return lines;
        }

        let total = lines.len();
        let mut justified = Vec::with_capacity(total);
        let mut in_fenced_block = false;
        for (index, line) in lines.into_iter().enumerate() {
            let is_last = index + 1 == total;
            let mut next_in_fenced_block = in_fenced_block;
            let is_fence_line = {
                let line_text_storage: std::borrow::Cow<'_, str> = if line.spans.len() == 1 {
                    std::borrow::Cow::Borrowed(line.spans[0].content.as_ref())
                } else {
                    std::borrow::Cow::Owned(
                        line.spans
                            .iter()
                            .map(|span| span.content.as_ref())
                            .collect::<String>(),
                    )
                };
                let line_text = line_text_storage.as_ref();
                let trimmed_start = line_text.trim_start();
                trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~")
            };
            if is_fence_line {
                next_in_fenced_block = !in_fenced_block;
            }

            if !in_fenced_block
                && !is_fence_line
                && self.should_justify_message_line(&line, max_width, is_last)
            {
                justified.push(self.justify_message_line(&line, max_width));
            } else {
                justified.push(line);
            }

            in_fenced_block = next_in_fenced_block;
        }

        justified
    }

    fn should_justify_message_line(
        &self,
        line: &Line<'static>,
        max_width: usize,
        is_last: bool,
    ) -> bool {
        if is_last || max_width == 0 {
            return false;
        }
        if line.spans.len() != 1 {
            return false;
        }
        let text = line.spans[0].content.as_ref();
        if text.trim().is_empty() {
            return false;
        }
        if text.starts_with(char::is_whitespace) {
            return false;
        }
        let trimmed = text.trim();
        if trimmed.starts_with(|ch: char| matches!(ch, '-' | '*' | '`' | '>' | '#')) {
            return false;
        }
        if trimmed.contains("```") {
            return false;
        }
        let width = UnicodeWidthStr::width(trimmed);
        if width >= max_width || width < max_width / 2 {
            return false;
        }

        justify_plain_text(text, max_width).is_some()
    }

    fn justify_message_line(&self, line: &Line<'static>, max_width: usize) -> Line<'static> {
        let span = &line.spans[0];
        if let Some(justified) = justify_plain_text(span.content.as_ref(), max_width) {
            Line::from(vec![Span::styled(justified, span.style)])
        } else {
            line.clone()
        }
    }

    fn prepare_transcript_scroll(
        &mut self,
        total_rows: usize,
        viewport_rows: usize,
    ) -> (usize, usize) {
        let viewport = viewport_rows.max(1);
        let clamped_total = total_rows.max(1);
        let max_offset = clamped_total.saturating_sub(viewport);
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
        self.cached_max_scroll_offset = max_offset;
        self.scroll_metrics_dirty = false;

        let top_offset = max_offset.saturating_sub(self.scroll_offset);
        (top_offset, clamped_total)
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

fn justify_plain_text(text: &str, max_width: usize) -> Option<String> {
    let trimmed = text.trim();
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() <= 1 {
        return None;
    }

    let total_word_width: usize = words.iter().map(|word| UnicodeWidthStr::width(*word)).sum();
    if total_word_width >= max_width {
        return None;
    }

    let gaps = words.len() - 1;
    let spaces_needed = max_width.saturating_sub(total_word_width);
    if spaces_needed <= gaps {
        return None;
    }

    let base_space = spaces_needed / gaps;
    if base_space == 0 {
        return None;
    }
    let extra = spaces_needed % gaps;

    let mut output = String::with_capacity(max_width + gaps);
    for (index, word) in words.iter().enumerate() {
        output.push_str(word);
        if index < gaps {
            let mut count = base_space;
            if index < extra {
                count += 1;
            }
            for _ in 0..count {
                output.push(' ');
            }
        }
    }

    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        style::{Color, Modifier},
        text::Line,
    };

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

    fn themed_inline_colors() -> InlineTheme {
        let mut theme = InlineTheme::default();
        theme.foreground = Some(AnsiColorEnum::Rgb(RgbColor(0xEE, 0xEE, 0xEE)));
        theme.tool_accent = Some(AnsiColorEnum::Rgb(RgbColor(0xBF, 0x45, 0x45)));
        theme.tool_body = Some(AnsiColorEnum::Rgb(RgbColor(0xAA, 0x88, 0x88)));
        theme.primary = Some(AnsiColorEnum::Rgb(RgbColor(0x88, 0x88, 0x88)));
        theme.secondary = Some(AnsiColorEnum::Rgb(RgbColor(0x77, 0x99, 0xAA)));
        theme
    }

    fn session_with_input(input: &str, cursor: usize) -> Session {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
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
        let offset = usize::from(session.transcript_scroll.offset().y);
        let lines = session.reflow_transcript_lines(width);

        let start = offset.min(lines.len());
        let mut collected: Vec<String> = lines
            .into_iter()
            .skip(start)
            .take(viewport)
            .map(|line| {
                line.spans
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect();
        let filler = viewport.saturating_sub(collected.len());
        collected.extend((0..filler).map(|_| String::new()));
        collected
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.clone().into_owned())
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
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

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
    fn streaming_segments_render_incrementally() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        session.push_line(InlineMessageKind::Agent, vec![make_segment("")]);

        session.append_inline(InlineMessageKind::Agent, make_segment("Hello"));
        let first = visible_transcript(&mut session);
        assert!(first.iter().any(|line| line.contains("Hello")));

        session.append_inline(InlineMessageKind::Agent, make_segment(" world"));
        let second = visible_transcript(&mut session);
        assert!(second.iter().any(|line| line.contains("Hello world")));
    }

    #[test]
    fn page_up_reveals_prior_lines_until_buffer_start() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

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
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

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

    #[test]
    fn scroll_end_displays_full_final_paragraph() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        let total = LINE_COUNT * 5;

        for index in 1..=total {
            let label = format!("{LABEL_PREFIX}-{index}");
            let text = format!("{label}\n{label}-continued");
            session.push_line(InlineMessageKind::Agent, vec![make_segment(text.as_str())]);
        }

        // Prime layout to ensure transcript dimensions are measured.
        visible_transcript(&mut session);

        for _ in 0..total {
            session.scroll_page_up();
            if session.scroll_offset == session.current_max_scroll_offset() {
                break;
            }
        }
        assert!(session.scroll_offset > 0);

        for _ in 0..total {
            session.scroll_page_down();
            if session.scroll_offset == 0 {
                break;
            }
        }

        assert_eq!(session.scroll_offset, 0);

        let view = visible_transcript(&mut session);
        let expected_tail = format!("{LABEL_PREFIX}-{total}-continued");
        assert!(
            view.iter().any(|line| line.contains(&expected_tail)),
            "expected final paragraph tail `{expected_tail}` to appear, got {view:?}"
        );
        assert!(
            view.last().map_or(false, |line| line.is_empty()),
            "expected bottom padding row to be blank, got {view:?}"
        );
    }

    #[test]
    fn user_messages_render_with_dividers() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        session.push_line(InlineMessageKind::User, vec![make_segment("Hi")]);

        let width = 10;
        let lines = session.reflow_transcript_lines(width);
        assert!(
            lines.len() >= 3,
            "expected dividers around the user message"
        );

        let top = line_text(&lines[0]);
        let bottom = line_text(
            lines
                .last()
                .expect("user message should have closing divider"),
        );
        let expected = ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(width as usize);

        assert_eq!(top, expected);
        assert_eq!(bottom, expected);
    }

    #[test]
    fn header_lines_include_provider_model_and_metadata() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        session.header_context.provider = format!("{}xAI", ui::HEADER_PROVIDER_PREFIX);
        session.header_context.model = format!("{}grok-4-fast", ui::HEADER_MODEL_PREFIX);
        session.header_context.reasoning = format!("{}medium", ui::HEADER_REASONING_PREFIX);
        session.header_context.mode = ui::HEADER_MODE_AUTO.to_string();
        session.header_context.workspace_trust = format!("{}full auto", ui::HEADER_TRUST_PREFIX);
        session.header_context.tools =
            format!("{}allow 11 · prompt 7 · deny 0", ui::HEADER_TOOLS_PREFIX);
        session.header_context.languages = format!("{}Rust:177", ui::HEADER_LANGUAGES_PREFIX);
        session.header_context.mcp = format!("{}enabled", ui::HEADER_MCP_PREFIX);

        let title_line = session.header_title_line();
        let title_text: String = title_line
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();
        assert!(title_text.contains(ui::HEADER_PROVIDER_PREFIX));
        assert!(title_text.contains(ui::HEADER_MODEL_PREFIX));
        assert!(title_text.contains(ui::HEADER_REASONING_PREFIX));

        let meta_line = session.header_meta_line();
        let meta_text: String = meta_line
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();
        assert!(meta_text.contains(ui::HEADER_MODE_AUTO));
        assert!(meta_text.contains(ui::HEADER_TRUST_PREFIX));
        assert!(meta_text.contains(ui::HEADER_TOOLS_PREFIX));
        assert!(meta_text.contains(ui::HEADER_LANGUAGES_PREFIX));
        assert!(meta_text.contains(ui::HEADER_MCP_PREFIX));
        assert!(!meta_text.contains(ui::HEADER_STATUS_LABEL));
        assert!(!meta_text.contains(ui::HEADER_MESSAGES_LABEL));
        assert!(!meta_text.contains(ui::HEADER_INPUT_LABEL));
    }

    #[test]
    fn header_height_expands_when_wrapping_required() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        session.header_context.provider = format!(
            "{}Example Provider With Extended Label",
            ui::HEADER_PROVIDER_PREFIX
        );
        session.header_context.model = format!(
            "{}ExampleModelIdentifierWithDetail",
            ui::HEADER_MODEL_PREFIX
        );
        session.header_context.reasoning = format!("{}medium", ui::HEADER_REASONING_PREFIX);
        session.header_context.mode = ui::HEADER_MODE_AUTO.to_string();
        session.header_context.workspace_trust = format!("{}full auto", ui::HEADER_TRUST_PREFIX);
        session.header_context.tools =
            format!("{}allow 11 · prompt 7 · deny 0", ui::HEADER_TOOLS_PREFIX);
        session.header_context.languages = format!(
            "{}Rust:177, JavaScript:4, Python:2, Go:3, TypeScript:5",
            ui::HEADER_LANGUAGES_PREFIX
        );
        session.header_context.mcp = format!("{}enabled", ui::HEADER_MCP_PREFIX);

        let wide = session.header_height_for_width(120);
        let narrow = session.header_height_for_width(40);

        assert!(
            narrow > wide,
            "expected narrower width to require more header rows"
        );
    }

    #[test]
    fn agent_messages_include_left_padding() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        session.push_line(InlineMessageKind::Agent, vec![make_segment("Response")]);

        let lines = session.reflow_transcript_lines(VIEW_WIDTH);
        let message_line = lines
            .iter()
            .map(line_text)
            .find(|text| text.contains("Response"))
            .expect("agent message should be visible");

        let expected_prefix = format!(
            "{}{}",
            ui::INLINE_AGENT_QUOTE_PREFIX,
            ui::INLINE_AGENT_MESSAGE_LEFT_PADDING
        );

        assert!(
            message_line.starts_with(&expected_prefix),
            "agent message should include left padding",
        );
        assert!(
            !message_line.contains('│'),
            "agent message should not render a left border",
        );
    }

    #[test]
    fn agent_label_uses_accent_color_without_border() {
        let accent = AnsiColorEnum::Rgb(RgbColor(0x12, 0x34, 0x56));
        let mut theme = InlineTheme::default();
        theme.primary = Some(accent);

        let mut session = Session::new(theme, None, VIEW_ROWS, true);
        session.labels.agent = Some("Agent".to_string());
        session.push_line(InlineMessageKind::Agent, vec![make_segment("Response")]);

        let line = session
            .lines
            .last()
            .cloned()
            .expect("agent message should be available");
        let spans = session.render_message_spans(&line);

        assert!(spans.len() >= 3);

        let label_span = &spans[0];
        assert_eq!(label_span.content.clone().into_owned(), "Agent");
        assert_eq!(label_span.style.fg, Some(Color::Rgb(0x12, 0x34, 0x56)));

        let padding_span = &spans[1];
        assert_eq!(
            padding_span.content.clone().into_owned(),
            ui::INLINE_AGENT_MESSAGE_LEFT_PADDING
        );

        assert!(
            !spans
                .iter()
                .any(|span| span.content.clone().into_owned().contains('│')),
            "agent prefix should not render a left border",
        );
        assert!(
            !spans
                .iter()
                .any(|span| span.content.clone().into_owned().contains('✦')),
            "agent prefix should not include decorative symbols",
        );
    }

    #[test]
    fn timeline_hidden_keeps_navigation_unselected() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, false);
        session.push_line(InlineMessageKind::Agent, vec![make_segment("Response")]);

        let backend = TestBackend::new(VIEW_WIDTH, VIEW_ROWS);
        let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
        terminal
            .draw(|frame| session.render(frame))
            .expect("failed to render session with hidden timeline");

        assert!(session.navigation_state.selected().is_none());
    }

    #[test]
    fn timeline_visible_selects_latest_item() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        session.push_line(InlineMessageKind::Agent, vec![make_segment("First")]);
        session.push_line(InlineMessageKind::Agent, vec![make_segment("Second")]);

        let backend = TestBackend::new(VIEW_WIDTH, VIEW_ROWS);
        let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
        terminal
            .draw(|frame| session.render(frame))
            .expect("failed to render session with timeline");

        assert_eq!(session.navigation_state.selected(), Some(1));
    }

    #[test]
    fn tool_header_applies_accent_and_italic_tail() {
        let theme = themed_inline_colors();
        let mut session = Session::new(theme, None, VIEW_ROWS, true);
        session.push_line(
            InlineMessageKind::Tool,
            vec![InlineSegment {
                text: "  [shell] executing".to_string(),
                style: InlineTextStyle::default(),
            }],
        );

        let line = session
            .lines
            .last()
            .cloned()
            .expect("tool header line should exist");
        let spans = session.render_message_spans(&line);

        assert!(spans.len() >= 3);
        assert_eq!(spans[0].content.clone().into_owned(), "  ");
        assert_eq!(spans[1].content.clone().into_owned(), "[shell]");
        assert_eq!(spans[1].style.fg, Some(Color::Rgb(0xBF, 0x45, 0x45)));
        assert!(spans[2].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn tool_detail_renders_with_border_and_body_style() {
        let theme = themed_inline_colors();
        let mut session = Session::new(theme, None, VIEW_ROWS, true);
        let mut detail_style = InlineTextStyle::default();
        detail_style.italic = true;
        session.push_line(
            InlineMessageKind::Tool,
            vec![InlineSegment {
                text: "    result line".to_string(),
                style: detail_style,
            }],
        );

        let line = session
            .lines
            .last()
            .cloned()
            .expect("tool detail line should exist");
        let spans = session.render_message_spans(&line);

        assert!(spans.len() >= 2);
        let border_span = &spans[0];
        assert_eq!(
            border_span.content.clone().into_owned(),
            format!("{} ", Session::tool_border_symbol())
        );
        assert_eq!(border_span.style.fg, Some(Color::Rgb(0x77, 0x99, 0xAA)));
        assert!(
            border_span.style.add_modifier.contains(Modifier::DIM),
            "tool border should use dimmed styling"
        );

        let body_span = &spans[1];
        assert!(body_span.style.add_modifier.contains(Modifier::ITALIC));
        assert_eq!(body_span.content.clone().into_owned(), "result line");
    }
}
