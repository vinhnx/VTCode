use std::{cmp::min, fmt::Write, mem, time::Instant};

use ansi_to_tui::IntoText;
use anstyle::{AnsiColor, Color as AnsiColorEnum, RgbColor};
use crossterm::event::{
    Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent,
    MouseEventKind,
};
use line_clipping::cohen_sutherland::clip_line;
use line_clipping::{LineSegment, Point, Window};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use tokio::sync::mpsc::UnboundedSender;
use tui_popup::PopupState;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::types::{
    InlineCommand, InlineEvent, InlineHeaderContext, InlineHeaderHighlight, InlineListItem,
    InlineListSearchConfig, InlineListSelection, InlineMessageKind, InlineSegment, InlineTextStyle,
    InlineTheme, SecurePromptConfig,
};
use crate::config::constants::{prompts, ui};

mod file_palette;
mod input;
mod message;
mod modal;
mod prompt_palette;
mod queue;
mod slash_palette;
mod transcript;

use self::file_palette::{FilePalette, extract_file_reference};
use self::message::{MessageLabels, MessageLine};
use self::modal::{
    ModalBodyContext, ModalKeyModifiers, ModalListKeyResult, ModalListLayout, ModalListState,
    ModalRenderStyles, ModalSearchState, ModalState, compute_modal_area, modal_content_width,
    render_modal_body,
};
use self::prompt_palette::{PromptPalette, extract_prompt_reference};
use self::queue::QueueOverlay;
use self::slash_palette::{SlashPalette, SlashPaletteUpdate, command_prefix, command_range};
use self::transcript::{CachedMessage, TranscriptReflowCache};
use crate::prompts::CustomPromptRegistry;
#[cfg(test)]
use crate::tools::PlanSummary;
use crate::tools::{PlanCompletionState, PlanStep, StepStatus, TaskPlan};

const USER_PREFIX: &str = "";
const PLACEHOLDER_COLOR: RgbColor = RgbColor(0x88, 0x88, 0x88);
const PROMPT_COMMAND_NAME: &str = "prompt";
const LEGACY_PROMPT_COMMAND_NAME: &str = "prompts";
const PROMPT_INVOKE_PREFIX: &str = "prompt:";
const LEGACY_PROMPT_INVOKE_PREFIX: &str = "prompts:";
const PROMPT_COMMAND_PREFIX: &str = "/prompt:";

