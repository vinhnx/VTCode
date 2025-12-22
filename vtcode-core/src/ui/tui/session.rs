use std::{collections::VecDeque, sync::Arc};

#[cfg(test)]
use anstyle::Color as AnsiColorEnum;
use anstyle::RgbColor;
use ratatui::crossterm::event::{
    Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind,
};

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::{Line, Span, Text},
    widgets::{Clear, ListState, Widget},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::{
    style::{measure_text_width, ratatui_color_from_ansi, ratatui_style_from_inline},
    types::{
        InlineCommand, InlineEvent, InlineHeaderContext, InlineMessageKind, InlineTextStyle,
        InlineTheme,
    },
};
use crate::config::constants::ui;
use crate::ui::tui::widgets::SessionWidget;

pub mod file_palette;
mod header;
mod input;
mod input_manager;
mod message;
pub mod modal;
mod navigation;
mod palette_renderer;
pub mod prompt_palette;
mod queue;
pub mod render;
mod scroll;
pub mod slash;
pub mod slash_palette;
mod styling;
mod text_utils;
mod transcript;

// New modular components (refactored from main session.rs)
mod ansi_utils;
mod command;
pub mod config_palette;
mod editing;

mod events;
mod message_renderer;
mod messages;
mod palette;
mod reflow;
mod spinner;
mod state;
pub mod terminal_capabilities;
mod tool_renderer;

use self::config_palette::ConfigPalette;
use self::file_palette::FilePalette;
use self::input_manager::InputManager;
use self::message::{MessageLabels, MessageLine};
use self::modal::{ModalState, WizardModalState};

use self::prompt_palette::PromptPalette;
use self::queue::QueueOverlay;
use self::scroll::ScrollManager;
use self::slash_palette::SlashPalette;
use self::spinner::ThinkingSpinner;
use self::styling::SessionStyles;
use self::transcript::TranscriptReflowCache;
#[cfg(test)]
use super::types::InlineHeaderHighlight;
use crate::prompts::CustomPromptRegistry;
#[cfg(test)]
use crate::tools::PlanSummary;
use crate::tools::TaskPlan;
use crate::ui::tui::log::{LogEntry, highlight_log_entry};

const USER_PREFIX: &str = "";
const PLACEHOLDER_COLOR: RgbColor = RgbColor(0x88, 0x88, 0x88);
pub const PROMPT_COMMAND_NAME: &str = "prompt";
pub const LEGACY_PROMPT_COMMAND_NAME: &str = "prompts";
pub const PROMPT_INVOKE_PREFIX: &str = "prompt:";
pub const LEGACY_PROMPT_INVOKE_PREFIX: &str = "prompts:";
pub const PROMPT_COMMAND_PREFIX: &str = "/prompt:";
const MAX_LOG_LINES: usize = 256;

pub struct Session {
    // --- Managers (Phase 2) ---
    /// Manages user input, cursor, and command history
    pub(crate) input_manager: InputManager,
    /// Manages scroll state and viewport metrics
    pub(crate) scroll_manager: ScrollManager,

    // --- Message Management ---
    pub(crate) lines: Vec<MessageLine>,
    pub(crate) theme: InlineTheme,
    pub(crate) styles: SessionStyles,
    pub(crate) header_context: InlineHeaderContext,
    pub(crate) header_rows: u16,
    pub(crate) labels: MessageLabels,

    // --- Prompt/Input Display ---
    prompt_prefix: String,
    prompt_style: InlineTextStyle,
    placeholder: Option<String>,
    placeholder_style: Option<InlineTextStyle>,
    pub(crate) input_status_left: Option<String>,
    pub(crate) input_status_right: Option<String>,

    // --- UI State ---
    slash_palette: SlashPalette,
    navigation_state: ListState,
    plan_navigation_state: ListState,
    input_enabled: bool,
    cursor_visible: bool,
    pub(crate) needs_redraw: bool,
    pub(crate) needs_full_clear: bool,
    /// Track if transcript content changed (not just scroll position)
    pub(crate) transcript_content_changed: bool,
    should_exit: bool,
    pub(crate) view_rows: u16,
    pub(crate) input_height: u16,
    pub(crate) transcript_rows: u16,
    pub(crate) transcript_width: u16,
    pub(crate) transcript_view_top: usize,

