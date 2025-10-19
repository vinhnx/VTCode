use std::{cmp::min, fmt::Write, mem};

use anstyle::{AnsiColor, Color as AnsiColorEnum, RgbColor};
use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use line_clipping::cohen_sutherland::clip_line;
use line_clipping::{LineSegment, Point, Window};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use tokio::sync::mpsc::UnboundedSender;
use tui_popup::{Popup, PopupState, SizedWrapper};
use tui_scrollview::ScrollViewState;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::types::{
    InlineCommand, InlineEvent, InlineHeaderContext, InlineHeaderHighlight, InlineListItem,
    InlineListSearchConfig, InlineListSelection, InlineMessageKind, InlineSegment, InlineTextStyle,
    InlineTheme, SecurePromptConfig,
};
use crate::config::constants::ui;

mod message;
mod modal;
mod slash_palette;

use self::message::{MessageLabels, MessageLine};
use self::modal::{
    ModalBodyContext, ModalKeyModifiers, ModalListKeyResult, ModalListLayout, ModalListState,
    ModalRenderStyles, ModalSearchState, ModalState, compute_modal_area, modal_content_width,
    render_modal_body,
};
use self::slash_palette::{SlashPalette, SlashPaletteUpdate, command_prefix, command_range};
use crate::prompts::CustomPromptRegistry;

const USER_PREFIX: &str = "❯ ";
const PLACEHOLDER_COLOR: RgbColor = RgbColor(0x88, 0x88, 0x88);

fn measure_text_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text) as u16
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

struct InputRender {
    text: Text<'static>,
    cursor_x: u16,
    cursor_y: u16,
}

#[derive(Default)]
struct InputLineBuffer {
    prefix: String,
    text: String,
    prefix_width: u16,
    text_width: u16,
}

impl InputLineBuffer {
    fn new(prefix: String, prefix_width: u16) -> Self {
        Self {
            prefix,
            text: String::new(),
            prefix_width,
            text_width: 0,
        }
    }
}