fn measure_text_width(text: &str) -> u16 {
    UnicodeWidthStr::width(text) as u16
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
    needs_full_clear: bool,
    should_exit: bool,
    view_rows: u16,
    input_height: u16,
    scroll_offset: usize,
    transcript_rows: u16,
    transcript_width: u16,
    transcript_view_top: usize,
    cached_max_scroll_offset: usize,
    scroll_metrics_dirty: bool,
    transcript_cache: Option<TranscriptReflowCache>,
    queued_inputs: Vec<String>,
    queue_overlay_cache: Option<QueueOverlay>,
    queue_overlay_version: u64,
    modal: Option<ModalState>,
    show_timeline_pane: bool,
    plan: TaskPlan,
    line_revision_counter: u64,
    in_tool_code_fence: bool,
    input_history: Vec<String>,
    input_history_index: Option<usize>,
    input_history_draft: Option<String>,
    last_escape_time: Option<Instant>,
    custom_prompts: Option<CustomPromptRegistry>,
    file_palette: Option<FilePalette>,
    file_palette_active: bool,
    deferred_file_browser_trigger: bool,
    prompt_palette: Option<PromptPalette>,
    prompt_palette_active: bool,
    deferred_prompt_browser_trigger: bool,
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
            needs_full_clear: false,
            should_exit: false,
            view_rows: resolved_rows,
            input_height: Self::input_block_height_for_lines(1),
            scroll_offset: 0,
            transcript_rows: initial_transcript_rows,
            transcript_width: 0,
            transcript_view_top: 0,
            cached_max_scroll_offset: 0,
            scroll_metrics_dirty: true,
            transcript_cache: None,
            queued_inputs: Vec::new(),
            queue_overlay_cache: None,
            queue_overlay_version: 0,
            modal: None,
            show_timeline_pane,
            plan: TaskPlan::default(),
            header_rows: initial_header_rows,
            line_revision_counter: 0,
            in_tool_code_fence: false,
            input_history: Vec::new(),
            input_history_index: None,
            input_history_draft: None,
            custom_prompts: None,
            file_palette: None,
            file_palette_active: false,
            deferred_file_browser_trigger: false,
            prompt_palette: None,
            prompt_palette_active: false,
            deferred_prompt_browser_trigger: false,
            last_escape_time: None,
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
            InlineCommand::SetQueuedInputs { entries } => {
                self.set_queued_inputs_entries(entries);
                self.mark_dirty();
            }
            InlineCommand::SetPlan { plan } => {
                self.set_plan(plan);
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
            InlineCommand::LoadFilePalette { files, workspace } => {
                self.load_file_palette(files, workspace);
            }
            InlineCommand::ClearScreen => {
                self.clear_screen();
            }
            InlineCommand::Shutdown => {
                self.request_exit();
            }
        }
        self.mark_dirty();
    }

    pub fn handle_event(
        &mut self,
        event: CrosstermEvent,
        events: &UnboundedSender<InlineEvent>,
        callback: Option<&(dyn Fn(&InlineEvent) + Send + Sync + 'static)>,
    ) {
        match event {
            CrosstermEvent::Key(key) => {
                // Only process Press events to avoid duplicate character insertion
                // Repeat events can cause characters to be inserted multiple times
                if matches!(key.kind, KeyEventKind::Press) {
                    if let Some(outbound) = self.process_key(key) {
                        if let Some(cb) = callback {
                            cb(&outbound);
                        }
                        let _ = events.send(outbound);
                    }
                }
            }
            CrosstermEvent::Mouse(MouseEvent { kind, .. }) => match kind {
                MouseEventKind::ScrollDown => {
                    self.scroll_line_down();
                    self.mark_dirty();
                    let event = InlineEvent::ScrollLineDown;
                    if let Some(cb) = callback {
                        cb(&event);
                    }
                    let _ = events.send(event);
                }
                MouseEventKind::ScrollUp => {
                    self.scroll_line_up();
                    self.mark_dirty();
                    let event = InlineEvent::ScrollLineUp;
                    if let Some(cb) = callback {
                        cb(&event);
                    }
                    let _ = events.send(event);
                }
                _ => {}
            },
            CrosstermEvent::Paste(content) => {
                if self.input_enabled {
                    self.insert_text(&content);
                    self.check_file_reference_trigger();
                    self.check_prompt_reference_trigger();
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

        // Clear entire frame if modal was just closed to remove artifacts
        if self.needs_full_clear {
            frame.render_widget(Clear, viewport);
            self.needs_full_clear = false;
        }

        // Handle deferred file browser trigger (after slash modal dismisses)
        if self.deferred_file_browser_trigger {
            self.deferred_file_browser_trigger = false;
            // Insert @ to trigger file browser now that slash modal is gone
            self.input.insert(self.cursor, '@');
            self.cursor += 1;
            self.check_file_reference_trigger();
            self.mark_dirty(); // Ensure UI updates
        }

        // Handle deferred prompt browser trigger (after slash modal dismisses)
        if self.deferred_prompt_browser_trigger {
            self.deferred_prompt_browser_trigger = false;
            // Insert # to trigger prompt browser now that slash modal is gone
            self.input.insert(self.cursor, '#');
            self.cursor += 1;
            self.check_prompt_reference_trigger();
            self.mark_dirty(); // Ensure UI updates
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
        self.render_file_palette(frame, viewport);
        self.render_prompt_palette(frame, viewport);
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
            .borders(Borders::LEFT)
            .border_type(BorderType::Plain)
            .style(self.default_style())
            .border_style(self.border_style());
        let inner = block.inner(area);
        if inner.height == 0 {
            frame.render_widget(block, area);
            return;
        }

        let items = self.navigation_items();
        let item_count = items.len();
        let viewport = inner.height as usize;

        if self.should_show_plan() {
            if item_count == 0 {
                self.navigation_state.select(None);
                *self.navigation_state.offset_mut() = 0;
            } else if let Some(selected) = self.plan_selected_index() {
                self.navigation_state.select(Some(selected));
                let max_offset = item_count.saturating_sub(viewport);
                let desired_offset = selected.saturating_sub(viewport.saturating_sub(1));
                *self.navigation_state.offset_mut() = desired_offset.min(max_offset);
            } else {
                self.navigation_state.select(None);
                *self.navigation_state.offset_mut() = 0;
            }
        } else if self.lines.is_empty() {
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
                defaults.workspace_trust.clone(),
            ),
            (&self.header_context.tools, defaults.tools.clone()),
            (&self.header_context.git, defaults.git.clone()),
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

                if let Some(body) = trimmed.strip_prefix(ui::HEADER_GIT_PREFIX) {
                    let body = body.trim();
                    if body.is_empty() {
                        return None;
                    }
                    selected = body.to_string();
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
            format!("/{}", PROMPT_COMMAND_NAME),
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
        if self.should_show_plan() {
            return self.plan_block_title();
        }

        let mut spans = Vec::new();
        spans.push(Span::styled(
            ui::NAVIGATION_BLOCK_TITLE.to_string(),
            self.section_title_style(),
        ));
        spans.push(Span::styled(
            format!(" · {}", ui::NAVIGATION_BLOCK_SHORTCUT_NOTE),
            self.navigation_preview_style(),
        ));

        Line::from(spans)
    }

    fn plan_block_title(&self) -> Line<'static> {
        let mut spans = Vec::new();
        spans.push(Span::styled(
            ui::PLAN_BLOCK_TITLE.to_string(),
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

    fn navigation_items(&self) -> Vec<ListItem<'static>> {
        if self.should_show_plan() {
            return self.plan_navigation_items();
        }
        self.timeline_navigation_items()
    }

    fn timeline_navigation_items(&self) -> Vec<ListItem<'static>> {
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
            step.status.checkbox().to_string(),
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

    fn plan_checkbox_style(&self, status: StepStatus) -> Style {
        match status {
            StepStatus::Completed => self.navigation_preview_style(),
            StepStatus::InProgress => self.accent_style().add_modifier(Modifier::BOLD),
            StepStatus::Pending => self.default_style(),
        }
    }

    fn plan_step_style(&self, status: StepStatus) -> Style {
        match status {
            StepStatus::Completed => self.navigation_preview_style(),
            StepStatus::InProgress => self.accent_style().add_modifier(Modifier::BOLD),
            StepStatus::Pending => self.default_style(),
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

    fn should_show_plan(&self) -> bool {
        self.plan.summary.status != PlanCompletionState::Empty && !self.plan.steps.is_empty()
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

        let content_width = inner.width;
        if content_width == 0 {
            return;
        }
        self.apply_transcript_width(content_width);

        let viewport_rows = inner.height as usize;
        let padding = usize::from(ui::INLINE_TRANSCRIPT_BOTTOM_PADDING);
        let effective_padding = padding.min(viewport_rows.saturating_sub(1));
        let total_rows = self.total_transcript_rows(content_width) + effective_padding;
        let (top_offset, _clamped_total_rows) =
            self.prepare_transcript_scroll(total_rows, viewport_rows);
        let vertical_offset = top_offset.min(self.cached_max_scroll_offset);
        self.transcript_view_top = vertical_offset;

        let visible_start = vertical_offset;
        let scroll_area = Rect::new(inner.x, inner.y, content_width, inner.height);
        let mut visible_lines =
            self.collect_transcript_window(content_width, visible_start, viewport_rows);
        let fill_count = viewport_rows.saturating_sub(visible_lines.len());
        if fill_count > 0 {
            let target_len = visible_lines.len() + fill_count;
            visible_lines.resize_with(target_len, Line::default);
        }
        self.overlay_queue_lines(&mut visible_lines, content_width);
        let paragraph = Paragraph::new(visible_lines)
            .style(self.default_style())
            .wrap(Wrap { trim: true });
        frame.render_widget(Clear, scroll_area);
        frame.render_widget(paragraph, scroll_area);
    }

    fn set_plan(&mut self, plan: TaskPlan) {
        self.plan = plan;
        self.mark_dirty();
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
                    // For custom prompts, format as /prompt:name (legacy alias /prompts:name)
                    let prompt_cmd = format!("{}:{}", PROMPT_COMMAND_NAME, prompt.name);
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

        // Get all list items (scrollable via ListState)
        let list_items = self.slash_list_items();

        let list = List::new(list_items)
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

    fn render_file_palette(&mut self, frame: &mut Frame<'_>, viewport: Rect) {
        if !self.file_palette_active {
            return;
        }

        let Some(palette) = self.file_palette.as_ref() else {
            return;
        };

        if viewport.height == 0 || viewport.width == 0 || self.modal.is_some() {
            return;
        }

        // Show loading state if no files loaded yet
        if !palette.has_files() {
            self.render_file_palette_loading(frame, viewport);
            return;
        }

        let items = palette.current_page_items();
        if items.is_empty() && palette.filter_query().is_empty() {
            return;
        }

        let mut width_hint = 40u16;
        for (_, entry, _) in &items {
            width_hint = width_hint.max(measure_text_width(&entry.display_name) + 4);
        }

        let instructions = self.file_palette_instructions(palette);
        let has_continuation = palette.has_more_items();
        let modal_height = items.len()
            + instructions.len()
            + 2  // borders
            + if has_continuation { 1 } else { 0 }; // continuation indicator
        let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

        frame.render_widget(Clear, area);
        let title = format!(
            "File Browser (Page {}/{})",
            palette.current_page_number(),
            palette.total_pages()
        );
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(self.default_style())
            .border_style(self.border_style());
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let layout = ModalListLayout::new(inner, instructions.len());
        if let Some(text_area) = layout.text_area {
            let paragraph = Paragraph::new(instructions).wrap(Wrap { trim: true });
            frame.render_widget(paragraph, text_area);
        }

        let mut list_items: Vec<ListItem> = items
            .iter()
            .map(|(_, entry, is_selected)| {
                let base_style = if *is_selected {
                    self.modal_list_highlight_style()
                } else {
                    self.default_style()
                };

                // Add visual distinction for directories
                let style = if entry.is_dir {
                    base_style.add_modifier(Modifier::BOLD)
                } else {
                    base_style
                };

                // Add icon prefix
                let prefix = if entry.is_dir {
                    "↳  " // Folder indicator
                } else {
                    "  · " // Indent files
                };

                let display_text = format!("{}{}", prefix, entry.display_name);

                ListItem::new(Line::from(Span::styled(display_text, style)))
            })
            .collect();

        // Add continuation indicator if there are more items
        if palette.has_more_items() {
            let continuation_text = format!(
                "  ... ({} more items)",
                palette.total_items() - (palette.current_page_number() * 20)
            );
            let continuation_style = self
                .default_style()
                .add_modifier(Modifier::DIM | Modifier::ITALIC);
            list_items.push(ListItem::new(Line::from(Span::styled(
                continuation_text,
                continuation_style,
            ))));
        }

        let list = List::new(list_items).style(self.default_style());
        frame.render_widget(list, layout.list_area);
    }

    fn render_file_palette_loading(&self, frame: &mut Frame<'_>, viewport: Rect) {
        let width_hint = 40u16;
        let modal_height = 3;
        let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

        frame.render_widget(Clear, area);
        let block = Block::default()
            .title("File Browser")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(self.default_style())
            .border_style(self.border_style());
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height > 0 && inner.width > 0 {
            let loading_text = vec![Line::from(Span::styled(
                "Loading workspace files...".to_string(),
                self.default_style().add_modifier(Modifier::DIM),
            ))];
            let paragraph = Paragraph::new(loading_text).wrap(Wrap { trim: true });
            frame.render_widget(paragraph, inner);
        }
    }

    fn file_palette_instructions(&self, palette: &FilePalette) -> Vec<Line<'static>> {
        let mut lines = vec![];

        if palette.is_empty() {
            lines.push(Line::from(Span::styled(
                "No files found matching filter".to_string(),
                self.default_style().add_modifier(Modifier::DIM),
            )));
        } else {
            let total = palette.total_items();
            let count_text = if total == 1 {
                "1 file".to_string()
            } else {
                format!("{} files", total)
            };

            let nav_text = "↑↓ Navigate · PgUp/PgDn Page · Tab/Enter Select";

            lines.push(Line::from(vec![Span::styled(
                format!("{} · Esc Close", nav_text),
                self.default_style(),
            )]));

            lines.push(Line::from(vec![
                Span::styled(
                    format!("Showing {}", count_text),
                    self.default_style().add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    if !palette.filter_query().is_empty() {
                        format!(" matching '{}'", palette.filter_query())
                    } else {
                        String::new()
                    },
                    self.accent_style(),
                ),
            ]));
        }

        lines
    }

    fn render_prompt_palette(&mut self, frame: &mut Frame<'_>, viewport: Rect) {
        if !self.prompt_palette_active {
            return;
        }

        let Some(palette) = self.prompt_palette.as_ref() else {
            return;
        };

        if viewport.height == 0 || viewport.width == 0 || self.modal.is_some() {
            return;
        }

        // Show loading state if no prompts loaded yet
        if !palette.has_prompts() {
            self.render_prompt_palette_loading(frame, viewport);
            return;
        }

        let items = palette.current_page_items();
        if items.is_empty() && palette.filter_query().is_empty() {
            return;
        }

        let mut width_hint = 40u16;
        for (_, entry, _) in &items {
            width_hint = width_hint.max(measure_text_width(&entry.name) + 4);
        }

        let instructions = self.prompt_palette_instructions(palette);
        let has_continuation = palette.has_more_items();
        let modal_height = items.len()
            + instructions.len()
            + 2  // borders
            + if has_continuation { 1 } else { 0 }; // continuation indicator
        let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

        frame.render_widget(Clear, area);
        let title = format!(
            "Custom Prompts (Page {}/{})",
            palette.current_page_number(),
            palette.total_pages()
        );
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(self.default_style())
            .border_style(self.border_style());
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let layout = ModalListLayout::new(inner, instructions.len());
        if let Some(text_area) = layout.text_area {
            let paragraph = Paragraph::new(instructions).wrap(Wrap { trim: true });
            frame.render_widget(paragraph, text_area);
        }

        let mut list_items: Vec<ListItem> = items
            .iter()
            .map(|(_, entry, is_selected)| {
                let base_style = if *is_selected {
                    self.modal_list_highlight_style()
                } else {
                    self.default_style()
                };

                // Format: "  · prompt-name"
                let display_text = format!("  · {}", entry.name);

                ListItem::new(Line::from(Span::styled(display_text, base_style)))
            })
            .collect();

        // Add continuation indicator if there are more items
        if palette.has_more_items() {
            let continuation_text = format!(
                "  ... ({} more items)",
                palette.total_items() - (palette.current_page_number() * 20)
            );
            let continuation_style = self
                .default_style()
                .add_modifier(Modifier::DIM | Modifier::ITALIC);
            list_items.push(ListItem::new(Line::from(Span::styled(
                continuation_text,
                continuation_style,
            ))));
        }

        let list = List::new(list_items).style(self.default_style());
        frame.render_widget(list, layout.list_area);
    }

    fn render_prompt_palette_loading(&self, frame: &mut Frame<'_>, viewport: Rect) {
        let width_hint = 40u16;
        let modal_height = 3;
        let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

        frame.render_widget(Clear, area);
        let block = Block::default()
            .title("Custom Prompts")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(self.default_style())
            .border_style(self.border_style());
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height > 0 && inner.width > 0 {
            let loading_text = vec![Line::from(Span::styled(
                "Loading custom prompts...".to_string(),
                self.default_style().add_modifier(Modifier::DIM),
            ))];
            let paragraph = Paragraph::new(loading_text).wrap(Wrap { trim: true });
            frame.render_widget(paragraph, inner);
        }
    }

    fn prompt_palette_instructions(&self, palette: &PromptPalette) -> Vec<Line<'static>> {
        let mut lines = vec![];

        if palette.is_empty() {
            lines.push(Line::from(Span::styled(
                "No prompts found matching filter".to_string(),
                self.default_style().add_modifier(Modifier::DIM),
            )));
        } else {
            let total = palette.total_items();
            let count_text = if total == 1 {
                "1 prompt".to_string()
            } else {
                format!("{} prompts", total)
            };

            lines.push(Line::from(vec![Span::styled(
                "↑↓ Navigate · Enter/Tab Select · Esc Close",
                self.default_style(),
            )]));

            lines.push(Line::from(vec![
                Span::styled(
                    format!("Showing {}", count_text),
                    self.default_style().add_modifier(Modifier::DIM),
                ),
                Span::styled(
                    if !palette.filter_query().is_empty() {
                        format!(" matching '{}'", palette.filter_query())
                    } else {
                        String::new()
                    },
                    self.accent_style(),
                ),
            ]));
        }

        lines
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

            // Replace the input with the selected custom prompt in /prompt:name format
            let mut new_input = String::from(PROMPT_COMMAND_PREFIX);
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

        // Store command name before borrowing self mutably
        let command_name = command.name.to_string();

        let Some(range) = command_range(&self.input, self.cursor) else {
            return false;
        };

        let suffix = self.input[range.end..].to_string();
        let mut new_input = format!("/{}", command_name);

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

        // Special handling: if /files command was selected, defer file browser opening
        if command_name == "files" {
            // Clear slash modal first
            self.clear_slash_suggestions();
            self.mark_dirty();

            // Set flag to open file browser on next render cycle (after modal fully dismisses)
            self.deferred_file_browser_trigger = true;
        } else if command_name == PROMPT_COMMAND_NAME || command_name == LEGACY_PROMPT_COMMAND_NAME
        {
            // Clear slash modal first
            self.clear_slash_suggestions();
            self.mark_dirty();

            // Set flag to open prompt browser on next render cycle (after modal fully dismisses)
            self.deferred_prompt_browser_trigger = true;
        } else {
            // For other commands, update slash suggestions normally
            self.update_slash_suggestions();
            self.mark_dirty();
        }

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
                // Parse ANSI escape sequences in PTY output for color support
                // Limit to last 30 lines for performance and readability
                let output_text = if header_text.lines().count() > 30 {
                    let lines: Vec<&str> = header_text.lines().collect();
                    let start = lines.len().saturating_sub(30);
                    format!(
                        "[... {} lines truncated ...]\n{}",
                        lines.len() - 30,
                        lines[start..].join("\n")
                    )
                } else {
                    header_text.clone()
                };

                if let Ok(parsed) = output_text.as_bytes().into_text() {
                    let base_style = parsed.style;
                    for line in &parsed.lines {
                        let line_style = base_style.patch(line.style);
                        for span in &line.spans {
                            let content = span.content.clone().into_owned();
                            if content.is_empty() {
                                continue;
                            }
                            let span_style = line_style.patch(span.style);
                            spans.push(Span::styled(content, span_style));
                        }
                        // Add newline between lines
                        spans.push(Span::raw("\n"));
                    }
                    // Remove trailing newline
                    if spans.last().map(|s| s.content.as_ref()) == Some("\n") {
                        spans.pop();
                    }
                } else {
                    // Fallback to plain text if ANSI parsing fails
                    spans.push(Span::styled(
                        output_text,
                        ratatui_style_from_inline(&body_style, self.theme.foreground),
                    ));
                }
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

        // Always render tool calls as a single combined line
        self.render_tool_header_line(&combined)
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
        }

        if remaining.is_empty() {
            return spans;
        }

        remaining = self.strip_tool_status_prefix(remaining);
        if remaining.is_empty() {
            return spans;
        }

        let (name, tail) = if remaining.starts_with('[') {
            if let Some(end) = remaining.find(']') {
                let name = &remaining[1..end];
                let tail = &remaining[end + 1..];
                (name, tail)
            } else {
                (remaining, "")
            }
        } else {
            let mut name_end = remaining.len();
            for (index, character) in remaining.char_indices() {
                if character.is_whitespace() {
                    name_end = index;
                    break;
                }
            }
            remaining.split_at(name_end)
        };
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

                // Parse and style the action text with special highlighting
                self.render_styled_action_text(&mut spans, action, &body_style);

                // Format additional parameters (limit to avoid multi-line)
                let max_parts = 3; // Limit parameters to keep on one line
                for (i, part) in parts[1..].iter().enumerate() {
                    if i >= max_parts {
                        spans.push(Span::raw(" "));
                        spans.push(Span::styled(
                            "· ...",
                            self.accent_style().add_modifier(Modifier::DIM),
                        ));
                        break;
                    }
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        "·",
                        self.accent_style().add_modifier(Modifier::DIM),
                    ));
                    spans.push(Span::raw(" "));

                    // Differentiate between parameter names and values
                    let param_parts: Vec<&str> = part.split(": ").collect();
                    if param_parts.len() > 1 {
                        // Parameter name (before colon) - in accent color with bold
                        spans.push(Span::styled(
                            format!("{}: ", param_parts[0]),
                            self.accent_style().add_modifier(Modifier::BOLD),
                        ));

                        // Parameter value (after colon) - highlighted with different color
                        let mut value_style = InlineTextStyle::default();
                        value_style.color = Some(AnsiColor::Green.into()); // Green for argument values
                        value_style.bold = true;
                        spans.push(Span::styled(
                            param_parts[1].to_string(),
                            ratatui_style_from_inline(&value_style, self.theme.foreground),
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
                let mut simplified_text = self.simplify_tool_display(trimmed_tail);
                // Truncate to fit in one line (approximately 100 characters for readability)
                if simplified_text.len() > 100 {
                    simplified_text = simplified_text.chars().take(97).collect::<String>() + "...";
                }
                spans.push(Span::styled(
                    simplified_text,
                    ratatui_style_from_inline(&body_style, self.theme.foreground),
                ));
            }
        }

        spans
    }

    fn render_styled_action_text(
        &self,
        spans: &mut Vec<Span<'static>>,
        action: &str,
        body_style: &InlineTextStyle,
    ) {
        let words: Vec<&str> = action.split_whitespace().collect();

        for (i, word) in words.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" "));
            }

            if *word == "in" {
                // Style "in" with italic and different color (cyan)
                let mut in_style = InlineTextStyle::default();
                in_style.color = Some(AnsiColor::Cyan.into());
                in_style.italic = true;
                spans.push(Span::styled(
                    word.to_string(),
                    ratatui_style_from_inline(&in_style, self.theme.foreground),
                ));
            } else if i < 2
                && (word.starts_with("List")
                    || word.starts_with("Read")
                    || word.starts_with("Write")
                    || word.starts_with("Find")
                    || word.starts_with("Search")
                    || word.starts_with("Run"))
            {
                // Highlight the main action verb (first 1-2 words) with bold and accent color
                let mut action_style = InlineTextStyle::default();
                action_style.color = self.theme.tool_accent.or(Some(AnsiColor::Yellow.into()));
                action_style.bold = true;
                spans.push(Span::styled(
                    word.to_string(),
                    ratatui_style_from_inline(&action_style, self.theme.foreground),
                ));
            } else {
                // Normal styling for other words
                spans.push(Span::styled(
                    word.to_string(),
                    ratatui_style_from_inline(body_style, self.theme.foreground),
                ));
            }
        }
    }

    fn strip_tool_status_prefix<'a>(&self, text: &'a str) -> &'a str {
        let trimmed = text.trim_start();
        const STATUS_ICONS: [&str; 4] = ["✓", "✗", "~", "✕"];
        for icon in STATUS_ICONS {
            if trimmed.starts_with(icon) {
                return trimmed[icon.len()..].trim_start();
            }
        }
        text
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

    pub fn mark_dirty(&mut self) {
        self.needs_redraw = true;
    }

    fn toggle_timeline_pane(&mut self) {
        self.show_timeline_pane = !self.show_timeline_pane;
        self.invalidate_scroll_metrics();
        self.mark_dirty();
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
            // Force full screen clear on next render to remove modal artifacts
            self.needs_full_clear = true;
            // Force transcript cache invalidation to ensure full redraw
            self.transcript_cache = None;
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

        let block = Block::default()
            .title(Line::styled(modal.title.clone(), styles.title))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(styles.border);

        frame.render_widget(Clear, area);
        frame.render_widget(block, area);

        if area.width <= 2 || area.height <= 2 {
            return;
        }

        let inner = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
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

    fn clear_screen(&mut self) {
        self.lines.clear();
        self.scroll_offset = 0;
        self.invalidate_transcript_cache();
        self.invalidate_scroll_metrics();
        self.needs_full_clear = true;
        self.mark_dirty();
    }

    pub fn set_custom_prompts(&mut self, custom_prompts: CustomPromptRegistry) {
        // Initialize prompt palette when custom prompts are loaded
        if custom_prompts.enabled() && !custom_prompts.is_empty() {
            let mut palette = PromptPalette::new();
            palette.load_prompts(custom_prompts.iter());
            self.prompt_palette = Some(palette);
        }

        self.custom_prompts = Some(custom_prompts);
        // Update slash palette if we're currently viewing slash commands
        if self.input.starts_with('/') {
            self.update_slash_suggestions();
        }
    }

    fn load_file_palette(&mut self, files: Vec<String>, workspace: std::path::PathBuf) {
        let mut palette = FilePalette::new(workspace);
        palette.load_files(files);
        self.file_palette = Some(palette);
        self.file_palette_active = false;
        self.check_file_reference_trigger();
    }

    fn check_file_reference_trigger(&mut self) {
        if let Some(palette) = self.file_palette.as_mut() {
            if let Some((_, _, query)) = extract_file_reference(&self.input, self.cursor) {
                // Reset selection and clear previous state when opening
                palette.reset();
                palette.set_filter(query);
                self.file_palette_active = true;
            } else {
                self.file_palette_active = false;
            }
        }
    }

    fn close_file_palette(&mut self) {
        self.file_palette_active = false;

        // Clean up resources when closing to free memory
        if let Some(palette) = self.file_palette.as_mut() {
            palette.cleanup();
        }
    }

    fn handle_file_palette_key(&mut self, key: &KeyEvent) -> bool {
        if !self.file_palette_active {
            return false;
        }

        let Some(palette) = self.file_palette.as_mut() else {
            return false;
        };

        match key.code {
            KeyCode::Up => {
                palette.move_selection_up();
                self.mark_dirty();
                true
            }
            KeyCode::Down => {
                palette.move_selection_down();
                self.mark_dirty();
                true
            }
            KeyCode::PageUp => {
                palette.page_up();
                self.mark_dirty();
                true
            }
            KeyCode::PageDown => {
                palette.page_down();
                self.mark_dirty();
                true
            }
            KeyCode::Home => {
                palette.move_to_first();
                self.mark_dirty();
                true
            }
            KeyCode::End => {
                palette.move_to_last();
                self.mark_dirty();
                true
            }
            KeyCode::Esc => {
                self.close_file_palette();
                self.mark_dirty();
                true
            }
            KeyCode::Tab => {
                if let Some(entry) = palette.get_selected() {
                    let path = entry.relative_path.clone();
                    self.insert_file_reference(&path);
                    self.close_file_palette();
                    self.mark_dirty();
                }
                true
            }
            KeyCode::Enter => {
                if let Some(entry) = palette.get_selected() {
                    let path = entry.relative_path.clone();
                    self.insert_file_reference(&path);
                    self.close_file_palette();
                    self.mark_dirty();
                }
                true
            }
            _ => false,
        }
    }

    fn check_prompt_reference_trigger(&mut self) {
        // Initialize prompt palette on-demand if it doesn't exist
        if self.prompt_palette.is_none() {
            let mut palette = PromptPalette::new();

            // Try loading from custom_prompts first
            let loaded = if let Some(ref custom_prompts) = self.custom_prompts {
                if custom_prompts.enabled() && !custom_prompts.is_empty() {
                    palette.load_prompts(custom_prompts.iter());
                    true
                } else {
                    false
                }
            } else {
                false
            };

            // Fallback: load directly from filesystem if custom_prompts not available
            if !loaded {
                // Try default .vtcode/prompts directory
                if let Ok(current_dir) = std::env::current_dir() {
                    let prompts_dir = current_dir.join(".vtcode").join("prompts");
                    palette.load_from_directory(&prompts_dir);
                }
            }

            if let Ok(current_dir) = std::env::current_dir() {
                let core_dir = current_dir.join(prompts::CORE_BUILTIN_PROMPTS_DIR);
                palette.load_from_directory(&core_dir);
            }

            let builtin_prompts = CustomPromptRegistry::builtin_prompts();
            if !builtin_prompts.is_empty() {
                palette.append_custom_prompts(builtin_prompts.iter());
            }

            self.prompt_palette = Some(palette);
        }

        if let Some(palette) = self.prompt_palette.as_mut() {
            if let Some((_, _, query)) = extract_prompt_reference(&self.input, self.cursor) {
                // Reset selection and clear previous state when opening
                palette.reset();
                palette.set_filter(query);
                self.prompt_palette_active = true;
            } else {
                self.prompt_palette_active = false;
            }
        }
    }

    fn close_prompt_palette(&mut self) {
        self.prompt_palette_active = false;

        // Clean up resources when closing to free memory
        if let Some(palette) = self.prompt_palette.as_mut() {
            palette.cleanup();
        }
    }

    fn handle_prompt_palette_key(&mut self, key: &KeyEvent) -> bool {
        if !self.prompt_palette_active {
            return false;
        }

        let Some(palette) = self.prompt_palette.as_mut() else {
            return false;
        };

        match key.code {
            KeyCode::Up => {
                palette.move_selection_up();
                self.mark_dirty();
                true
            }
            KeyCode::Down => {
                palette.move_selection_down();
                self.mark_dirty();
                true
            }
            KeyCode::PageUp => {
                palette.page_up();
                self.mark_dirty();
                true
            }
            KeyCode::PageDown => {
                palette.page_down();
                self.mark_dirty();
                true
            }
            KeyCode::Home => {
                palette.move_to_first();
                self.mark_dirty();
                true
            }
            KeyCode::End => {
                palette.move_to_last();
                self.mark_dirty();
                true
            }
            KeyCode::Esc => {
                self.close_prompt_palette();
                self.mark_dirty();
                true
            }
            KeyCode::Tab | KeyCode::Enter => {
                if let Some(entry) = palette.get_selected() {
                    let prompt_name = entry.name.clone();
                    self.insert_prompt_reference(&prompt_name);
                    self.close_prompt_palette();
                    self.mark_dirty();
                }
                true
            }
            _ => false,
        }
    }

    fn insert_prompt_reference(&mut self, prompt_name: &str) {
        let mut command = String::from(PROMPT_COMMAND_PREFIX);
        command.push_str(prompt_name);
        command.push(' ');

        self.input = command;
        self.cursor = self.input.len();
        self.update_slash_suggestions();
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

        if self.handle_file_palette_key(&key) {
            return None;
        }

        if self.handle_prompt_palette_key(&key) {
            return None;
        }

        if self.try_handle_slash_navigation(&key, has_control, has_alt, has_command) {
            return None;
        }

        match key.code {
            KeyCode::Char('c') | KeyCode::Char('C') if has_control => {
                self.mark_dirty();
                Some(InlineEvent::Interrupt)
            }
            KeyCode::Char(c) if c == '' => {
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
                    // Handle double escape to clear input
                    let now = Instant::now();
                    let is_double_escape = if let Some(last_time) = self.last_escape_time {
                        now.duration_since(last_time).as_millis() < 500 // 500ms timeout for double escape
                    } else {
                        false
                    };

                    if is_double_escape && !self.input.is_empty() {
                        // Double escape detected - clear the input
                        self.clear_input();
                        self.mark_dirty();
                        None // Don't send an event, just clear the input
                    } else {
                        // Single escape - either send cancel event or update last escape time
                        self.last_escape_time = Some(now);
                        self.mark_dirty();
                        Some(InlineEvent::Cancel)
                    }
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

                // Only scroll transcript if not navigating history
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

                // Only scroll transcript if not navigating history
                self.scroll_line_down();
                self.mark_dirty();
                Some(InlineEvent::ScrollLineDown)
            }
            KeyCode::Enter => {
                if !self.input_enabled {
                    return None;
                }

                if self.file_palette_active {
                    if let Some(palette) = self.file_palette.as_ref() {
                        if let Some(entry) = palette.get_selected() {
                            let file_path = entry.path.clone();
                            self.insert_file_reference(&file_path);
                            self.close_file_palette();
                            self.mark_dirty();
                            return Some(InlineEvent::FileSelected(file_path));
                        }
                    }
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

                // Don't submit empty input
                if submitted.trim().is_empty() {
                    self.mark_dirty();
                    return None;
                }

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
                    if has_alt {
                        // Alt+Backspace (Option+Backspace on Mac) - delete word backwards
                        self.delete_word_backward();
                    } else if has_command {
                        // Command+Backspace (Mac) - delete sentence backwards
                        self.delete_sentence_backward();
                    } else {
                        // Standard Backspace - backward delete of single character
                        self.delete_char();
                    }
                    self.check_file_reference_trigger();
                    self.check_prompt_reference_trigger();
                    self.mark_dirty();
                }
                None
            }
            KeyCode::Delete => {
                if self.input_enabled {
                    if has_alt {
                        // Alt+Delete (Option+Delete on Mac) - delete word backwards
                        self.delete_word_backward();
                    } else if has_command {
                        // Command+Delete (Mac) - delete sentence backwards
                        self.delete_sentence_backward();
                    } else {
                        // Standard Delete - forward delete
                        self.delete_char_forward();
                    }
                    self.check_file_reference_trigger();
                    self.check_prompt_reference_trigger();
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
            KeyCode::Char('t') | KeyCode::Char('T') if has_control => {
                self.toggle_timeline_pane();
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
                    self.check_file_reference_trigger();
                    self.check_prompt_reference_trigger();
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

    fn insert_file_reference(&mut self, file_path: &str) {
        if let Some((start, end, _)) = extract_file_reference(&self.input, self.cursor) {
            let replacement = format!("@{}", file_path);
            self.input.replace_range(start..end, &replacement);
            self.cursor = start + replacement.len();
            self.input.insert(self.cursor, ' ');
            self.cursor += 1;
        }
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

    fn delete_char_forward(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }

        if let Some((index, _)) = self
            .input
            .char_indices()
            .find(|(idx, _)| *idx >= self.cursor)
        {
            self.input.drain(index..(index + 1));
            // cursor stays the same as characters shift left
            self.update_slash_suggestions();
        }
    }

    fn delete_word_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }

        // Find the start of the current word by moving backward (same logic as move_left_word)
        let graphemes: Vec<(usize, &str)> =
            self.input[..self.cursor].grapheme_indices(true).collect();

        if graphemes.is_empty() {
            return;
        }

        let mut index = graphemes.len();

        // Skip any trailing whitespace
        while index > 0 {
            let (_, grapheme) = graphemes[index - 1];
            if grapheme.chars().all(char::is_whitespace) {
                index -= 1;
            } else {
                break;
            }
        }

        // Move backwards until we find whitespace (start of the word)
        while index > 0 {
            let (_, grapheme) = graphemes[index - 1];
            if grapheme.chars().all(char::is_whitespace) {
                break;
            }
            index -= 1;
        }

        // Calculate the position to delete from
        let delete_start = if index < graphemes.len() {
            graphemes[index].0
        } else {
            0
        };

        // Delete from delete_start to cursor
        if delete_start < self.cursor {
            self.input.drain(delete_start..self.cursor);
            self.cursor = delete_start;
            self.update_slash_suggestions();
        }
    }

    fn delete_sentence_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let input_before_cursor = &self.input[..self.cursor];
        let chars: Vec<(usize, char)> = input_before_cursor.char_indices().collect();

        if chars.is_empty() {
            return;
        }

        // Look backwards from cursor for the most recent sentence ending followed by whitespace
        // A sentence typically ends with ., !, ? followed by space, tab, newline or end of input
        let mut delete_start = 0;

        // Search backwards to find the most recent sentence boundary
        for i in (0..chars.len()).rev() {
            let (pos, ch) = chars[i];

            if matches!(ch, '.' | '!' | '?') {
                // Check if this punctuation is followed by whitespace or we're at the end
                // Since we're looking at input before cursor, we check the original full input
                if pos + ch.len_utf8() < self.input.len() {
                    // Check the character after the punctuation in the full input string
                    let after_punct = &self.input[pos + ch.len_utf8()..self.cursor];
                    if !after_punct.is_empty() {
                        let next_char = after_punct.chars().next().unwrap();
                        if next_char.is_whitespace() {
                            // Found sentence ending punctuation followed by whitespace
                            delete_start = pos + ch.len_utf8();
                            break;
                        }
                    } else {
                        // At the end of the text being considered (before cursor)
                        // This might be a sentence boundary if there's whitespace after cursor
                        delete_start = pos + ch.len_utf8();
                        break;
                    }
                } else {
                    // At the end of the entire input string
                    delete_start = pos + ch.len_utf8();
                    break;
                }
            } else if matches!(ch, '\n' | '\r') {
                // Newlines can also separate sentences
                delete_start = pos + ch.len_utf8();
                break;
            }
        }

        // Delete from delete_start to cursor
        if delete_start < self.cursor {
            self.input.drain(delete_start..self.cursor);
            self.cursor = delete_start;
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
                // Don't update slash suggestions during history navigation to prevent popup from showing
                // Slash suggestions will be updated when user starts typing normally
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
                // Don't update slash suggestions during history navigation to prevent popup from showing
                // Slash suggestions will be updated when user starts typing normally
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
        let previous = self.scroll_offset;
        if self.scroll_metrics_dirty {
            self.scroll_offset = self.scroll_offset.saturating_add(1);
        } else if self.cached_max_scroll_offset == 0 {
            self.scroll_offset = 0;
        } else {
            self.scroll_offset = min(self.scroll_offset + 1, self.cached_max_scroll_offset);
        }
        if self.scroll_offset != previous {
            self.needs_full_clear = true;
        }
    }

    fn scroll_line_down(&mut self) {
        let previous = self.scroll_offset;
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
        if self.scroll_offset != previous {
            self.needs_full_clear = true;
        }
    }

    fn scroll_page_up(&mut self) {
        let previous = self.scroll_offset;
        if self.scroll_metrics_dirty {
            self.scroll_offset = self
                .scroll_offset
                .saturating_add(self.viewport_height().max(1));
        } else if self.cached_max_scroll_offset == 0 {
            self.scroll_offset = 0;
        } else {
            let page = self.viewport_height().max(1);
            self.scroll_offset = min(self.scroll_offset + page, self.cached_max_scroll_offset);
        }
        if self.scroll_offset != previous {
            self.needs_full_clear = true;
        }
    }

    fn scroll_page_down(&mut self) {
        let previous = self.scroll_offset;
        let page = self.viewport_height().max(1);
        if self.scroll_offset > page {
            self.scroll_offset -= page;
        } else {
            self.scroll_offset = 0;
        }
        if self.scroll_offset != previous {
            self.needs_full_clear = true;
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
        let total_rows = self.total_transcript_rows(self.transcript_width) + effective_padding;
        let max_offset = total_rows.saturating_sub(viewport_rows);
        self.cached_max_scroll_offset = max_offset;
        self.scroll_metrics_dirty = false;
    }

    fn ensure_reflow_cache(&mut self, width: u16) -> &mut TranscriptReflowCache {
        let mut cache = self
            .transcript_cache
            .take()
            .unwrap_or_else(|| TranscriptReflowCache::new(width));

        let width_changed = cache.width != width;
        if width_changed {
            cache.width = width;
        }

        if cache.messages.len() > self.lines.len() {
            cache.messages.truncate(self.lines.len());
        } else if cache.messages.len() < self.lines.len() {
            cache
                .messages
                .resize_with(self.lines.len(), CachedMessage::default);
        }

        if cache.row_offsets.len() > self.lines.len() {
            cache.row_offsets.truncate(self.lines.len());
        } else if cache.row_offsets.len() < self.lines.len() {
            cache.row_offsets.resize(self.lines.len(), 0);
        }

        if width_changed {
            for entry in &mut cache.messages {
                entry.revision = 0;
            }
        }

        let mut dirty_start = if width_changed { 0 } else { self.lines.len() };
        if dirty_start != 0 {
            for (index, line) in self.lines.iter().enumerate() {
                if cache.messages[index].revision != line.revision {
                    dirty_start = index;
                    break;
                }
            }
        }

        if dirty_start == self.lines.len() {
            cache.total_rows = if let Some(last_index) = self.lines.len().checked_sub(1) {
                cache.row_offsets[last_index] + cache.messages[last_index].lines.len()
            } else {
                0
            };
            self.transcript_cache = Some(cache);
            return self.transcript_cache.as_mut().unwrap();
        }

        let mut total_rows = if dirty_start == 0 {
            0
        } else {
            let prev_index = dirty_start - 1;
            cache.row_offsets[prev_index] + cache.messages[prev_index].lines.len()
        };

        for index in dirty_start..self.lines.len() {
            let line = &self.lines[index];
            if cache.messages[index].revision != line.revision {
                let new_lines = self.reflow_message_lines(index, width);
                let entry = &mut cache.messages[index];
                entry.lines = new_lines;
                entry.revision = line.revision;
            }
            cache.row_offsets[index] = total_rows;
            total_rows += cache.messages[index].lines.len();
        }

        cache.total_rows = total_rows;
        self.transcript_cache = Some(cache);
        self.transcript_cache.as_mut().unwrap()
    }

    fn total_transcript_rows(&mut self, width: u16) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        let cache = self.ensure_reflow_cache(width);
        cache.total_rows
    }

    fn collect_transcript_window(
        &mut self,
        width: u16,
        start_row: usize,
        max_rows: usize,
    ) -> Vec<Line<'static>> {
        if max_rows == 0 {
            return Vec::new();
        }
        let cache = self.ensure_reflow_cache(width);
        if cache.total_rows == 0 || start_row >= cache.total_rows {
            return Vec::new();
        }

        let mut remaining = max_rows.min(cache.total_rows - start_row);
        let mut output = Vec::with_capacity(remaining);
        if cache.messages.is_empty() {
            return output;
        }

        let mut message_index = match cache.row_offsets.binary_search(&start_row) {
            Ok(idx) => idx,
            Err(0) => 0,
            Err(pos) => pos - 1,
        };

        let mut row = start_row;
        while message_index < cache.messages.len() && remaining > 0 {
            let message_start = cache.row_offsets[message_index];
            let entry = &cache.messages[message_index];
            if entry.lines.is_empty() {
                message_index += 1;
                continue;
            }
            let skip = row.saturating_sub(message_start);
            if skip >= entry.lines.len() {
                message_index += 1;
                continue;
            }
            for line in entry.lines.iter().skip(skip) {
                if remaining == 0 {
                    break;
                }
                output.push(line.clone());
                remaining -= 1;
                row += 1;
            }
            message_index += 1;
        }

        output
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
        let next_is_tool = self
            .lines
            .get(index + 1)
            .map(|next| next.kind == InlineMessageKind::Tool)
            .unwrap_or(false);

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
            // For simple tool output, render without borders
            let content = self.render_tool_segments(line);
            for segment in content {
                lines.push(Line::from(vec![segment]));
            }
        }

        if is_end {
            // Don't add bottom border for simple tool output
            // lines.push(self.block_footer_line(width, border_style));
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
            // Don't add bottom border for PTY output either
            // lines.push(self.block_footer_line(width, border_style));
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
    use super::prompt_palette;
    use super::*;
    use chrono::Utc;
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
        let offset = session.transcript_view_top;
        let lines = session.reflow_transcript_lines(width);

        let start = offset.min(lines.len());
        let mut visible: Vec<Line<'static>> =
            lines.into_iter().skip(start).take(viewport).collect();
        let filler = viewport.saturating_sub(visible.len());
        if filler > 0 {
            visible.extend((0..filler).map(|_| Line::default()));
        }
        session.overlay_queue_lines(&mut visible, width);

        visible
            .into_iter()
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
    fn shift_enter_inserts_newline() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        session.input = "queued".to_string();
        session.cursor = session.input.len();

        let result = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        assert!(result.is_none());
        assert_eq!(session.input, "queued\n");
        assert_eq!(session.cursor, session.input.len());
    }

    #[test]
    fn selecting_prompt_via_palette_updates_input_with_slash_command() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        let mut palette = PromptPalette::new();
        palette.append_entries(vec![prompt_palette::PromptEntry {
            name: "vtcode".to_string(),
            description: String::new(),
        }]);
        session.prompt_palette = Some(palette);
        session.prompt_palette_active = true;
        session.input = "#vt".to_string();
        session.cursor = session.input.len();

        let handled =
            session.handle_prompt_palette_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(handled);

        assert_eq!(session.input, "/prompt:vtcode ");
        assert_eq!(session.cursor, session.input.len());
        assert!(!session.prompt_palette_active);
    }

    #[test]
    fn control_enter_queues_submission() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        session.input = "queued".to_string();
        session.cursor = session.input.len();

        let queued = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert!(matches!(queued, Some(InlineEvent::QueueSubmit(value)) if value == "queued"));
    }

    #[test]
    fn command_enter_queues_submission() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        session.input = "queued".to_string();
        session.cursor = session.input.len();

        let queued = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SUPER));
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
        session.input = "notes".to_string();
        session.cursor = session.input.len();

        let title_line = session.header_title_line();
        let title_text: String = title_line
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();
        let provider_badge = format!(
            "[{}]",
            session.header_provider_short_value().to_ascii_uppercase()
        );
        assert!(title_text.contains(&provider_badge));
        assert!(title_text.contains(&session.header_model_short_value()));
        let reasoning_label = format!("({})", session.header_reasoning_short_value());
        assert!(title_text.contains(&reasoning_label));

        let meta_line = session.header_meta_line();
        let meta_text: String = meta_line
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();
        let mode_label = session.header_mode_short_label();
        assert!(meta_text.contains(&mode_label));
        for value in session.header_chain_values() {
            assert!(meta_text.contains(&value));
        }
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
        session.input = "notes".to_string();
        session.cursor = session.input.len();

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
        session.input = "notes".to_string();
        session.cursor = session.input.len();

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
        session.input = "notes".to_string();
        session.cursor = session.input.len();

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
        session.header_context.highlights = vec![InlineHeaderHighlight {
            title: "Tips".to_string(),
            lines: vec![
                "- Use /prompt:quick-start for boilerplate".to_string(),
                "- Keep responses focused".to_string(),
            ],
        }];
        session.input = "notes".to_string();
        session.cursor = session.input.len();

        let wide = session.header_height_for_width(120);
        let narrow = session.header_height_for_width(40);

        assert!(
            narrow >= wide,
            "expected narrower width to require at least as many header rows"
        );
        assert!(
            wide >= ui::INLINE_HEADER_HEIGHT && narrow >= ui::INLINE_HEADER_HEIGHT,
            "expected header rows to meet minimum height"
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
    fn queued_inputs_overlay_bottom_rows() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        session.push_line(
            InlineMessageKind::Agent,
            vec![make_segment("Latest response from agent")],
        );

        session.handle_command(InlineCommand::SetQueuedInputs {
            entries: vec![
                "first queued message".to_string(),
                "second queued message".to_string(),
                "third queued message".to_string(),
            ],
        });

        let view = visible_transcript(&mut session);
        let footer: Vec<String> = view.iter().rev().take(4).cloned().collect();

        assert!(
            footer
                .iter()
                .any(|line| line.contains("Queued messages (3)")),
            "queued header should be visible at the bottom of the transcript"
        );
        assert!(
            footer.iter().any(|line| line.contains("1.")),
            "first queued message label should be rendered"
        );
        assert!(
            footer.iter().any(|line| line.contains("2.")),
            "second queued message label should be rendered"
        );
        assert!(
            footer.iter().any(|line| line.contains("+1...")),
            "an indicator should show how many queued messages are hidden"
        );
        assert!(
            footer.iter().all(|line| !line.contains("3.")),
            "queued messages beyond the display limit should be hidden"
        );
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
    fn plan_sidebar_highlights_active_step() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        let mut plan = TaskPlan::default();
        plan.steps = vec![
            PlanStep {
                step: "Outline approach".to_string(),
                status: StepStatus::InProgress,
            },
            PlanStep {
                step: "Implement fix".to_string(),
                status: StepStatus::Pending,
            },
        ];
        plan.summary = PlanSummary::from_steps(&plan.steps);
        plan.version = 1;
        plan.updated_at = Utc::now();

        session.handle_command(InlineCommand::SetPlan { plan });

        let backend = TestBackend::new(VIEW_WIDTH, VIEW_ROWS);
        let mut terminal = Terminal::new(backend).expect("failed to create test terminal");
        terminal
            .draw(|frame| session.render(frame))
            .expect("failed to render session with plan sidebar");

        assert!(session.should_show_plan());
        assert_eq!(session.navigation_state.selected(), Some(0));

        let title: String = session
            .navigation_block_title()
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();
        assert!(title.contains(ui::PLAN_BLOCK_TITLE));
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