    // --- Logging ---
    log_receiver: Option<UnboundedReceiver<LogEntry>>,
    log_lines: VecDeque<Arc<Text<'static>>>,
    log_cached_text: Option<Arc<Text<'static>>>,
    log_evicted: bool,
    pub(crate) show_logs: bool,

    // --- Rendering ---
    transcript_cache: Option<TranscriptReflowCache>,
    /// Cache of visible lines by (scroll_offset, width) - shared via Arc for zero-copy reads
    /// Avoids expensive clone on cache hits
    visible_lines_cache: Option<(usize, u16, Arc<Vec<Line<'static>>>)>,
    pub(crate) queued_inputs: Vec<String>,
    queue_overlay_cache: Option<QueueOverlay>,
    queue_overlay_version: u64,
    pub(crate) modal: Option<ModalState>,
    wizard_modal: Option<WizardModalState>,
    pub(crate) show_timeline_pane: bool,
    pub(crate) plan: TaskPlan,
    line_revision_counter: u64,
    /// Track the first line that needs reflow/update to avoid O(N) scans
    first_dirty_line: Option<usize>,
    in_tool_code_fence: bool,

    // --- Palette Management ---
    custom_prompts: Option<CustomPromptRegistry>,
    pub(crate) config_palette: Option<ConfigPalette>,
    pub(crate) config_palette_active: bool,
    pub(crate) file_palette: Option<FilePalette>,
    pub(crate) file_palette_active: bool,
    pub(crate) deferred_file_browser_trigger: bool,
    pub(crate) prompt_palette: Option<PromptPalette>,
    pub(crate) prompt_palette_active: bool,
    pub(crate) deferred_prompt_browser_trigger: bool,

    // --- Thinking Indicator ---
    pub(crate) thinking_spinner: ThinkingSpinner,
}

impl Session {
    /// Check if the content appears to be an error message that should go to transcript instead of input field
    fn is_error_content(content: &str) -> bool {
        // Check if message contains common error indicators
        let lower_content = content.to_lowercase();
        let error_indicators = [
            "error:",
            "error ",
            "error\n",
            "failed",
            "failure",
            "exception",
            "invalid",
            "not found",
            "couldn't",
            "can't",
            "cannot",
            "denied",
            "forbidden",
            "unauthorized",
            "timeout",
            "connection refused",
            "no such",
            "does not exist",
        ];

        error_indicators
            .iter()
            .any(|indicator| lower_content.contains(indicator))
    }

    #[allow(dead_code)]
    pub fn new(
        theme: InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_timeline_pane: bool,
    ) -> Self {
        Self::new_with_logs(theme, placeholder, view_rows, show_timeline_pane, true)
    }

    pub fn new_with_logs(
        theme: InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_timeline_pane: bool,
        show_logs: bool,
    ) -> Self {
        let resolved_rows = view_rows.max(2);
        let initial_header_rows = ui::INLINE_HEADER_HEIGHT;
        let reserved_rows = initial_header_rows + Self::input_block_height_for_lines(1);
        let initial_transcript_rows = resolved_rows.saturating_sub(reserved_rows).max(1);

        let mut session = Self {
            // --- Managers (Phase 2) ---
            input_manager: InputManager::new(),
            scroll_manager: ScrollManager::new(initial_transcript_rows),

            // --- Message Management ---
            lines: Vec::with_capacity(64),
            styles: SessionStyles::new(theme.clone()),
            theme,
            header_context: InlineHeaderContext::default(),
            labels: MessageLabels::default(),

            // --- Prompt/Input Display ---
            prompt_prefix: USER_PREFIX.to_string(),
            prompt_style: InlineTextStyle::default(),
            placeholder,
            placeholder_style: None,
            input_status_left: None,
            input_status_right: None,

            // --- UI State ---
            slash_palette: SlashPalette::new(),
            navigation_state: ListState::default(),
            plan_navigation_state: ListState::default(),
            input_enabled: true,
            cursor_visible: true,
            needs_redraw: true,
            needs_full_clear: false,
            transcript_content_changed: true,
            should_exit: false,
            view_rows: resolved_rows,
            input_height: Self::input_block_height_for_lines(1),
            transcript_rows: initial_transcript_rows,
            transcript_width: 0,
            transcript_view_top: 0,

            // --- Logging ---
            log_receiver: None,
            log_lines: VecDeque::with_capacity(MAX_LOG_LINES),
            log_cached_text: None,
            log_evicted: false,
            show_logs,

            // --- Rendering ---
            transcript_cache: None,
            visible_lines_cache: None,
            queued_inputs: Vec::with_capacity(4),
            queue_overlay_cache: None,
            queue_overlay_version: 0,
            modal: None,
            wizard_modal: None,
            show_timeline_pane,
            plan: TaskPlan::default(),
            header_rows: initial_header_rows,
            line_revision_counter: 0,
            first_dirty_line: None,
            in_tool_code_fence: false,

            // --- Palette Management ---
            custom_prompts: None,
            config_palette: None,
            config_palette_active: false,
            file_palette: None,
            file_palette_active: false,
            deferred_file_browser_trigger: false,
            prompt_palette: None,
            prompt_palette_active: false,
            deferred_prompt_browser_trigger: false,

            // --- Thinking Indicator ---
            thinking_spinner: ThinkingSpinner::new(),
        };
        session.ensure_prompt_style_color();
        session
    }