struct InputLayout {
    buffers: Vec<InputLineBuffer>,
    cursor_line_idx: usize,
    cursor_column: u16,
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
    input_status_left: Option<String>,
    input_status_right: Option<String>,
    input: String,
    cursor: usize,
    slash_palette: SlashPalette,
    navigation_state: ListState,
    input_enabled: bool,
    cursor_visible: bool,
    needs_redraw: bool,
    should_exit: bool,
    view_rows: u16,
    input_height: u16,
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
    in_tool_code_fence: bool,
    input_history: Vec<String>,
    input_history_index: Option<usize>,
    input_history_draft: Option<String>,
    custom_prompts: Option<CustomPromptRegistry>,
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
        let reserved_rows = initial_header_rows + Self::input_block_height_for_lines(1);
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
            input_status_left: None,
            input_status_right: None,
            input: String::new(),
            cursor: 0,
            slash_palette: SlashPalette::new(),
            navigation_state: ListState::default(),
            input_enabled: true,
            cursor_visible: true,
            needs_redraw: true,
            should_exit: false,
            view_rows: resolved_rows,
            input_height: Self::input_block_height_for_lines(1),
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
            in_tool_code_fence: false,
            input_history: Vec::new(),
            input_history_index: None,
            input_history_draft: None,
            custom_prompts: None,
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
            InlineCommand::SetInputStatus { left, right } => {
                self.input_status_left = left;
                self.input_status_right = right;
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
                self.scroll_offset = 0;
                self.reset_history_navigation();
                self.update_slash_suggestions();
            }
            InlineCommand::ClearInput => {
                self.clear_input();
            }
            InlineCommand::ForceRedraw => {
                self.mark_dirty();
            }
            InlineCommand::ShowModal {
                title,
                lines,
                secure_prompt,
            } => {
                self.show_modal(title, lines, secure_prompt);
            }
            InlineCommand::ShowListModal {
                title,
                lines,
                items,
                selected,
                search,
            } => {
                self.show_list_modal(title, lines, items, selected, search);
            }
            InlineCommand::CloseModal => {
                self.close_modal();
            }
            InlineCommand::SetCustomPrompts { registry } => {
                self.set_custom_prompts(registry);
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
            CrosstermEvent::Paste(content) => {
                if self.input_enabled {
                    self.insert_text(&content);
                    self.mark_dirty();
                } else if let Some(modal) = self.modal.as_mut() {
                    if let (Some(list), Some(search)) = (modal.list.as_mut(), modal.search.as_mut())
                    {
                        search.insert(&content);
                        list.apply_search(&search.query);
                        self.mark_dirty();
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

        let status_height = if viewport.width > 0 && self.has_input_status() {
            1
        } else {
            0
        };
        let inner_width = viewport.width.saturating_sub(2);
        let desired_lines = self.desired_input_lines(inner_width);
        let block_height = Self::input_block_height_for_lines(desired_lines);
        let input_height = block_height.saturating_add(status_height);
        self.apply_input_height(input_height);

        let mut constraints = vec![Constraint::Length(header_height), Constraint::Min(1)];
        constraints.push(Constraint::Length(input_height));

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

        // Prioritize suggestions when input is empty or starts with /
        if self.should_show_suggestions() {
            if let Some(suggestions) = self.header_suggestions_line() {
                lines.push(suggestions);
            }
        } else if let Some(highlights) = self.header_highlights_line() {
            lines.push(highlights);
        }

        lines.truncate(3);
        lines
    }

    fn header_height_from_lines(&self, width: u16, lines: &[Line<'static>]) -> u16 {
        if width == 0 {
            return self.header_rows.max(ui::INLINE_HEADER_HEIGHT);
        }

        let paragraph = self.build_header_paragraph(lines);
        let measured = paragraph.line_count(width);
        let resolved = u16::try_from(measured).unwrap_or(u16::MAX);
        // Limit to max 3 lines to accommodate suggestions
        resolved.min(3).max(ui::INLINE_HEADER_HEIGHT)
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
            .borders(Borders::NONE)
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
        // First line: badge-style provider + model + reasoning summary
        let mut spans = Vec::new();

        let provider = self.header_provider_short_value();
        let model = self.header_model_short_value();
        let reasoning = self.header_reasoning_short_value();

        if !provider.is_empty() {
            let badge = format!("[{}]", provider.to_uppercase());
            let mut style = self.header_primary_style();
            style = style.add_modifier(Modifier::BOLD);
            spans.push(Span::styled(badge, style));
        }

        if !model.is_empty() {
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            let mut style = self.header_primary_style();
            style = style.add_modifier(Modifier::ITALIC);
            spans.push(Span::styled(model, style));
        }

        if !reasoning.is_empty() {
            if !spans.is_empty() {
                spans.push(Span::raw(" "));
            }
            let mut style = self.header_secondary_style();
            style = style.add_modifier(Modifier::ITALIC | Modifier::DIM);
            spans.push(Span::styled(format!("({})", reasoning), style));
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

    fn header_mode_short_label(&self) -> String {
        let full = self.header_mode_label();
        let value = full.trim();
        if value.eq_ignore_ascii_case(ui::HEADER_MODE_AUTO) {
            return "Auto".to_string();
        }
        if value.eq_ignore_ascii_case(ui::HEADER_MODE_INLINE) {
            return "Inline".to_string();
        }
        if value.eq_ignore_ascii_case(ui::HEADER_MODE_ALTERNATE) {
            return "Alternate".to_string();
        }
        let compact = value
            .strip_suffix(ui::HEADER_MODE_FULL_AUTO_SUFFIX)
            .unwrap_or(value)
            .trim();
        compact.to_string()
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

    fn strip_prefix<'a>(value: &'a str, prefix: &str) -> &'a str {
        value.strip_prefix(prefix).unwrap_or(value)
    }

    fn header_provider_short_value(&self) -> String {
        let value = self.header_provider_value();
        Self::strip_prefix(&value, ui::HEADER_PROVIDER_PREFIX)
            .trim()
            .to_string()
    }

    fn header_model_short_value(&self) -> String {
        let value = self.header_model_value();
        Self::strip_prefix(&value, ui::HEADER_MODEL_PREFIX)
            .trim()
            .to_string()
    }

    fn header_reasoning_short_value(&self) -> String {
        let value = self.header_reasoning_value().unwrap_or_else(String::new);
        Self::strip_prefix(&value, ui::HEADER_REASONING_PREFIX)
            .trim()
            .to_string()
    }

    fn header_chain_values(&self) -> Vec<String> {
        let defaults = InlineHeaderContext::default();
        let fields = [
            (
                &self.header_context.workspace_trust,
                defaults.workspace_trust,
            ),
            (&self.header_context.tools, defaults.tools),
            // Removed MCP info from header as requested
        ];

        fields
            .into_iter()
            .filter_map(|(value, fallback)| {
                let mut selected = if value.trim().is_empty() {
                    fallback
                } else {
                    value.clone()
                };
                let trimmed = selected.trim();
                if trimmed.is_empty() {
                    return None;
                }

                if let Some(body) = trimmed.strip_prefix(ui::HEADER_TRUST_PREFIX) {
                    selected = format!("Trust {}", body.trim());
                    return Some(selected);
                }

                if let Some(body) = trimmed.strip_prefix(ui::HEADER_TOOLS_PREFIX) {
                    selected = format!("Tools: {}", body.trim());
                    return Some(selected);
                }

                Some(selected)
            })
            .collect()
    }

    fn header_meta_line(&self) -> Line<'static> {
        let mut spans = Vec::new();

        let mut first_section = true;
        let mode_label = self.header_mode_short_label();
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

    fn header_highlights_line(&self) -> Option<Line<'static>> {
        let mut spans = Vec::new();
        let mut first_section = true;

        for highlight in &self.header_context.highlights {
            let title = highlight.title.trim();
            let summary = self.header_highlight_summary(highlight);

            if title.is_empty() && summary.is_none() {
                continue;
            }

            if !first_section {
                spans.push(Span::styled(
                    ui::HEADER_META_SEPARATOR.to_string(),
                    self.header_secondary_style(),
                ));
            }

            if !title.is_empty() {
                let mut title_style = self.header_secondary_style();
                title_style = title_style.add_modifier(Modifier::BOLD);
                let mut title_text = title.to_string();
                if summary.is_some() {
                    title_text.push(':');
                }
                spans.push(Span::styled(title_text, title_style));
                if summary.is_some() {
                    spans.push(Span::styled(" ".to_string(), self.header_secondary_style()));
                }
            }

            if let Some(body) = summary {
                spans.push(Span::styled(body, self.header_primary_style()));
            }

            first_section = false;
        }

        if spans.is_empty() {
            None
        } else {
            Some(Line::from(spans))
        }
    }

    fn header_highlight_summary(&self, highlight: &InlineHeaderHighlight) -> Option<String> {
        let entries: Vec<String> = highlight
            .lines
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|line| {
                let stripped = line
                    .strip_prefix("- ")
                    .or_else(|| line.strip_prefix("• "))
                    .unwrap_or(line);
                stripped.trim().to_string()
            })
            .collect();

        if entries.is_empty() {
            return None;
        }

        Some(self.compact_highlight_entries(&entries))
    }

    fn compact_highlight_entries(&self, entries: &[String]) -> String {
        let mut summary =
            self.truncate_highlight_preview(entries.first().map(String::as_str).unwrap_or(""));
        if entries.len() > 1 {
            let remaining = entries.len() - 1;
            if !summary.is_empty() {
                let _ = write!(summary, " (+{} more)", remaining);
            } else {
                summary = format!("(+{} more)", remaining);
            }
        }
        summary
    }

    fn truncate_highlight_preview(&self, text: &str) -> String {
        let max = ui::HEADER_HIGHLIGHT_PREVIEW_MAX_CHARS;
        if max == 0 {
            return String::new();
        }

        let grapheme_count = text.graphemes(true).count();
        if grapheme_count <= max {
            return text.to_string();
        }

        let mut truncated = String::new();
        for grapheme in text.graphemes(true).take(max.saturating_sub(1)) {
            truncated.push_str(grapheme);
        }
        truncated.push_str(ui::INLINE_PREVIEW_ELLIPSIS);
        truncated
    }

    /// Determine if suggestions should be shown in the header
    fn should_show_suggestions(&self) -> bool {
        // Show suggestions when input is empty or starts with /
        self.input.is_empty() || self.input.starts_with('/')
    }

    /// Generate header line with slash command and keyboard shortcut suggestions
    fn header_suggestions_line(&self) -> Option<Line<'static>> {
        let mut spans = Vec::new();

        spans.push(Span::styled(
            "/help",
            self.header_primary_style().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            " · ",
            self.header_secondary_style().add_modifier(Modifier::DIM),
        ));
        spans.push(Span::styled(
            "/prompts",
            self.header_primary_style().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            " · ",
            self.header_secondary_style().add_modifier(Modifier::DIM),
        ));
        spans.push(Span::styled(
            "/model",
            self.header_primary_style().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            "  |  ",
            self.header_secondary_style().add_modifier(Modifier::DIM),
        ));
        spans.push(Span::styled(
            "↑↓",
            self.header_primary_style().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" Nav · ", self.header_secondary_style()));
        spans.push(Span::styled(
            "Tab",
            self.header_primary_style().add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" Complete", self.header_secondary_style()));

        Some(Line::from(spans))
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
            .borders(Borders::NONE)
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
        let effective_padding = padding.min(viewport_rows.saturating_sub(1));
        let total_rows = {
            let lines = self.cached_transcript_lines(content_width);
            lines.len() + effective_padding
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
            self.slash_palette.clear_visible_rows();
            return;
        }
        let suggestions = self.slash_palette.suggestions();
        if suggestions.is_empty() {
            self.slash_palette.clear_visible_rows();
            return;
        }

        let mut width_hint = measure_text_width(ui::SLASH_PALETTE_HINT_PRIMARY);
        width_hint = width_hint.max(measure_text_width(ui::SLASH_PALETTE_HINT_SECONDARY));
        for suggestion in suggestions.iter().take(ui::SLASH_SUGGESTION_LIMIT) {
            let label = match suggestion {
                slash_palette::SlashPaletteSuggestion::Static(cmd) => {
                    if !cmd.description.is_empty() {
                        format!("/{} {}", cmd.name, cmd.description)
                    } else {
                        format!("/{}", cmd.name)
                    }
                }
                slash_palette::SlashPaletteSuggestion::Custom(prompt) => {
                    // For custom prompts, format as /prompts:name
                    let prompt_cmd = format!("prompts:{}", prompt.name);
                    let description = prompt.description.as_deref().unwrap_or("");
                    if !description.is_empty() {
                        format!("/{} {}", prompt_cmd, description)
                    } else {
                        format!("/{}", prompt_cmd)
                    }
                }
            };
            width_hint = width_hint.max(measure_text_width(&label));
        }

        let instructions = self.slash_palette_instructions();
        let area = compute_modal_area(viewport, width_hint, instructions.len(), 0, 0, true);

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
            self.slash_palette.clear_visible_rows();
            return;
        }

        let layout = ModalListLayout::new(inner, instructions.len());
        if let Some(text_area) = layout.text_area {
            let paragraph = Paragraph::new(instructions).wrap(Wrap { trim: true });
            frame.render_widget(paragraph, text_area);
        }

        self.slash_palette
            .set_visible_rows(layout.list_area.height as usize);

        let list = List::new(self.slash_list_items())
            .style(self.default_style())
            .highlight_style(self.slash_highlight_style());

        frame.render_stateful_widget(list, layout.list_area, self.slash_palette.list_state_mut());
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

    fn render_input(&mut self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        if area.height == 0 {
            return;
        }

        let mut input_area = area;
        let mut status_area = None;
        let mut status_line = None;

        if area.height >= 2 {
            if let Some(line) = self.render_input_status_line(area.width) {
                let block_height = area.height.saturating_sub(1).max(1);
                input_area.height = block_height;
                status_area = Some(Rect::new(area.x, area.y + block_height, area.width, 1));
                status_line = Some(line);
            }
        }

        let block = Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .border_type(BorderType::Rounded)
            .style(self.default_style())
            .border_style(self.accent_style());
        let inner = block.inner(input_area);
        let input_render = self.build_input_render(inner.width, inner.height);
        let paragraph = Paragraph::new(input_render.text)
            .style(self.default_style())
            .wrap(Wrap { trim: false })
            .block(block);
        frame.render_widget(paragraph, input_area);

        if self.cursor_should_be_visible() && inner.width > 0 && inner.height > 0 {
            let cursor_x = input_render
                .cursor_x
                .min(inner.width.saturating_sub(1))
                .saturating_add(inner.x);
            let cursor_y = input_render
                .cursor_y
                .min(inner.height.saturating_sub(1))
                .saturating_add(inner.y);
            frame.set_cursor_position((cursor_x, cursor_y));
        }

        if let (Some(status_rect), Some(line)) = (status_area, status_line) {
            let paragraph = Paragraph::new(line)
                .style(self.default_style())
                .wrap(Wrap { trim: false });
            frame.render_widget(paragraph, status_rect);
        }
    }

    fn desired_input_lines(&self, inner_width: u16) -> u16 {
        if inner_width == 0 || self.input.is_empty() {
            return 1;
        }

        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let prompt_display_width = prompt_width.min(inner_width);
        let layout = self.input_layout(inner_width, prompt_display_width);
        let line_count = layout.buffers.len().max(1);
        let capped = line_count.min(ui::INLINE_INPUT_MAX_LINES.max(1));
        capped as u16
    }

    fn input_layout(&self, width: u16, prompt_display_width: u16) -> InputLayout {
        let indent_prefix = " ".repeat(prompt_display_width as usize);
        let mut buffers = vec![InputLineBuffer::new(
            self.prompt_prefix.clone(),
            prompt_display_width,
        )];
        let secure_prompt_active = self.secure_prompt_active();
        let mut cursor_line_idx = 0usize;
        let mut cursor_column = prompt_display_width;
        let mut cursor_set = self.cursor == 0;

        for (idx, ch) in self.input.char_indices() {
            if !cursor_set && self.cursor == idx {
                if let Some(current) = buffers.last() {
                    cursor_line_idx = buffers.len() - 1;
                    cursor_column = current.prefix_width + current.text_width;
                    cursor_set = true;
                }
            }

            if ch == '\n' {
                let end = idx + ch.len_utf8();
                buffers.push(InputLineBuffer::new(
                    indent_prefix.clone(),
                    prompt_display_width,
                ));
                if !cursor_set && self.cursor == end {
                    cursor_line_idx = buffers.len() - 1;
                    cursor_column = prompt_display_width;
                    cursor_set = true;
                }
                continue;
            }

            let display_ch = if secure_prompt_active { '•' } else { ch };
            let char_width = UnicodeWidthChar::width(display_ch).unwrap_or(0) as u16;

            if let Some(current) = buffers.last_mut() {
                let capacity = width.saturating_sub(current.prefix_width);
                if capacity > 0
                    && current.text_width + char_width > capacity
                    && !current.text.is_empty()
                {
                    buffers.push(InputLineBuffer::new(
                        indent_prefix.clone(),
                        prompt_display_width,
                    ));
                }
            }

            if let Some(current) = buffers.last_mut() {
                current.text.push(display_ch);
                current.text_width = current.text_width.saturating_add(char_width);
            }

            let end = idx + ch.len_utf8();
            if !cursor_set && self.cursor == end {
                if let Some(current) = buffers.last() {
                    cursor_line_idx = buffers.len() - 1;
                    cursor_column = current.prefix_width + current.text_width;
                    cursor_set = true;
                }
            }
        }

        if !cursor_set {
            if let Some(current) = buffers.last() {
                cursor_line_idx = buffers.len() - 1;
                cursor_column = current.prefix_width + current.text_width;
            }
        }

        InputLayout {
            buffers,
            cursor_line_idx,
            cursor_column,
        }
    }

    fn apply_input_height(&mut self, height: u16) {
        let resolved = height.max(Self::input_block_height_for_lines(1));
        if self.input_height != resolved {
            self.input_height = resolved;
            self.recalculate_transcript_rows();
        }
    }

    fn input_block_height_for_lines(lines: u16) -> u16 {
        lines.max(1).saturating_add(2)
    }

    fn build_input_render(&self, width: u16, height: u16) -> InputRender {
        if width == 0 || height == 0 {
            return InputRender {
                text: Text::default(),
                cursor_x: 0,
                cursor_y: 0,
            };
        }

        let max_visible_lines = height.max(1).min(ui::INLINE_INPUT_MAX_LINES as u16) as usize;

        let mut prompt_style = self.prompt_style.clone();
        if prompt_style.color.is_none() {
            prompt_style.color = self.theme.primary.or(self.theme.foreground);
        }
        let prompt_style = ratatui_style_from_inline(&prompt_style, self.theme.foreground);
        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let prompt_display_width = prompt_width.min(width);

        if self.input.is_empty() {
            let mut spans = Vec::new();
            spans.push(Span::styled(self.prompt_prefix.clone(), prompt_style));

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

            return InputRender {
                text: Text::from(vec![Line::from(spans)]),
                cursor_x: prompt_display_width,
                cursor_y: 0,
            };
        }

        let accent_style =
            ratatui_style_from_inline(&self.accent_inline_style(), self.theme.foreground);
        let layout = self.input_layout(width, prompt_display_width);
        let total_lines = layout.buffers.len();
        let visible_limit = max_visible_lines.max(1);
        let mut start = total_lines.saturating_sub(visible_limit);
        if layout.cursor_line_idx < start {
            start = layout.cursor_line_idx.saturating_sub(visible_limit - 1);
        }
        let end = (start + visible_limit).min(total_lines);
        let cursor_y = layout.cursor_line_idx.saturating_sub(start) as u16;

        let mut lines = Vec::new();
        for buffer in &layout.buffers[start..end] {
            let mut spans = Vec::new();
            spans.push(Span::styled(buffer.prefix.clone(), prompt_style));
            if !buffer.text.is_empty() {
                spans.push(Span::styled(buffer.text.clone(), accent_style));
            }
            lines.push(Line::from(spans));
        }

        if lines.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                self.prompt_prefix.clone(),
                prompt_style,
            )]));
        }

        InputRender {
            text: Text::from(lines),
            cursor_x: layout.cursor_column,
            cursor_y,
        }
    }

    fn render_input_status_line(&self, width: u16) -> Option<Line<'static>> {
        if width == 0 {
            return None;
        }

        let left = self
            .input_status_left
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let right = self
            .input_status_right
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        if left.is_none() && right.is_none() {
            return None;
        }

        let style = self.default_style().add_modifier(Modifier::DIM);
        let mut spans = Vec::new();

        match (left, right) {
            (Some(left_value), Some(right_value)) => {
                let left_width = measure_text_width(&left_value);
                let right_width = measure_text_width(&right_value);
                let padding = width.saturating_sub(left_width + right_width);

                spans.push(Span::styled(left_value, style));
                if padding > 0 {
                    spans.push(Span::raw(" ".repeat(padding as usize)));
                } else {
                    spans.push(Span::raw(" ".to_string()));
                }
                spans.push(Span::styled(right_value, style));
            }
            (Some(left_value), None) => {
                spans.push(Span::styled(left_value, style));
            }
            (None, Some(right_value)) => {
                let right_width = measure_text_width(&right_value);
                let padding = width.saturating_sub(right_width);
                if padding > 0 {
                    spans.push(Span::raw(" ".repeat(padding as usize)));
                }
                spans.push(Span::styled(right_value, style));
            }
            (None, None) => return None,
        }

        Some(Line::from(spans))
    }

    fn has_input_status(&self) -> bool {
        let left_present = self
            .input_status_left
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty());
        if left_present {
            return true;
        }
        self.input_status_right
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
    }

    fn slash_list_items(&self) -> Vec<ListItem<'static>> {
        let command_style = self.slash_name_style();
        let match_style = self.slash_match_style();
        let description_style = self.slash_description_style();

        self.slash_palette
            .items()
            .into_iter()
            .map(|item| {
                let mut spans: Vec<Span<'static>> = Vec::new();
                spans.push(Span::styled("/".to_string(), command_style));
                spans.extend(item.name_segments.into_iter().map(|segment| {
                    let style = if segment.highlighted {
                        match_style
                    } else {
                        command_style
                    };
                    Span::styled(segment.content, style)
                }));
                if !item.description.is_empty() {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        item.description.to_string(),
                        description_style,
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect()
    }

    fn slash_match_style(&self) -> Style {
        self.slash_name_style().add_modifier(Modifier::UNDERLINED)
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
        self.header_reserved_rows() + self.input_height
    }

    fn recalculate_transcript_rows(&mut self) {
        let reserved = self.input_reserved_rows().saturating_add(2); // account for transcript block borders
        let available = self.view_rows.saturating_sub(reserved).max(1);
        self.apply_transcript_rows(available);
    }

    fn handle_slash_palette_change(&mut self) {
        self.recalculate_transcript_rows();
        self.enforce_scroll_bounds();
        self.mark_dirty();
    }

    fn clear_slash_suggestions(&mut self) {
        if self.slash_palette.clear() {
            self.handle_slash_palette_change();
        }
    }

    fn update_slash_suggestions(&mut self) {
        if !self.input_enabled {
            self.clear_slash_suggestions();
            return;
        }

        let Some(prefix) = command_prefix(&self.input, self.cursor) else {
            self.clear_slash_suggestions();
            return;
        };

        // Update slash palette with custom prompts if available
        if let Some(ref custom_prompts) = self.custom_prompts {
            self.slash_palette
                .set_custom_prompts(custom_prompts.clone());
        }

        match self
            .slash_palette
            .update(Some(&prefix), ui::SLASH_SUGGESTION_LIMIT)
        {
            SlashPaletteUpdate::NoChange => {}
            SlashPaletteUpdate::Cleared | SlashPaletteUpdate::Changed { .. } => {
                self.handle_slash_palette_change();
            }
        }
    }

    fn slash_navigation_available(&self) -> bool {
        self.input_enabled && !self.slash_palette.is_empty()
    }

    fn move_slash_selection_up(&mut self) -> bool {
        let changed = self.slash_palette.move_up();
        self.handle_slash_selection_change(changed)
    }

    fn move_slash_selection_down(&mut self) -> bool {
        let changed = self.slash_palette.move_down();
        self.handle_slash_selection_change(changed)
    }

    fn select_first_slash_suggestion(&mut self) -> bool {
        let changed = self.slash_palette.select_first();
        self.handle_slash_selection_change(changed)
    }

    fn select_last_slash_suggestion(&mut self) -> bool {
        let changed = self.slash_palette.select_last();
        self.handle_slash_selection_change(changed)
    }

    fn page_up_slash_suggestion(&mut self) -> bool {
        let changed = self.slash_palette.page_up();
        self.handle_slash_selection_change(changed)
    }

    fn page_down_slash_suggestion(&mut self) -> bool {
        let changed = self.slash_palette.page_down();
        self.handle_slash_selection_change(changed)
    }

    fn handle_slash_selection_change(&mut self, changed: bool) -> bool {
        if changed {
            self.preview_selected_slash_suggestion();
            self.recalculate_transcript_rows();
            self.enforce_scroll_bounds();
            self.mark_dirty();
            true
        } else {
            false
        }
    }

    fn preview_selected_slash_suggestion(&mut self) {
        let Some(command) = self.slash_palette.selected_command() else {
            return;
        };
        let Some(range) = command_range(&self.input, self.cursor) else {
            return;
        };

        let current_input = self.input.clone();
        let prefix = &current_input[..range.start];
        let suffix = &current_input[range.end..];

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

    fn apply_selected_slash_suggestion(&mut self) -> bool {
        // Check if there's a selected custom prompt first
        if let Some(custom_prompt) = self.slash_palette.selected_custom_prompt() {
            let Some(range) = command_range(&self.input, self.cursor) else {
                return false;
            };

            // Replace the input with the selected custom prompt in /prompts:name format
            let mut new_input = String::from("/prompts:");
            new_input.push_str(&custom_prompt.name);

            let suffix = &self.input[range.end..];
            if !suffix.is_empty() {
                if !suffix.chars().next().map_or(false, char::is_whitespace) {
                    new_input.push(' ');
                }
                new_input.push_str(suffix);
            } else {
                new_input.push(' ');
            }

            let cursor_position = new_input.len();

            self.input = new_input;
            self.cursor = cursor_position;
            self.update_slash_suggestions();
            self.mark_dirty();
            return true;
        }

        // Fall back to regular command if no custom prompt is selected
        let Some(command) = self.slash_palette.selected_command() else {
            return false;
        };
        let Some(range) = command_range(&self.input, self.cursor) else {
            return false;
        };

        let suffix = self.input[range.end..].to_string();
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
        has_command: bool,
    ) -> bool {
        if !self.slash_navigation_available() || has_control || has_alt {
            return false;
        }

        let handled = match key.code {
            KeyCode::Up => {
                if has_command {
                    self.select_first_slash_suggestion()
                } else {
                    self.move_slash_selection_up()
                }
            }
            KeyCode::Down => {
                if has_command {
                    self.select_last_slash_suggestion()
                } else {
                    self.move_slash_selection_down()
                }
            }
            KeyCode::PageUp => self.page_up_slash_suggestion(),
            KeyCode::PageDown => self.page_down_slash_suggestion(),
            KeyCode::Home => self.select_first_slash_suggestion(),
            KeyCode::End => self.select_last_slash_suggestion(),
            KeyCode::Tab => self.apply_selected_slash_suggestion(),
            KeyCode::BackTab => self.move_slash_selection_up(),
            _ => return false,
        };

        if handled {
            true
        } else {
            matches!(
                key.code,
                KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::PageUp
                    | KeyCode::PageDown
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::Tab
                    | KeyCode::BackTab
            )
        }
    }

    fn render_message_spans(&self, index: usize) -> Vec<Span<'static>> {
        let Some(line) = self.lines.get(index) else {
            return vec![Span::raw(String::new())];
        };
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

        if line.kind == InlineMessageKind::Pty {
            let prev_is_pty = index
                .checked_sub(1)
                .and_then(|prev| self.lines.get(prev))
                .map(|prev| prev.kind == InlineMessageKind::Pty)
                .unwrap_or(false);
            if !prev_is_pty {
                let mut combined = String::new();
                for segment in &line.segments {
                    combined.push_str(segment.text.as_str());
                }
                let header_text = if combined.trim().is_empty() {
                    ui::INLINE_PTY_PLACEHOLDER.to_string()
                } else {
                    combined.trim().to_string()
                };
                let mut label_style = InlineTextStyle::default();
                label_style.color = self.theme.primary.or(self.theme.foreground);
                label_style.bold = true;
                spans.push(Span::styled(
                    format!("[{}]", ui::INLINE_PTY_HEADER_LABEL),
                    ratatui_style_from_inline(&label_style, self.theme.foreground),
                ));
                spans.push(Span::raw(" "));
                let mut body_style = InlineTextStyle::default();
                body_style.color = self.theme.foreground;
                body_style.bold = true;
                spans.push(Span::styled(
                    header_text,
                    ratatui_style_from_inline(&body_style, self.theme.foreground),
                ));
                return spans;
            }
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

        // Custom prefix instead of default detail prefix
        spans.push(Span::styled(
            "  ↳ ",
            self.accent_style().add_modifier(Modifier::DIM),
        ));

        let mut body_style = InlineTextStyle::default();
        body_style.color = self.theme.tool_body.or(self.theme.foreground);
        body_style.italic = false; // Remove italic for cleaner look
        body_style.bold = false;

        let trimmed = text.trim_start();
        if !trimmed.is_empty() {
            spans.push(Span::styled(
                trimmed.to_string(),
                ratatui_style_from_inline(&body_style, self.theme.foreground),
            ));
        }

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
            // Add bracket wrapper with different styling
            spans.push(Span::styled(
                "[",
                self.accent_style().add_modifier(Modifier::BOLD),
            ));

            // Get distinctive color based on the tool name for better differentiation
            let tool_name_style = self.tool_inline_style(name);

            spans.push(Span::styled(
                name.to_string(),
                ratatui_style_from_inline(&tool_name_style, self.theme.foreground),
            ));

            spans.push(Span::styled(
                "] ",
                self.accent_style().add_modifier(Modifier::BOLD),
            ));

            // Add arrow separator with different styling
            spans.push(Span::styled(
                "→ ",
                self.accent_style().add_modifier(Modifier::DIM),
            ));
        }

        let trimmed_tail = tail.trim_start();
        if !trimmed_tail.is_empty() {
            // Parse the tail to extract tool action and parameters for better formatting
            let parts: Vec<&str> = trimmed_tail.split(" · ").collect();
            if parts.len() > 1 {
                // Format as "action → description · parameter1 · parameter2"
                let action = parts[0];
                let mut body_style = InlineTextStyle::default();
                body_style.color = self.theme.tool_body.or(self.theme.foreground);
                body_style.bold = false;

                spans.push(Span::styled(
                    action.to_string(),
                    ratatui_style_from_inline(&body_style, self.theme.foreground),
                ));

                // Format additional parameters
                for part in parts[1..].iter() {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        "·",
                        self.accent_style().add_modifier(Modifier::DIM),
                    ));
                    spans.push(Span::raw(" "));

                    // Differentiate between parameter names and values
                    let param_parts: Vec<&str> = part.split(": ").collect();
                    if param_parts.len() > 1 {
                        // Parameter name (before colon) - in accent color
                        spans.push(Span::styled(
                            format!("{}: ", param_parts[0]),
                            self.accent_style(),
                        ));

                        // Parameter value (after colon) - in regular color
                        spans.push(Span::styled(
                            param_parts[1].to_string(),
                            ratatui_style_from_inline(&body_style, self.theme.foreground),
                        ));
                    } else {
                        spans.push(Span::styled(
                            part.to_string(),
                            ratatui_style_from_inline(&body_style, self.theme.foreground),
                        ));
                    }
                }
            } else {
                // Fallback for original formatting
                let mut body_style = InlineTextStyle::default();
                body_style.color = self.theme.tool_body.or(self.theme.foreground);
                body_style.italic = false;

                // Simplify common tool call patterns for human readability
                let simplified_text = self.simplify_tool_display(trimmed_tail);
                spans.push(Span::styled(
                    simplified_text,
                    ratatui_style_from_inline(&body_style, self.theme.foreground),
                ));
            }
        }

        spans
    }

    /// Simplify tool call display text for better human readability
    fn simplify_tool_display(&self, text: &str) -> String {
        // Common patterns to simplify for human readability
        let simplified = if text.starts_with("file ") {
            // Convert "file path/to/file" to "accessing path/to/file"
            text.replacen("file ", "accessing ", 1)
        } else if text.starts_with("path: ") {
            // Convert "path: path/to/file" to "file: path/to/file"
            text.replacen("path: ", "file: ", 1)
        } else if text.contains(" → file ") {
            // Convert complex patterns to simpler ones
            text.replace(" → file ", " → ")
        } else if text.starts_with("grep ") {
            // Simplify grep patterns for better readability
            text.replacen("grep ", "searching for ", 1)
        } else if text.starts_with("find ") {
            // Simplify find patterns
            text.replacen("find ", "finding ", 1)
        } else if text.starts_with("list ") {
            // Simplify list patterns
            text.replacen("list ", "listing ", 1)
        } else {
            // Return original text if no simplification needed
            text.to_string()
        };

        // Further simplify parameter displays
        self.format_tool_parameters(&simplified)
    }

    /// Format tool parameters for better readability
    fn format_tool_parameters(&self, text: &str) -> String {
        // Convert common parameter patterns to more readable formats
        let mut formatted = text.to_string();

        // Convert "pattern: xyz" to "matching 'xyz'"
        if formatted.contains("pattern: ") {
            formatted = formatted.replace("pattern: ", "matching '");
            // Close the quote if there's a parameter separator
            if formatted.contains(" · ") {
                formatted = formatted.replacen(" · ", "' · ", 1);
            } else if formatted.contains("  ") {
                formatted = formatted.replacen("  ", "' ", 1);
            } else {
                formatted.push('\'');
            }
        }

        // Convert "path: xyz" to "in 'xyz'"
        if formatted.contains("path: ") {
            formatted = formatted.replace("path: ", "in '");
            // Close the quote if there's a parameter separator
            if formatted.contains(" · ") {
                formatted = formatted.replacen(" · ", "' · ", 1);
            } else if formatted.contains("  ") {
                formatted = formatted.replacen("  ", "' ", 1);
            } else {
                formatted.push('\'');
            }
        }

        formatted
    }

    /// Normalize tool names to group similar tools together
    fn normalize_tool_name(&self, tool_name: &str) -> String {
        // Group similar tools under common names for consistent styling
        match tool_name.to_lowercase().as_str() {
            "grep" | "rg" | "ripgrep" | "grep_file" | "search" | "find" | "ag" => {
                "search".to_string()
            }
            "list" | "ls" | "dir" | "list_files" => "list".to_string(),
            "read" | "cat" | "file" | "read_file" => "read".to_string(),
            "write" | "edit" | "save" | "insert" | "edit_file" => "write".to_string(),
            "run" | "command" | "bash" | "sh" => "run".to_string(),
            _ => tool_name.to_string(),
        }
    }

    fn tool_inline_style(&self, tool_name: &str) -> InlineTextStyle {
        let normalized_name = self.normalize_tool_name(tool_name);
        let mut style = InlineTextStyle::default();

        // Set bold as default for all tools
        style.bold = true;

        // Assign distinctive colors based on normalized tool type
        style.color = match normalized_name.to_lowercase().as_str() {
            "read" => {
                // Blue for file reading operations
                Some(AnsiColor::Blue.into())
            }
            "list" => {
                // Green for listing operations
                Some(AnsiColor::Green.into())
            }
            "search" => {
                // Yellow for search operations
                Some(AnsiColor::Yellow.into())
            }
            "write" => {
                // Magenta for write/edit operations
                Some(AnsiColor::Magenta.into())
            }
            "run" => {
                // Red for execution operations
                Some(AnsiColor::Red.into())
            }
            "git" | "version_control" => {
                // Cyan for version control
                Some(AnsiColor::Cyan.into())
            }
            _ => {
                // Use the default tool accent color for other tools
                self.theme
                    .tool_accent
                    .or(self.theme.primary)
                    .or(self.theme.foreground)
            }
        };

        style
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

    fn cursor_should_be_visible(&self) -> bool {
        self.cursor_visible && self.input_enabled
    }

    fn secure_prompt_active(&self) -> bool {
        self.modal
            .as_ref()
            .and_then(|modal| modal.secure_prompt.as_ref())
            .is_some()
    }

    pub fn mark_dirty(&mut self) {
        self.needs_redraw = true;
    }

    fn show_modal(
        &mut self,
        title: String,
        lines: Vec<String>,
        secure_prompt: Option<SecurePromptConfig>,
    ) {
        let state = ModalState {
            title,
            lines,
            list: None,
            secure_prompt,
            popup_state: PopupState::default(),
            restore_input: self.input_enabled,
            restore_cursor: self.cursor_visible,
            search: None,
        };
        if state.secure_prompt.is_none() {
            self.input_enabled = false;
        }
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
        search: Option<InlineListSearchConfig>,
    ) {
        let mut list_state = ModalListState::new(items, selected);
        let search_state = search.map(ModalSearchState::from);
        if let Some(search) = &search_state {
            list_state.apply_search(&search.query);
        }
        let state = ModalState {
            title,
            lines,
            list: Some(list_state),
            secure_prompt: None,
            popup_state: PopupState::default(),
            restore_input: self.input_enabled,
            restore_cursor: self.cursor_visible,
            search: search_state,
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

        let width_hint = modal_content_width(
            &modal.lines,
            modal.list.as_ref(),
            modal.secure_prompt.as_ref(),
            modal.search.as_ref(),
        );
        let prompt_lines = modal.secure_prompt.is_some().then_some(2).unwrap_or(0);
        let search_lines = modal.search.as_ref().map(|_| 3).unwrap_or(0);
        let area = compute_modal_area(
            viewport,
            width_hint,
            modal.lines.len(),
            prompt_lines,
            search_lines,
            modal.list.is_some(),
        );

        frame.render_widget(Clear, area);

        let body = SizedWrapper {
            inner: Text::raw(""),
            width: area.width as usize,
            height: area.height as usize,
        };

        let popup = Popup::new(body)
            .title(Line::styled(modal.title.clone(), styles.title))
            .borders(Borders::ALL)
            .border_set(border::ROUNDED)
            .border_style(styles.border);

        frame.render_stateful_widget_ref(popup, viewport, &mut modal.popup_state);

        let Some(popup_area) = modal.popup_state.area() else {
            return;
        };

        if popup_area.width <= 2 || popup_area.height <= 2 {
            return;
        }

        let inner = Rect {
            x: popup_area.x.saturating_add(1),
            y: popup_area.y.saturating_add(1),
            width: popup_area.width.saturating_sub(2),
            height: popup_area.height.saturating_sub(2),
        };

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        render_modal_body(
            frame,
            inner,
            ModalBodyContext {
                instructions: &modal.lines,
                list: modal.list.as_mut(),
                styles: &styles,
                secure_prompt: modal.secure_prompt.as_ref(),
                search: modal.search.as_ref(),
                input: &self.input,
                cursor: self.cursor,
            },
        );
    }

    fn modal_render_styles(&self) -> ModalRenderStyles {
        ModalRenderStyles {
            border: self.border_style(),
            highlight: self.modal_list_highlight_style(),
            badge: self.section_title_style().add_modifier(Modifier::DIM),
            header: self.section_title_style(),
            selectable: self.default_style().add_modifier(Modifier::BOLD),
            detail: self.default_style().add_modifier(Modifier::DIM),
            search_match: self
                .accent_style()
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            title: Style::default().add_modifier(Modifier::BOLD),
            divider: self
                .default_style()
                .add_modifier(Modifier::DIM | Modifier::ITALIC),
            instruction_border: self.border_style(),
            instruction_title: self.section_title_style(),
            instruction_bullet: self.accent_style().add_modifier(Modifier::BOLD),
            instruction_body: self.default_style(),
            hint: self
                .default_style()
                .add_modifier(Modifier::DIM | Modifier::ITALIC),
        }
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.scroll_offset = 0;
        self.reset_history_navigation();
        self.update_slash_suggestions();
        self.mark_dirty();
    }

    pub fn set_custom_prompts(&mut self, custom_prompts: CustomPromptRegistry) {
        self.custom_prompts = Some(custom_prompts);
        // Update slash palette if we're currently viewing slash commands
        if self.input.starts_with('/') {
            self.update_slash_suggestions();
        }
    }

    fn process_key(&mut self, key: KeyEvent) -> Option<InlineEvent> {
        let modifiers = key.modifiers;
        let has_control = modifiers.contains(KeyModifiers::CONTROL);
        let has_shift = modifiers.contains(KeyModifiers::SHIFT);
        let raw_alt = modifiers.contains(KeyModifiers::ALT);
        let raw_meta = modifiers.contains(KeyModifiers::META);
        let has_super = modifiers.contains(KeyModifiers::SUPER);
        let has_alt = raw_alt || (!has_super && raw_meta);
        let has_command = has_super || (raw_meta && !has_alt);

        if let Some(modal) = self.modal.as_mut() {
            let result = modal.handle_list_key_event(
                &key,
                ModalKeyModifiers {
                    control: has_control,
                    alt: has_alt,
                    command: has_command,
                },
            );

            match result {
                ModalListKeyResult::Redraw => {
                    self.mark_dirty();
                    return None;
                }
                ModalListKeyResult::HandledNoRedraw => {
                    return None;
                }
                ModalListKeyResult::Submit(event) | ModalListKeyResult::Cancel(event) => {
                    self.close_modal();
                    return Some(event);
                }
                ModalListKeyResult::NotHandled => {}
            }
        }

        if self.try_handle_slash_navigation(&key, has_control, has_alt, has_command) {
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
                let history_requested = if self.input_enabled && (has_alt || has_command) {
                    true
                } else if self.input_enabled {
                    self.current_max_scroll_offset() == 0
                } else {
                    false
                };

                if history_requested && self.navigate_history_previous() {
                    return None;
                }

                self.scroll_line_up();
                self.mark_dirty();
                Some(InlineEvent::ScrollLineUp)
            }
            KeyCode::Down => {
                let history_requested = if self.input_enabled && (has_alt || has_command) {
                    true
                } else if self.input_enabled {
                    self.current_max_scroll_offset() == 0
                } else {
                    false
                };

                if history_requested && self.navigate_history_next() {
                    return None;
                }

                self.scroll_line_down();
                self.mark_dirty();
                Some(InlineEvent::ScrollLineDown)
            }
            KeyCode::Enter => {
                if !self.input_enabled {
                    return None;
                }

                if has_shift && !has_control && !has_command {
                    self.insert_char('\n');
                    self.mark_dirty();
                    return None;
                }

                let submitted = std::mem::take(&mut self.input);
                self.cursor = 0;
                self.scroll_offset = 0;
                // Input is handled with standard paragraph, not TextArea
                self.update_slash_suggestions();
                self.remember_submitted_input(&submitted);
                self.mark_dirty();

                if has_control || has_command {
                    Some(InlineEvent::QueueSubmit(submitted))
                } else {
                    Some(InlineEvent::Submit(submitted))
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
        if ch == '\n' && !self.can_insert_newline() {
            return;
        }
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.update_slash_suggestions();
    }

    fn insert_text(&mut self, text: &str) {
        let mut remaining_newlines = self.remaining_newline_capacity();
        let sanitized: String = text
            .chars()
            .filter_map(|ch| {
                if matches!(ch, '\r' | '\u{7f}') {
                    return None;
                }
                if ch == '\n' {
                    if remaining_newlines == 0 {
                        return None;
                    }
                    remaining_newlines = remaining_newlines.saturating_sub(1);
                }
                Some(ch)
            })
            .collect();
        if sanitized.is_empty() {
            return;
        }
        self.input.insert_str(self.cursor, &sanitized);
        self.cursor += sanitized.len();
        self.update_slash_suggestions();
    }

    fn remaining_newline_capacity(&self) -> usize {
        ui::INLINE_INPUT_MAX_LINES
            .saturating_sub(1)
            .saturating_sub(self.input.matches('\n').count())
    }

    fn can_insert_newline(&self) -> bool {
        self.remaining_newline_capacity() > 0
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

    fn remember_submitted_input(&mut self, submitted: &str) {
        self.reset_history_navigation();
        if submitted.trim().is_empty() {
            return;
        }

        if self
            .input_history
            .last()
            .map_or(false, |last| last == submitted)
        {
            return;
        }

        self.input_history.push(submitted.to_string());
    }

    fn navigate_history_previous(&mut self) -> bool {
        if self.input_history.is_empty() {
            return false;
        }

        if let Some(index) = self.input_history_index {
            if index == 0 {
                self.apply_history_entry(index);
            } else {
                let new_index = index.saturating_sub(1);
                self.apply_history_entry(new_index);
            }
            true
        } else {
            let new_index = self.input_history.len().saturating_sub(1);
            self.input_history_draft = Some(self.input.clone());
            self.apply_history_entry(new_index);
            true
        }
    }

    fn navigate_history_next(&mut self) -> bool {
        let Some(index) = self.input_history_index else {
            return false;
        };

        if index + 1 < self.input_history.len() {
            let new_index = index + 1;
            self.apply_history_entry(new_index);
        } else {
            let draft = self.input_history_draft.take().unwrap_or_default();
            if self.input != draft {
                self.input = draft;
                self.cursor = self.input.len();
                self.scroll_offset = 0;
                self.update_slash_suggestions();
            }
            self.input_history_index = None;
            self.mark_dirty();
        }
        true
    }

    fn apply_history_entry(&mut self, index: usize) {
        if let Some(entry) = self.input_history.get(index) {
            if self.input != *entry {
                self.input = entry.clone();
                self.cursor = self.input.len();
                self.scroll_offset = 0;
                self.update_slash_suggestions();
            } else {
                self.cursor = self.input.len();
            }
            self.mark_dirty();
            self.input_history_index = Some(index);
        }
    }

    fn reset_history_navigation(&mut self) {
        self.input_history_index = None;
        self.input_history_draft = None;
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

        if kind == InlineMessageKind::Tool && self.handle_tool_code_fence_marker(text) {
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

        if appended {
            self.invalidate_scroll_metrics();
            return;
        }

        let can_reuse_last = self
            .lines
            .last()
            .map(|line| line.kind == kind && line.segments.is_empty())
            .unwrap_or(false);
        if can_reuse_last {
            let revision = self.next_revision();
            if let Some(line) = self.lines.last_mut() {
                line.segments.push(InlineSegment {
                    text: text.to_string(),
                    style: style.clone(),
                });
                line.revision = revision;
            }
            self.invalidate_scroll_metrics();
            return;
        }

        let revision = self.next_revision();
        self.lines.push(MessageLine {
            kind,
            segments: vec![InlineSegment {
                text: text.to_string(),
                style: style.clone(),
            }],
            revision,
        });

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
        let effective_padding = padding.min(viewport_rows.saturating_sub(1));
        let total_rows =
            self.cached_transcript_lines(self.transcript_width).len() + effective_padding;
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
                updates.push(Some(self.reflow_message_lines(index, width)));
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
            let mut lines: Vec<Line<'static>> = Vec::new();
            for index in 0..self.lines.len() {
                lines.extend(self.reflow_message_lines(index, 0));
            }
            if lines.is_empty() {
                lines.push(Line::default());
            }
            return lines;
        }

        let mut wrapped_lines = Vec::new();
        for index in 0..self.lines.len() {
            wrapped_lines.extend(self.reflow_message_lines(index, width));
        }

        if wrapped_lines.is_empty() {
            wrapped_lines.push(Line::default());
        }

        wrapped_lines
    }

    fn reflow_message_lines(&self, index: usize, width: u16) -> Vec<Line<'static>> {
        let Some(message) = self.lines.get(index) else {
            return vec![Line::default()];
        };

        if message.kind == InlineMessageKind::Tool {
            return self.reflow_tool_lines(index, width);
        }

        if message.kind == InlineMessageKind::Pty {
            return self.reflow_pty_lines(index, width);
        }

        let spans = self.render_message_spans(index);
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
        wrapped.extend(lines);

        if message.kind == InlineMessageKind::User && max_width > 0 {
            wrapped.push(self.message_divider_line(max_width, message.kind));
        }

        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        wrapped
    }

    fn wrap_block_lines(
        &self,
        first_prefix: &str,
        _continuation_prefix: &str,
        content: Vec<Span<'static>>,
        max_width: usize,
        border_style: Style,
    ) -> Vec<Line<'static>> {
        if max_width < 2 {
            return vec![Line::from(vec![Span::styled(
                format!("{}││", first_prefix),
                border_style,
            )])];
        }

        let right_border = ui::INLINE_BLOCK_BODY_RIGHT;
        let prefix_width = first_prefix.chars().count();
        let border_width = right_border.chars().count();
        let consumed_width = prefix_width.saturating_add(border_width);
        let content_width = max_width.saturating_sub(consumed_width);

        if max_width == usize::MAX {
            let mut spans = vec![Span::styled(first_prefix.to_string(), border_style)];
            spans.extend(content);
            spans.push(Span::styled(right_border.to_string(), border_style));
            return vec![Line::from(spans)];
        }

        let mut wrapped = self.wrap_line(Line::from(content), content_width);
        if wrapped.is_empty() {
            wrapped.push(Line::default());
        }

        // Add borders to each wrapped line
        for line in wrapped.iter_mut() {
            let line_width = line.spans.iter().map(|s| s.width()).sum::<usize>();
            let padding = content_width.saturating_sub(line_width);

            let mut new_spans = vec![Span::styled(first_prefix.to_string(), border_style)];
            new_spans.extend(line.spans.drain(..));
            if padding > 0 {
                new_spans.push(Span::styled(" ".repeat(padding), Style::default()));
            }
            new_spans.push(Span::styled(right_border.to_string(), border_style));
            line.spans = new_spans;
        }

        wrapped
    }

    fn block_footer_line(&self, width: u16, border_style: Style) -> Line<'static> {
        if width == 0 || ui::INLINE_BLOCK_HORIZONTAL.is_empty() {
            return Line::from(vec![Span::styled(
                format!(
                    "{}{}",
                    ui::INLINE_BLOCK_BOTTOM_LEFT,
                    ui::INLINE_BLOCK_BOTTOM_RIGHT
                ),
                border_style,
            )]);
        }

        let max_width = width as usize;
        if max_width <= 1 {
            return Line::from(vec![Span::styled(
                format!(
                    "{}{}",
                    ui::INLINE_BLOCK_BOTTOM_LEFT,
                    ui::INLINE_BLOCK_BOTTOM_RIGHT
                ),
                border_style,
            )]);
        }

        let mut content = ui::INLINE_BLOCK_BOTTOM_LEFT.to_string();
        content.push_str(&ui::INLINE_BLOCK_HORIZONTAL.repeat(max_width.saturating_sub(2)));
        content.push_str(ui::INLINE_BLOCK_BOTTOM_RIGHT);
        Line::from(vec![Span::styled(content, border_style)])
    }

    fn reflow_tool_lines(&self, index: usize, width: u16) -> Vec<Line<'static>> {
        let Some(line) = self.lines.get(index) else {
            return vec![Line::default()];
        };

        let max_width = if width == 0 {
            usize::MAX
        } else {
            width as usize
        };

        let mut border_style =
            ratatui_style_from_inline(&self.tool_border_style(), self.theme.foreground);
        border_style = border_style.add_modifier(Modifier::DIM);

        let is_detail = line.segments.iter().any(|segment| segment.style.italic);
        let prev_is_tool = index
            .checked_sub(1)
            .and_then(|prev| self.lines.get(prev))
            .map(|prev| prev.kind == InlineMessageKind::Tool)
            .unwrap_or(false);
        let next_is_tool = self
            .lines
            .get(index + 1)
            .map(|next| next.kind == InlineMessageKind::Tool)
            .unwrap_or(false);

        let is_start = !prev_is_tool;
        let is_end = !next_is_tool;

        let mut lines = Vec::new();
        if is_detail {
            let body_prefix = format!("{} ", ui::INLINE_BLOCK_BODY_LEFT);
            let content = self.render_tool_segments(line);
            lines.extend(self.wrap_block_lines(
                &body_prefix,
                &body_prefix,
                content,
                max_width,
                border_style.clone(),
            ));
        } else {
            // Add top border line for tool blocks
            if is_start && max_width > 2 {
                let top_border_content = format!(
                    "{}{}{}",
                    ui::INLINE_BLOCK_TOP_LEFT,
                    ui::INLINE_BLOCK_HORIZONTAL.repeat(max_width.saturating_sub(2)),
                    ui::INLINE_BLOCK_TOP_RIGHT
                );
                lines.push(Line::from(vec![Span::styled(
                    top_border_content,
                    border_style.clone(),
                )]));
            }

            let first_prefix = format!("{} ", ui::INLINE_BLOCK_BODY_LEFT);
            let continuation_prefix = format!("{} ", ui::INLINE_BLOCK_BODY_LEFT);
            let content = self.render_tool_segments(line);
            lines.extend(self.wrap_block_lines(
                &first_prefix,
                &continuation_prefix,
                content,
                max_width,
                border_style.clone(),
            ));
        }

        if is_end {
            lines.push(self.block_footer_line(width, border_style));
        }

        if lines.is_empty() {
            lines.push(Line::default());
        }

        lines
    }

    fn handle_tool_code_fence_marker(&mut self, text: &str) -> bool {
        let trimmed = text.trim();
        let stripped = trimmed
            .strip_prefix("```")
            .or_else(|| trimmed.strip_prefix("~~~"));

        let Some(rest) = stripped else {
            return false;
        };

        if rest.contains("```") || rest.contains("~~~") {
            return false;
        }

        if self.in_tool_code_fence {
            self.in_tool_code_fence = false;
            self.remove_trailing_empty_tool_line();
        } else {
            self.in_tool_code_fence = true;
        }

        true
    }

    fn remove_trailing_empty_tool_line(&mut self) {
        let should_remove = self
            .lines
            .last()
            .map(|line| line.kind == InlineMessageKind::Tool && line.segments.is_empty())
            .unwrap_or(false);
        if should_remove {
            self.lines.pop();
            self.invalidate_scroll_metrics();
        }
    }

    fn pty_block_has_content(&self, index: usize) -> bool {
        if self.lines.is_empty() {
            return false;
        }

        let mut start = index;
        while start > 0 {
            let Some(previous) = self.lines.get(start - 1) else {
                break;
            };
            if previous.kind != InlineMessageKind::Pty {
                break;
            }
            start -= 1;
        }

        let mut end = index;
        while end + 1 < self.lines.len() {
            let Some(next) = self.lines.get(end + 1) else {
                break;
            };
            if next.kind != InlineMessageKind::Pty {
                break;
            }
            end += 1;
        }

        for line in &self.lines[start..=end] {
            if line
                .segments
                .iter()
                .any(|segment| !segment.text.trim().is_empty())
            {
                return true;
            }
        }

        false
    }

    fn reflow_pty_lines(&self, index: usize, width: u16) -> Vec<Line<'static>> {
        let Some(line) = self.lines.get(index) else {
            return vec![Line::default()];
        };

        let max_width = if width == 0 {
            usize::MAX
        } else {
            width as usize
        };

        if !self.pty_block_has_content(index) {
            return Vec::new();
        }

        let mut border_inline = InlineTextStyle::default();
        border_inline.color = self.theme.secondary.or(self.theme.foreground);
        let mut border_style = ratatui_style_from_inline(&border_inline, self.theme.foreground);
        border_style = border_style.add_modifier(Modifier::DIM);

        let mut header_inline = InlineTextStyle::default();
        header_inline.color = self.theme.primary.or(self.theme.foreground);
        header_inline.bold = true;
        let header_style = ratatui_style_from_inline(&header_inline, self.theme.foreground);

        let mut body_inline = InlineTextStyle::default();
        body_inline.color = self.theme.foreground;
        let mut body_style = ratatui_style_from_inline(&body_inline, self.theme.foreground);
        body_style = body_style.add_modifier(Modifier::BOLD);

        let prev_is_pty = index
            .checked_sub(1)
            .and_then(|prev| self.lines.get(prev))
            .map(|prev| prev.kind == InlineMessageKind::Pty)
            .unwrap_or(false);
        let next_is_pty = self
            .lines
            .get(index + 1)
            .map(|next| next.kind == InlineMessageKind::Pty)
            .unwrap_or(false);

        let is_start = !prev_is_pty;
        let is_end = !next_is_pty;

        let mut lines = Vec::new();

        let mut combined = String::new();
        for segment in &line.segments {
            combined.push_str(segment.text.as_str());
        }
        if is_start && is_end && combined.trim().is_empty() {
            return Vec::new();
        }
        let header_text = combined
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| ui::INLINE_PTY_PLACEHOLDER.to_string());

        if is_start {
            // Add top border line
            if max_width > 2 {
                let top_border_content = format!(
                    "{}{}{}",
                    ui::INLINE_BLOCK_TOP_LEFT,
                    ui::INLINE_BLOCK_HORIZONTAL.repeat(max_width.saturating_sub(2)),
                    ui::INLINE_BLOCK_TOP_RIGHT
                );
                lines.push(Line::from(vec![Span::styled(
                    top_border_content,
                    border_style.clone(),
                )]));
            }

            let mut header_spans = Vec::new();
            header_spans.push(Span::styled(
                format!("[{}]", ui::INLINE_PTY_HEADER_LABEL),
                header_style.clone(),
            ));
            header_spans.push(Span::raw(" "));
            let mut running_style = InlineTextStyle::default();
            running_style.color = self.theme.secondary.or(self.theme.foreground);
            running_style.italic = true;
            header_spans.push(Span::styled(
                ui::INLINE_PTY_RUNNING_LABEL.to_string(),
                ratatui_style_from_inline(&running_style, self.theme.foreground),
            ));
            if !header_text.is_empty() {
                header_spans.push(Span::raw(" "));
                header_spans.push(Span::styled(header_text.clone(), body_style.clone()));
            }
            let status_label = if is_end {
                ui::INLINE_PTY_STATUS_DONE
            } else {
                ui::INLINE_PTY_STATUS_LIVE
            };
            header_spans.push(Span::raw(" "));
            header_spans.push(Span::styled(
                format!("[{}]", status_label),
                self.accent_style()
                    .add_modifier(Modifier::REVERSED | Modifier::BOLD),
            ));

            let first_prefix = format!("{} ", ui::INLINE_BLOCK_BODY_LEFT);
            let continuation_prefix = format!("{} ", ui::INLINE_BLOCK_BODY_LEFT);
            lines.extend(self.wrap_block_lines(
                &first_prefix,
                &continuation_prefix,
                header_spans,
                max_width,
                border_style.clone(),
            ));
        } else {
            let fallback = self
                .text_fallback(InlineMessageKind::Pty)
                .or(self.theme.foreground);
            let mut body_spans = Vec::new();
            for segment in &line.segments {
                let style = ratatui_style_from_inline(&segment.style, fallback);
                body_spans.push(Span::styled(segment.text.clone(), style));
            }
            let body_prefix = format!("{} ", ui::INLINE_BLOCK_BODY_LEFT);
            lines.extend(self.wrap_block_lines(
                &body_prefix,
                &body_prefix,
                body_spans,
                max_width,
                border_style.clone(),
            ));
        }

        if is_end {
            lines.push(self.block_footer_line(width, border_style));
        }

        if lines.is_empty() {
            lines.push(Line::default());
        }

        lines
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

        fn push_span(spans: &mut Vec<Span<'static>>, style: &Style, text: &str) {
            if text.is_empty() {
                return;
            }

            if let Some(last) = spans.last_mut().filter(|last| last.style == *style) {
                last.content.to_mut().push_str(text);
                return;
            }

            spans.push(Span::styled(text.to_string(), *style));
        }

        let mut rows = Vec::new();
        let mut current_spans: Vec<Span<'static>> = Vec::new();
        let mut current_width = 0usize;
        let window = Window::new(0.0, max_width as f64, -1.0, 1.0);

        let flush_current = |spans: &mut Vec<Span<'static>>, rows: &mut Vec<Line<'static>>| {
            if spans.is_empty() {
                rows.push(Line::default());
            } else {
                rows.push(Line::from(mem::take(spans)));
            }
        };

        for span in line.spans.into_iter() {
            let style = span.style;
            let content = span.content.into_owned();
            if content.is_empty() {
                continue;
            }

            for piece in content.split_inclusive('\n') {
                let mut text = piece;
                let mut had_newline = false;
                if let Some(stripped) = text.strip_suffix('\n') {
                    text = stripped;
                    had_newline = true;
                    if let Some(without_carriage) = text.strip_suffix('\r') {
                        text = without_carriage;
                    }
                }

                if !text.is_empty() {
                    for grapheme in UnicodeSegmentation::graphemes(text, true) {
                        if grapheme.is_empty() {
                            continue;
                        }

                        let width = UnicodeWidthStr::width(grapheme);
                        if width == 0 {
                            push_span(&mut current_spans, &style, grapheme);
                            continue;
                        }

                        let mut attempts = 0usize;
                        loop {
                            let line_segment = LineSegment::new(
                                Point::new(current_width as f64, 0.0),
                                Point::new((current_width + width) as f64, 0.0),
                            );

                            match clip_line(line_segment, window) {
                                Some(clipped) => {
                                    let visible = (clipped.p2.x - clipped.p1.x).round() as usize;
                                    if visible == width {
                                        push_span(&mut current_spans, &style, grapheme);
                                        current_width += width;
                                        break;
                                    }

                                    if current_width == 0 {
                                        push_span(&mut current_spans, &style, grapheme);
                                        current_width += width;
                                        break;
                                    }

                                    flush_current(&mut current_spans, &mut rows);
                                    current_width = 0;
                                }
                                None => {
                                    if current_width == 0 {
                                        push_span(&mut current_spans, &style, grapheme);
                                        current_width += width;
                                        break;
                                    }

                                    flush_current(&mut current_spans, &mut rows);
                                    current_width = 0;
                                }
                            }

                            attempts += 1;
                            if attempts > 4 {
                                push_span(&mut current_spans, &style, grapheme);
                                current_width += width;
                                break;
                            }
                        }

                        if current_width >= max_width {
                            flush_current(&mut current_spans, &mut rows);
                            current_width = 0;
                        }
                    }
                }

                if had_newline {
                    flush_current(&mut current_spans, &mut rows);
                    current_width = 0;
                }
            }
        }

        if !current_spans.is_empty() {
            flush_current(&mut current_spans, &mut rows);
        } else if rows.is_empty() {
            rows.push(Line::default());
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
        text::{Line, Span},
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
    fn arrow_keys_navigate_input_history() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        session.input = "first message".to_string();
        session.cursor = session.input.len();
        let submit_first = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            matches!(submit_first, Some(InlineEvent::Submit(value)) if value == "first message")
        );

        session.input = "second".to_string();
        session.cursor = session.input.len();
        let submit_second = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(submit_second, Some(InlineEvent::Submit(value)) if value == "second"));

        assert_eq!(session.input_history.len(), 2);
        assert!(session.input.is_empty());

        let up_latest = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
        assert!(up_latest.is_none());
        assert_eq!(session.input, "second");

        let up_previous = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
        assert!(up_previous.is_none());
        assert_eq!(session.input, "first message");

        let down_forward = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
        assert!(down_forward.is_none());
        assert_eq!(session.input, "second");

        let down_restore = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
        assert!(down_restore.is_none());
        assert!(session.input.is_empty());
        assert!(session.input_history_index.is_none());
    }

    #[test]
    fn shift_enter_queues_submission() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        session.input = "queued".to_string();
        session.cursor = session.input.len();

        let queued = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        assert!(matches!(queued, Some(InlineEvent::QueueSubmit(value)) if value == "queued"));
    }

    #[test]
    fn consecutive_duplicate_submissions_not_stored_twice() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        session.input = "repeat".to_string();
        session.cursor = session.input.len();
        let first = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(first, Some(InlineEvent::Submit(value)) if value == "repeat"));

        session.input = "repeat".to_string();
        session.cursor = session.input.len();
        let second = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(second, Some(InlineEvent::Submit(value)) if value == "repeat"));

        assert_eq!(session.input_history.len(), 1);
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
            (LINE_COUNT as u16)
                + ui::INLINE_HEADER_HEIGHT
                + Session::input_block_height_for_lines(1)
                + 2,
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
        // Removed assertion for HEADER_MCP_PREFIX since we're no longer showing MCP info in header
        assert!(!meta_text.contains("Languages"));
        assert!(!meta_text.contains(ui::HEADER_STATUS_LABEL));
        assert!(!meta_text.contains(ui::HEADER_MESSAGES_LABEL));
        assert!(!meta_text.contains(ui::HEADER_INPUT_LABEL));
    }

    #[test]
    fn header_highlights_collapse_to_single_line() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        session.header_context.highlights = vec![
            InlineHeaderHighlight {
                title: "Keyboard Shortcuts".to_string(),
                lines: vec![
                    "/help Show help".to_string(),
                    "Enter Submit message".to_string(),
                ],
            },
            InlineHeaderHighlight {
                title: "Usage Tips".to_string(),
                lines: vec!["- Keep tasks focused".to_string()],
            },
        ];

        let lines = session.header_lines();
        assert_eq!(lines.len(), 3);

        let summary: String = lines[2]
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();

        assert!(summary.contains("Keyboard Shortcuts"));
        assert!(summary.contains("/help Show help"));
        assert!(summary.contains("(+1 more)"));
        assert!(!summary.contains("Enter Submit message"));
        assert!(summary.contains("Usage Tips"));
        assert!(summary.contains("Keep tasks focused"));
    }

    #[test]
    fn header_highlight_summary_truncates_long_entries() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        let limit = ui::HEADER_HIGHLIGHT_PREVIEW_MAX_CHARS;
        let long_entry = "A".repeat(limit + 5);
        session.header_context.highlights = vec![InlineHeaderHighlight {
            title: "Details".to_string(),
            lines: vec![long_entry.clone()],
        }];

        let lines = session.header_lines();
        assert_eq!(lines.len(), 3);

        let summary: String = lines[2]
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();

        let expected_preview = format!(
            "{}{}",
            "A".repeat(limit.saturating_sub(1)),
            ui::INLINE_PREVIEW_ELLIPSIS
        );

        assert!(summary.contains("Details"));
        assert!(summary.contains(&expected_preview));
        assert!(!summary.contains(&long_entry));
    }

    #[test]
    fn header_highlight_summary_hides_truncated_command_segments() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        session.header_context.highlights = vec![InlineHeaderHighlight {
            title: String::new(),
            lines: vec![
                "  - /{command}".to_string(),
                "  - /help Show slash command help".to_string(),
                "  - Enter Submit message".to_string(),
                "  - Escape Cancel input".to_string(),
            ],
        }];

        let lines = session.header_lines();
        assert_eq!(lines.len(), 3);

        let summary: String = lines[2]
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();

        assert!(summary.contains("/{command}"));
        assert!(summary.contains("(+3 more)"));
        assert!(!summary.contains("Escape"));
        assert!(!summary.contains(ui::INLINE_PREVIEW_ELLIPSIS));
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
        session.header_context.tools = format!(
            "{}allow 11 · prompt 7 · deny 0 · extras extras extras",
            ui::HEADER_TOOLS_PREFIX
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
    fn wrap_line_splits_double_width_graphemes() {
        let session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        let style = session.default_style();
        let line = Line::from(vec![Span::styled("你好世界".to_string(), style)]);

        let wrapped = session.wrap_line(line, 4);
        let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

        assert_eq!(rendered, vec!["你好".to_string(), "世界".to_string()]);
    }

    #[test]
    fn wrap_line_keeps_explicit_blank_rows() {
        let session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        let style = session.default_style();
        let line = Line::from(vec![Span::styled("top\n\nbottom".to_string(), style)]);

        let wrapped = session.wrap_line(line, 40);
        let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

        assert_eq!(
            rendered,
            vec!["top".to_string(), String::new(), "bottom".to_string()]
        );
    }

    #[test]
    fn wrap_line_preserves_characters_wider_than_viewport() {
        let session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        let style = session.default_style();
        let line = Line::from(vec![Span::styled("你".to_string(), style)]);

        let wrapped = session.wrap_line(line, 1);
        let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

        assert_eq!(rendered, vec!["你".to_string()]);
    }

    #[test]
    fn wrap_line_discards_carriage_return_before_newline() {
        let session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        let style = session.default_style();
        let line = Line::from(vec![Span::styled("foo\r\nbar".to_string(), style)]);

        let wrapped = session.wrap_line(line, 80);
        let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

        assert_eq!(rendered, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn tool_code_fence_markers_are_skipped() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        session.append_inline(
            InlineMessageKind::Tool,
            InlineSegment {
                text: "```rust\nfn demo() {}\n```".to_string(),
                style: InlineTextStyle::default(),
            },
        );

        let tool_lines: Vec<&MessageLine> = session
            .lines
            .iter()
            .filter(|line| line.kind == InlineMessageKind::Tool)
            .collect();

        assert_eq!(tool_lines.len(), 1);
        assert_eq!(tool_lines[0].segments.len(), 1);
        assert_eq!(tool_lines[0].segments[0].text.as_str(), "fn demo() {}");
        assert!(!session.in_tool_code_fence);
    }

    #[test]
    fn pty_block_omits_placeholder_when_empty() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        session.push_line(InlineMessageKind::Pty, Vec::new());

        let lines = session.reflow_pty_lines(0, 80);
        assert!(lines.is_empty());
    }

    #[test]
    fn pty_block_hides_until_output_available() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        session.push_line(InlineMessageKind::Pty, Vec::new());

        assert!(session.reflow_pty_lines(0, 80).is_empty());

        session.push_line(
            InlineMessageKind::Pty,
            vec![InlineSegment {
                text: "first output".to_string(),
                style: InlineTextStyle::default(),
            }],
        );

        let rendered = session.reflow_pty_lines(0, 80);
        assert!(rendered.iter().any(|line| !line.spans.is_empty()));
    }

    #[test]
    fn pty_block_skips_status_only_sequence() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        session.push_line(InlineMessageKind::Pty, Vec::new());
        session.push_line(InlineMessageKind::Pty, Vec::new());

        assert!(session.reflow_pty_lines(0, 80).is_empty());
        assert!(session.reflow_pty_lines(1, 80).is_empty());
    }

    #[test]
    fn transcript_shows_content_when_viewport_smaller_than_padding() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        for index in 0..10 {
            let label = format!("{LABEL_PREFIX}-{index}");
            session.push_line(InlineMessageKind::Agent, vec![make_segment(label.as_str())]);
        }

        let minimal_view_rows =
            ui::INLINE_HEADER_HEIGHT + Session::input_block_height_for_lines(1) + 1;
        session.force_view_rows(minimal_view_rows);

        let view = visible_transcript(&mut session);
        assert!(
            view.iter()
                .any(|line| line.contains(&format!("{LABEL_PREFIX}-9"))),
            "expected most recent transcript line to remain visible even when viewport is small"
        );
    }

    #[test]
    fn pty_scroll_preserves_order() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        for index in 0..200 {
            let label = format!("{LABEL_PREFIX}-{index}");
            session.push_line(
                InlineMessageKind::Pty,
                vec![InlineSegment {
                    text: label,
                    style: InlineTextStyle::default(),
                }],
            );
        }

        let bottom_view = visible_transcript(&mut session);
        assert!(
            bottom_view
                .iter()
                .any(|line| line.contains(&format!("{LABEL_PREFIX}-199"))),
            "bottom view should include latest PTY line"
        );

        for _ in 0..200 {
            session.scroll_page_up();
            if session.scroll_offset == session.current_max_scroll_offset() {
                break;
            }
        }

        let top_view = visible_transcript(&mut session);
        assert!(
            top_view
                .iter()
                .any(|line| line.contains(&format!("{LABEL_PREFIX}-0"))),
            "top view should include earliest PTY line"
        );
        assert!(
            top_view
                .iter()
                .all(|line| !line.contains(&format!("{LABEL_PREFIX}-199"))),
            "top view should not include latest PTY line"
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

        let index = session
            .lines
            .len()
            .checked_sub(1)
            .expect("agent message should be available");
        let spans = session.render_message_spans(index);

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

        let index = session
            .lines
            .len()
            .checked_sub(1)
            .expect("tool header line should exist");
        let spans = session.render_message_spans(index);

        assert!(spans.len() >= 4);
        assert_eq!(spans[0].content.clone().into_owned(), "  ");
        let label = format!("[{}]", ui::INLINE_TOOL_HEADER_LABEL);
        assert_eq!(spans[1].content.clone().into_owned(), label);
        assert_eq!(spans[1].style.fg, Some(Color::Rgb(0xBF, 0x45, 0x45)));
        assert_eq!(spans[2].content.clone().into_owned(), "[shell]");
        assert_eq!(spans[2].style.fg, Some(Color::Rgb(0xBF, 0x45, 0x45)));
        let italic_span = spans
            .iter()
            .find(|span| span.style.add_modifier.contains(Modifier::ITALIC))
            .expect("tool header should include italic tail");
        assert_eq!(italic_span.content.clone().into_owned().trim(), "executing");
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

        let index = session
            .lines
            .len()
            .checked_sub(1)
            .expect("tool detail line should exist");
        let spans = session.render_message_spans(index);

        assert_eq!(spans.len(), 1);
        let body_span = &spans[0];
        assert!(body_span.style.add_modifier.contains(Modifier::ITALIC));
        assert_eq!(body_span.content.clone().into_owned(), "result line");
    }
}