    /// Clears the thinking spinner message when agent response or error arrives
    pub(super) fn clear_thinking_spinner_if_active(&mut self, kind: InlineMessageKind) {
        if matches!(kind, InlineMessageKind::Agent | InlineMessageKind::Error)
            && self.thinking_spinner.is_active
        {
            if let Some(spinner_idx) = self.thinking_spinner.spinner_line_index
                && spinner_idx < self.lines.len()
            {
                self.lines.remove(spinner_idx);
                // Mark as dirty from the removed line index onwards
                self.mark_line_dirty(spinner_idx);
            }
            self.thinking_spinner.stop();
        }
    }

    /// Expose cursor position for tests.
    #[allow(dead_code)]
    pub fn cursor(&self) -> usize {
        self.input_manager.cursor()
    }

    /// Set input content (for tests and utilities).
    #[allow(dead_code)]
    pub fn set_input(&mut self, text: impl Into<String>) {
        self.input_manager.set_content(text.into());
        self.mark_dirty();
    }

    /// Set cursor position (for tests and utilities).
    #[allow(dead_code)]
    pub fn set_cursor(&mut self, pos: usize) {
        self.input_manager.set_cursor(pos);
        self.mark_dirty();
    }

    pub fn set_log_receiver(&mut self, receiver: UnboundedReceiver<LogEntry>) {
        self.log_receiver = Some(receiver);
    }

    pub(super) fn poll_log_entries(&mut self) {
        if !self.show_logs {
            // Drain without processing to avoid accumulation
            if let Some(receiver) = self.log_receiver.as_mut() {
                while receiver.try_recv().is_ok() {}
            }
            return;
        }

        let mut updated = false;
        if let Some(receiver) = self.log_receiver.as_mut() {
            let mut drained = Vec::new();
            while let Ok(entry) = receiver.try_recv() {
                drained.push(entry);
            }
            for entry in drained {
                let rendered = Arc::new(highlight_log_entry(&entry));
                self.push_log_line(rendered);
                updated = true;
            }
        }
        if updated {
            self.mark_dirty();
        }
    }

    fn push_log_line(&mut self, text: Arc<Text<'static>>) {
        if self.log_lines.len() >= MAX_LOG_LINES {
            self.log_lines.pop_front();
            self.log_evicted = true;
        }
        self.log_lines.push_back(text);
        self.log_cached_text = None;
    }

    pub(crate) fn has_logs(&self) -> bool {
        !self.log_lines.is_empty()
    }

    pub(crate) fn log_text(&mut self) -> Arc<Text<'static>> {
        if let Some(cached) = &self.log_cached_text {
            return Arc::clone(cached);
        }

        let mut text = Text::default();
        if self.log_evicted {
            text.lines.push(Line::from("(oldest logs dropped)"));
        }

        for entry in self.log_lines.iter() {
            text.lines.extend(entry.lines.clone());
        }

        if text.lines.is_empty() {
            text.lines.push(Line::from("No logs yet"));
        }

        let arc = Arc::new(text);
        self.log_cached_text = Some(Arc::clone(&arc));
        arc
    }

    /// Expose scroll offset for tests.
    #[allow(dead_code)]
    pub fn scroll_offset(&self) -> usize {
        self.scroll_manager.offset()
    }

    /// Expose default style for tests.
    #[allow(dead_code)]
    pub fn default_style(&self) -> InlineTextStyle {
        self.styles.default_inline_style()
    }

    /// Expose key processing for tests.
    #[allow(dead_code)]
    pub fn process_key(&mut self, key: KeyEvent) -> Option<InlineEvent> {
        events::process_key(self, key)
    }

    pub fn handle_command(&mut self, command: InlineCommand) {
        match command {
            InlineCommand::AppendLine { kind, segments } => {
                self.clear_thinking_spinner_if_active(kind);
                self.push_line(kind, segments);
                self.transcript_content_changed = true;
            }
            InlineCommand::Inline { kind, segment } => {
                self.clear_thinking_spinner_if_active(kind);
                self.append_inline(kind, segment);
                self.transcript_content_changed = true;
            }
            InlineCommand::ReplaceLast { count, kind, lines } => {
                self.clear_thinking_spinner_if_active(kind);
                self.replace_last(count, kind, lines);
                self.transcript_content_changed = true;
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
                self.theme = theme.clone();
                self.styles.set_theme(theme);
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
                slash::update_slash_suggestions(self);
            }
            InlineCommand::SetInput(content) => {
                // Check if the content appears to be an error message
                // If it looks like an error, redirect to transcript instead
                if Self::is_error_content(&content) {
                    // Add error to transcript instead of input field
                    crate::utils::transcript::display_error(&content);
                } else {
                    self.input_manager.set_content(content);
                    self.scroll_manager.set_offset(0);
                    slash::update_slash_suggestions(self);
                }
            }
            InlineCommand::ClearInput => {
                command::clear_input(self);
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
            InlineCommand::ShowWizardModal {
                title,
                steps,
                current_step: _,
                search,
            } => {
                self.show_wizard_modal(title, steps, search);
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
            InlineCommand::OpenConfigPalette => {
                command::open_config_palette(self);
            }
            InlineCommand::SuspendEventLoop
            | InlineCommand::ResumeEventLoop
            | InlineCommand::ClearInputQueue => {
                // Handled by drive_terminal
            }
            InlineCommand::Shutdown => {
                self.request_exit();
            }
        }
        self.needs_redraw = true;
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
                if matches!(key.kind, KeyEventKind::Press)
                    && let Some(outbound) = events::process_key(self, key)
                {
                    self.emit_inline_event(&outbound, events, callback);
                }
            }
            CrosstermEvent::Mouse(MouseEvent { kind, .. }) => match kind {
                MouseEventKind::ScrollDown => {
                    // Scroll mouse functionality disabled
                    // self.handle_scroll_down(events, callback);
                }
                MouseEventKind::ScrollUp => {
                    // Scroll mouse functionality disabled
                    // self.handle_scroll_up(events, callback);
                }
                _ => {}
            },
            CrosstermEvent::Paste(content) => {
                if self.input_enabled {
                    self.insert_text(&content);
                    self.check_file_reference_trigger();
                    self.check_prompt_reference_trigger();
                    self.mark_dirty();
                } else if let Some(modal) = self.modal.as_mut()
                    && let (Some(list), Some(search)) = (modal.list.as_mut(), modal.search.as_mut())
                {
                    search.insert(&content);
                    list.apply_search(&search.query);
                    self.mark_dirty();
                }
            }
            CrosstermEvent::Resize(_, rows) => {
                self.apply_view_rows(rows);
                self.mark_dirty();
            }
            _ => {}
        }
    }

    pub fn apply_config(&mut self, config: &crate::config::loader::VTCodeConfig) {
        self.show_timeline_pane = config.ui.show_timeline_pane;

        // Apply theme changes in real-time
        if let Ok(_) = crate::ui::theme::set_active_theme(&config.agent.theme) {
            let active_styles = crate::ui::theme::active_styles();
            let inline_theme = crate::ui::tui::style::theme_from_styles(&active_styles);

            self.theme = inline_theme.clone();
            self.styles.set_theme(inline_theme);

            // Re-apply theme to prompt prefix if needed (though it usually uses self.theme)
            self.prompt_style.color = self.theme.primary.or(self.theme.foreground);
        }

        self.recalculate_transcript_rows();
        self.mark_dirty();
    }

    /// Emits an InlineEvent through the event channel and callback
    ///
    /// This helper consolidates the common pattern of:
    /// 1. Calling the callback if present
    /// 2. Sending the event through the channel
    ///
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

        // Calculate layout constraints
        let header_lines = self.header_lines();
        let header_height = self.header_height_from_lines(viewport.width, &header_lines);
        if header_height != self.header_rows {
            self.header_rows = header_height;
            self.recalculate_transcript_rows();
        }

        let has_status = self
            .input_status_left
            .as_ref()
            .is_some_and(|v| !v.trim().is_empty())
            || self
                .input_status_right
                .as_ref()
                .is_some_and(|v| !v.trim().is_empty());
        let status_height = if viewport.width > 0 && has_status {
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

        // Use SessionWidget for buffer-based rendering (header, transcript, overlays)
        SessionWidget::new(self)
            .header_lines(header_lines.clone())
            .header_area(header_area)
            .transcript_area(transcript_area)
            .navigation_area(navigation_area)
            .render(viewport, frame.buffer_mut());

        // Handle frame-based rendering for components that need it
        // Note: header, transcript, and overlays are handled by SessionWidget
        if self.show_timeline_pane {
            self.render_navigation(frame, navigation_area);
        }
        self.render_input(frame, input_area);
        render::render_modal(self, frame, viewport);
        slash::render_slash_palette(self, frame, viewport);
        render::render_config_palette(self, frame, viewport);
        render::render_file_palette(self, frame, viewport);
        render::render_prompt_palette(self, frame, viewport);
    }

    fn set_plan(&mut self, plan: TaskPlan) {
        self.plan = plan;
        self.mark_dirty();
    }

    pub fn apply_view_rows(&mut self, rows: u16) {
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




    fn recalculate_transcript_rows(&mut self) {
        // Calculate reserved rows: header + input + borders (2)
        let header_rows = self.header_rows.max(ui::INLINE_HEADER_HEIGHT);
        let reserved = (header_rows + self.input_height).saturating_add(2);
        let available = self.view_rows.saturating_sub(reserved).max(1);

        if self.transcript_rows != available {
            self.transcript_rows = available;
            self.invalidate_scroll_metrics();
        }
    }

    #[allow(dead_code)]
    fn render_message_spans(&self, index: usize) -> Vec<Span<'static>> {
        let Some(line) = self.lines.get(index) else {
            return vec![Span::raw(String::new())];
        };
        message_renderer::render_message_spans(
            line,
            &self.theme,
            &self.labels,
            |kind| self.prefix_text(kind),
            |line| self.prefix_style(line),
            |kind| self.text_fallback(kind),
        )
    }

    // All palette, editing, message, reflow, and state methods moved to session/ modules
}

#[cfg(test)]
mod tests {
    use super::prompt_palette;
    use super::*;
    use crate::tools::{PlanStep, StepStatus};
    use crate::ui::tui::style::ratatui_style_from_inline;
    use crate::ui::tui::{InlineSegment, InlineTextStyle, InlineTheme};
    use chrono::Utc;
    use ratatui::crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{
        Terminal,
        backend::TestBackend,
        style::{Color, Modifier},
        text::{Line, Span},
    };
    use tokio::sync::mpsc;

    const VIEW_ROWS: u16 = 14;
    const VIEW_WIDTH: u16 = 100;
    const LINE_COUNT: usize = 10;
    const LABEL_PREFIX: &str = "line";
    const EXTRA_SEGMENT: &str = "\nextra-line";

    fn make_segment(text: &str) -> InlineSegment {
        InlineSegment {
            text: text.to_string(),
            style: std::sync::Arc::new(InlineTextStyle::default()),
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
        session.set_input(input.to_string());
        session.set_cursor(cursor);
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
        assert_eq!(session.input_manager.cursor(), 6);

        session.move_left_word();
        assert_eq!(session.input_manager.cursor(), 0);
    }

    #[test]
    fn move_left_word_skips_trailing_whitespace() {
        let text = "hello  world";
        let mut session = session_with_input(text, text.len());

        session.move_left_word();
        assert_eq!(session.input_manager.cursor(), 7);
    }

    #[test]
    fn arrow_keys_navigate_input_history() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        session.set_input("first message".to_string());
        let submit_first = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            matches!(submit_first, Some(InlineEvent::Submit(value)) if value == "first message")
        );

        session.set_input("second".to_string());
        let submit_second = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(submit_second, Some(InlineEvent::Submit(value)) if value == "second"));

        assert_eq!(session.input_manager.history().len(), 2);
        assert!(session.input_manager.content().is_empty());

        let up_latest = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
        assert!(up_latest.is_none());
        assert_eq!(session.input_manager.content(), "second");

        let up_previous = session.process_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
        assert!(up_previous.is_none());
        assert_eq!(session.input_manager.content(), "first message");

        let down_forward = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
        assert!(down_forward.is_none());
        assert_eq!(session.input_manager.content(), "second");

        let down_restore = session.process_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
        assert!(down_restore.is_none());
        assert!(session.input_manager.content().is_empty());
        assert!(session.input_manager.history_index().is_none());
    }

    #[test]
    fn shift_enter_inserts_newline() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        session.input_manager.set_content("queued".to_string());

        let result = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        assert!(result.is_none());
        assert_eq!(session.input_manager.content(), "queued\n");
        assert_eq!(
            session.input_manager.cursor(),
            session.input_manager.content().len()
        );
    }

    #[test]
    fn paste_preserves_all_newlines() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        let pasted = (0..15)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let (tx, _rx) = mpsc::unbounded_channel();

        session.handle_event(CrosstermEvent::Paste(pasted.clone()), &tx, None);

        assert_eq!(session.input_manager.content(), pasted);
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
        session.set_input("#vt".to_string());

        let handled =
            session.handle_prompt_palette_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(handled);

        assert_eq!(session.input_manager.content(), "/prompt:vtcode ");
        assert_eq!(
            session.input_manager.cursor(),
            session.input_manager.content().len()
        );
        assert!(!session.prompt_palette_active);
    }

    #[test]
    fn control_enter_queues_submission() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        session.set_input("queued".to_string());

        let queued = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert!(matches!(queued, Some(InlineEvent::QueueSubmit(value)) if value == "queued"));
    }

    #[test]
    fn command_enter_queues_submission() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        session.set_input("queued".to_string());

        let queued = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SUPER));
        assert!(matches!(queued, Some(InlineEvent::QueueSubmit(value)) if value == "queued"));
    }

    #[test]
    fn consecutive_duplicate_submissions_not_stored_twice() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        session.set_input("repeat".to_string());
        let first = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(first, Some(InlineEvent::Submit(value)) if value == "repeat"));

        session.set_input("repeat".to_string());
        let second = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(second, Some(InlineEvent::Submit(value)) if value == "repeat"));

        assert_eq!(session.input_manager.history().len(), 1);
    }

    #[test]
    fn alt_arrow_left_moves_cursor_by_word() {
        let text = "hello world";
        let mut session = session_with_input(text, text.len());

        let event = KeyEvent::new(KeyCode::Left, KeyModifiers::ALT);
        session.process_key(event);

        assert_eq!(session.cursor(), 6);
    }

    #[test]
    fn alt_b_moves_cursor_by_word() {
        let text = "hello world";
        let mut session = session_with_input(text, text.len());

        let event = KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT);
        session.process_key(event);

        assert_eq!(session.cursor(), 6);
    }

    #[test]
    fn move_right_word_advances_to_word_boundaries() {
        let text = "hello  world";
        let mut session = session_with_input(text, 0);

        session.move_right_word();
        assert_eq!(session.cursor(), 5);

        session.move_right_word();
        assert_eq!(session.cursor(), 7);

        session.move_right_word();
        assert_eq!(session.cursor(), text.len());
    }

    #[test]
    fn move_right_word_from_whitespace_moves_to_next_word_start() {
        let text = "hello  world";
        let mut session = session_with_input(text, 5);

        session.move_right_word();
        assert_eq!(session.cursor(), 7);
    }

    #[test]
    fn super_arrow_right_moves_cursor_to_end() {
        let text = "hello world";
        let mut session = session_with_input(text, 0);

        let event = KeyEvent::new(KeyCode::Right, KeyModifiers::SUPER);
        let result = session.process_key(event);

        assert_eq!(session.cursor(), text.len());
        // Ensure Command+Right does NOT launch editor
        assert!(!matches!(result, Some(InlineEvent::LaunchEditor)));
    }

    #[test]
    fn super_a_moves_cursor_to_start() {
        let text = "hello world";
        let mut session = session_with_input(text, text.len());

        let event = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::SUPER);
        session.process_key(event);

        assert_eq!(session.cursor(), 0);
    }

    #[test]
    fn super_e_moves_cursor_to_end() {
        let text = "hello world";
        let mut session = session_with_input(text, 0);

        let event = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::SUPER);
        let result = session.process_key(event);

        // Should move to end and return None (no event)
        assert!(result.is_none());
        assert_eq!(session.cursor(), text.len());
    }

    #[test]
    fn control_e_does_not_launch_editor() {
        let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);

        let event = KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL);
        let result = session.process_key(event);

        // Control+E keybinding has been removed - use /edit command instead
        assert!(!matches!(result, Some(InlineEvent::LaunchEditor)));
    }

    #[test]
    fn control_super_e_does_not_launch_editor() {
        let text = "hello world";
        let mut session = session_with_input(text, 0);

        let event = KeyEvent::new(
            KeyCode::Char('e'),
            KeyModifiers::CONTROL | KeyModifiers::SUPER,
        );
        let result = session.process_key(event);

        // Should not launch editor when both Control and Super (Cmd) are pressed
        assert!(!matches!(result, Some(InlineEvent::LaunchEditor)));
    }

    #[test]
    fn arrow_keys_never_launch_editor() {
        let text = "hello world";
        let mut session = session_with_input(text, 0);

        // Test Right arrow with all possible modifier combinations
        for modifiers in [
            KeyModifiers::empty(),
            KeyModifiers::CONTROL,
            KeyModifiers::SHIFT,
            KeyModifiers::ALT,
            KeyModifiers::SUPER,
            KeyModifiers::META,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            KeyModifiers::CONTROL | KeyModifiers::SUPER,
        ] {
            let event = KeyEvent::new(KeyCode::Right, modifiers);
            let result = session.process_key(event);
            assert!(
                !matches!(result, Some(InlineEvent::LaunchEditor)),
                "Right arrow with modifiers {:?} should not launch editor",
                modifiers
            );
        }

        // Test other arrow keys for safety
        for key_code in [KeyCode::Left, KeyCode::Up, KeyCode::Down] {
            let event = KeyEvent::new(key_code, KeyModifiers::SUPER);
            let result = session.process_key(event);
            assert!(
                !matches!(result, Some(InlineEvent::LaunchEditor)),
                "{:?} with SUPER should not launch editor",
                key_code
            );
        }
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
            let previous_offset = session.scroll_offset();
            session.scroll_page_up();
            if session.scroll_offset() == previous_offset {
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
        let scroll_offset = session.scroll_offset();
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
        assert!(session.scroll_offset() > 0);

        session.force_view_rows(
            (LINE_COUNT as u16)
                + ui::INLINE_HEADER_HEIGHT
                + Session::input_block_height_for_lines(1)
                + 2,
        );

        assert_eq!(session.scroll_offset(), 0);
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
            if session.scroll_offset() == session.current_max_scroll_offset() {
                break;
            }
        }
        assert!(session.scroll_offset() > 0);

        for _ in 0..total {
            session.scroll_page_down();
            if session.scroll_offset() == 0 {
                break;
            }
        }

        assert_eq!(session.scroll_offset(), 0);

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

        let top = match lines.first() {
            Some(line) => line_text(line),
            None => {
                tracing::error!("lines is empty despite assertion");
                return;
            }
        };
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
        session.input_manager.set_content("notes".to_string());
        session
            .input_manager
            .set_cursor(session.input_manager.content().len());

        let lines = session.header_lines();
        assert_eq!(lines.len(), 1);

        let line_text: String = lines[0]
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();
        assert!(line_text.contains(&session.header_provider_short_value()));
        assert!(line_text.contains(&session.header_model_short_value()));
        let reasoning_label = format!("Reasoning: {}", session.header_reasoning_short_value());
        assert!(line_text.contains(&reasoning_label));
        let mode_label = session.header_mode_short_label();
        assert!(line_text.contains(&mode_label));
        for value in session.header_chain_values() {
            assert!(line_text.contains(&value));
        }
        // Removed assertion for HEADER_MCP_PREFIX since we're no longer showing MCP info in header
        assert!(!line_text.contains("Languages"));
        assert!(!line_text.contains(ui::HEADER_STATUS_LABEL));
        assert!(!line_text.contains(ui::HEADER_MESSAGES_LABEL));
        assert!(!line_text.contains(ui::HEADER_INPUT_LABEL));
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
        session.input_manager.set_content("notes".to_string());
        session
            .input_manager
            .set_cursor(session.input_manager.content().len());

        let lines = session.header_lines();
        assert_eq!(lines.len(), 1);

        let summary: String = lines[0]
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
        session.input_manager.set_content("notes".to_string());
        session
            .input_manager
            .set_cursor(session.input_manager.content().len());

        let lines = session.header_lines();
        assert_eq!(lines.len(), 1);

        let summary: String = lines[0]
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
        session.input_manager.set_content("notes".to_string());
        session
            .input_manager
            .set_cursor(session.input_manager.content().len());

        let lines = session.header_lines();
        assert_eq!(lines.len(), 1);

        let summary: String = lines[0]
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
        session.input_manager.set_content("notes".to_string());
        session
            .input_manager
            .set_cursor(session.input_manager.content().len());

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
        let line = Line::from(vec![Span::styled(
            "你好世界".to_string(),
            ratatui_style_from_inline(&style, None),
        )]);

        let wrapped = session.wrap_line(line, 4);
        let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

        assert_eq!(rendered, vec!["你好".to_string(), "世界".to_string()]);
    }

    #[test]
    fn wrap_line_keeps_explicit_blank_rows() {
        let session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        let style = session.default_style();
        let line = Line::from(vec![Span::styled(
            "top\n\nbottom".to_string(),
            ratatui_style_from_inline(&style, None),
        )]);

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
        let line = Line::from(vec![Span::styled(
            "hi".to_string(),
            ratatui_style_from_inline(&style, None),
        )]);

        let wrapped = session.wrap_line(line, 1);
        let rendered: Vec<String> = wrapped.iter().map(line_text).collect();

        assert_eq!(rendered, vec!["你".to_string()]);
    }

    #[test]
    fn wrap_line_discards_carriage_return_before_newline() {
        let session = Session::new(InlineTheme::default(), None, VIEW_ROWS, true);
        let style = session.default_style();
        let line = Line::from(vec![Span::styled(
            "foo\r\nbar".to_string(),
            ratatui_style_from_inline(&style, None),
        )]);

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
                style: std::sync::Arc::new(InlineTextStyle::default()),
            },
        );

        let tool_lines: Vec<&MessageLine> = session
            .lines
            .iter()
            .filter(|line| line.kind == InlineMessageKind::Tool)
            .collect();

        assert_eq!(tool_lines.len(), 1);
        let Some(first_line) = tool_lines.first() else {
            panic!("Expected at least one tool line");
        };
        assert_eq!(first_line.segments.len(), 1);
        let Some(first_segment) = first_line.segments.first() else {
            panic!("Expected at least one segment");
        };
        assert_eq!(first_segment.text.as_str(), "fn demo() {}");
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
                style: std::sync::Arc::new(InlineTextStyle::default()),
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
                    style: std::sync::Arc::new(InlineTextStyle::default()),
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
            if session.scroll_manager.offset() == session.current_max_scroll_offset() {
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
            .timeline_block_title()
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();
        assert!(title.contains(ui::NAVIGATION_BLOCK_TITLE));
    }

    #[test]
    fn tool_detail_renders_with_border_and_body_style() {
        let theme = themed_inline_colors();
        let mut session = Session::new(theme, None, VIEW_ROWS, true);
        let detail_style = InlineTextStyle::default().italic();
        session.push_line(
            InlineMessageKind::Tool,
            vec![InlineSegment {
                text: "    result line".to_string(),
                style: std::sync::Arc::new(detail_style),
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
