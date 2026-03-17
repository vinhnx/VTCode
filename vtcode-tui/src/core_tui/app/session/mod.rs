use std::collections::VecDeque;

pub(super) use ratatui::crossterm::event::{
    Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind,
};
pub(super) use ratatui::prelude::*;
pub(super) use ratatui::widgets::Clear;
pub(super) use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::core_tui::app::types::{
    DiffOverlayRequest, DiffPreviewState, InlineCommand, InlineEvent, SlashCommandItem,
};
use crate::core_tui::runner::TuiSessionDriver;
use crate::core_tui::session::Session as CoreSessionState;

pub mod diff_preview;
mod events;
pub mod file_palette;
pub mod history_picker;
mod impl_events;
mod impl_render;
mod layout;
mod palette;
pub mod render;
pub mod slash;
pub mod slash_palette;
pub mod trust;

use self::file_palette::FilePalette;
use self::history_picker::HistoryPickerState;
use self::slash_palette::SlashPalette;

/// App-level session that layers VT Code features on top of the core session.
pub struct AppSession {
    pub(crate) core: CoreSessionState,
    pub(crate) file_palette: Option<FilePalette>,
    pub(crate) file_palette_active: bool,
    pub(crate) inline_lists_visible: bool,
    pub(crate) slash_palette: SlashPalette,
    pub(crate) history_picker_state: HistoryPickerState,
    pub(crate) show_task_panel: bool,
    pub(crate) task_panel_lines: Vec<String>,
    pub(crate) diff_preview_state: Option<DiffPreviewState>,
    pub(crate) diff_overlay_queue: VecDeque<DiffOverlayRequest>,
}

pub(super) type Session = AppSession;

impl AppSession {
    pub fn new_with_logs(
        theme: crate::core_tui::types::InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_logs: bool,
        appearance: Option<crate::core_tui::session::config::AppearanceConfig>,
        slash_commands: Vec<SlashCommandItem>,
        app_name: String,
    ) -> Self {
        let core = CoreSessionState::new_with_logs(
            theme,
            placeholder,
            view_rows,
            show_logs,
            appearance,
            app_name,
        );

        Self {
            core,
            file_palette: None,
            file_palette_active: false,
            inline_lists_visible: true,
            slash_palette: SlashPalette::with_commands(slash_commands),
            history_picker_state: HistoryPickerState::new(),
            show_task_panel: false,
            task_panel_lines: Vec::new(),
            diff_preview_state: None,
            diff_overlay_queue: VecDeque::new(),
        }
    }

    pub fn new(
        theme: crate::core_tui::types::InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
    ) -> Self {
        Self::new_with_logs(
            theme,
            placeholder,
            view_rows,
            true,
            None,
            Vec::new(),
            "Agent TUI".to_string(),
        )
    }

    pub fn core(&self) -> &CoreSessionState {
        &self.core
    }

    pub fn core_mut(&mut self) -> &mut CoreSessionState {
        &mut self.core
    }

    pub(crate) fn inline_lists_visible(&self) -> bool {
        self.inline_lists_visible
    }

    pub(crate) fn toggle_inline_lists_visibility(&mut self) {
        self.inline_lists_visible = !self.inline_lists_visible;
        self.core.mark_dirty();
    }

    pub(crate) fn ensure_inline_lists_visible_for_trigger(&mut self) {
        if !self.inline_lists_visible {
            self.inline_lists_visible = true;
            self.core.mark_dirty();
        }
    }

    pub(crate) fn update_input_triggers(&mut self) {
        if !self.core.input_enabled() {
            if self.file_palette_active {
                self.close_file_palette();
            }
            slash::clear_slash_suggestions(self);
            return;
        }

        self.check_file_reference_trigger();
        slash::update_slash_suggestions(self);
    }

    pub(crate) fn set_task_panel_visible(&mut self, visible: bool) {
        if self.show_task_panel != visible {
            self.show_task_panel = visible;
            self.core.mark_dirty();
        }
    }

    pub(crate) fn set_task_panel_lines(&mut self, lines: Vec<String>) {
        self.task_panel_lines = lines;
        self.core.mark_dirty();
    }

    pub(crate) fn diff_preview_state(&self) -> Option<&DiffPreviewState> {
        self.diff_preview_state.as_ref()
    }

    pub(crate) fn diff_preview_state_mut(&mut self) -> Option<&mut DiffPreviewState> {
        self.diff_preview_state.as_mut()
    }

    pub(crate) fn show_diff_overlay(&mut self, request: DiffOverlayRequest) {
        if self.diff_preview_state.is_some() {
            self.diff_overlay_queue.push_back(request);
            return;
        }

        let mut state = DiffPreviewState::new_with_mode(
            request.file_path,
            request.before,
            request.after,
            request.hunks,
            request.mode,
        );
        state.current_hunk = request.current_hunk;
        self.diff_preview_state = Some(state);
        self.core.set_input_enabled(false);
        self.core.set_cursor_visible(false);
        self.core.mark_dirty();
    }

    pub(crate) fn close_diff_overlay(&mut self) {
        if self.diff_preview_state.is_none() {
            return;
        }
        self.diff_preview_state = None;
        self.core.set_input_enabled(true);
        self.core.set_cursor_visible(true);
        if let Some(next) = self.diff_overlay_queue.pop_front() {
            self.show_diff_overlay(next);
            return;
        }
        self.core.mark_dirty();
    }

    pub fn handle_command(&mut self, command: InlineCommand) {
        match command {
            InlineCommand::SetTaskPanelVisible(visible) => {
                self.set_task_panel_visible(visible);
            }
            InlineCommand::SetTaskPanelLines(lines) => {
                self.set_task_panel_lines(lines);
            }
            InlineCommand::LoadFilePalette { files, workspace } => {
                self.load_file_palette(files, workspace);
            }
            InlineCommand::SetInput(value) => {
                self.core
                    .handle_command(crate::core_tui::types::InlineCommand::SetInput(value));
                self.update_input_triggers();
            }
            InlineCommand::ApplySuggestedPrompt(value) => {
                self.core.handle_command(
                    crate::core_tui::types::InlineCommand::ApplySuggestedPrompt(value),
                );
                self.update_input_triggers();
            }
            InlineCommand::ClearInput => {
                self.core
                    .handle_command(crate::core_tui::types::InlineCommand::ClearInput);
                self.update_input_triggers();
            }
            InlineCommand::OpenHistoryPicker => {
                events::open_history_picker(self);
            }
            InlineCommand::CloseOverlay => {
                if self.diff_preview_state.is_some() {
                    self.close_diff_overlay();
                } else {
                    self.core
                        .handle_command(crate::core_tui::types::InlineCommand::CloseOverlay);
                }
            }
            InlineCommand::ShowOverlay { request } => match *request {
                crate::core_tui::app::types::OverlayRequest::Diff(request) => {
                    self.show_diff_overlay(request);
                }
                crate::core_tui::app::types::OverlayRequest::Modal(request) => {
                    self.core
                        .show_overlay(crate::core_tui::types::OverlayRequest::Modal(
                            request.into(),
                        ));
                }
                crate::core_tui::app::types::OverlayRequest::List(request) => {
                    self.core
                        .show_overlay(crate::core_tui::types::OverlayRequest::List(request.into()));
                }
                crate::core_tui::app::types::OverlayRequest::Wizard(request) => {
                    self.core
                        .show_overlay(crate::core_tui::types::OverlayRequest::Wizard(
                            request.into(),
                        ));
                }
            },
            _ => {
                if let Some(core_cmd) = to_core_command(&command) {
                    self.core.handle_command(core_cmd);
                }
            }
        }
    }
}

impl std::ops::Deref for AppSession {
    type Target = CoreSessionState;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl std::ops::DerefMut for AppSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

fn to_core_command(command: &InlineCommand) -> Option<crate::core_tui::types::InlineCommand> {
    use crate::core_tui::types::InlineCommand as CoreCommand;

    Some(match command {
        InlineCommand::AppendLine { kind, segments } => CoreCommand::AppendLine {
            kind: *kind,
            segments: segments.clone(),
        },
        InlineCommand::AppendPastedMessage {
            kind,
            text,
            line_count,
        } => CoreCommand::AppendPastedMessage {
            kind: *kind,
            text: text.clone(),
            line_count: *line_count,
        },
        InlineCommand::Inline { kind, segment } => CoreCommand::Inline {
            kind: *kind,
            segment: segment.clone(),
        },
        InlineCommand::ReplaceLast {
            count,
            kind,
            lines,
            link_ranges,
        } => CoreCommand::ReplaceLast {
            count: *count,
            kind: *kind,
            lines: lines.clone(),
            link_ranges: link_ranges.clone(),
        },
        InlineCommand::SetPrompt { prefix, style } => CoreCommand::SetPrompt {
            prefix: prefix.clone(),
            style: style.clone(),
        },
        InlineCommand::SetPlaceholder { hint, style } => CoreCommand::SetPlaceholder {
            hint: hint.clone(),
            style: style.clone(),
        },
        InlineCommand::SetMessageLabels { agent, user } => CoreCommand::SetMessageLabels {
            agent: agent.clone(),
            user: user.clone(),
        },
        InlineCommand::SetHeaderContext { context } => CoreCommand::SetHeaderContext {
            context: context.clone(),
        },
        InlineCommand::SetInputStatus { left, right } => CoreCommand::SetInputStatus {
            left: left.clone(),
            right: right.clone(),
        },
        InlineCommand::SetTheme { theme } => CoreCommand::SetTheme {
            theme: theme.clone(),
        },
        InlineCommand::SetAppearance { appearance } => CoreCommand::SetAppearance {
            appearance: appearance.clone(),
        },
        InlineCommand::SetVimModeEnabled(enabled) => CoreCommand::SetVimModeEnabled(*enabled),
        InlineCommand::SetQueuedInputs { entries } => CoreCommand::SetQueuedInputs {
            entries: entries.clone(),
        },
        InlineCommand::SetCursorVisible(value) => CoreCommand::SetCursorVisible(*value),
        InlineCommand::SetInputEnabled(value) => CoreCommand::SetInputEnabled(*value),
        InlineCommand::SetInput(value) => CoreCommand::SetInput(value.clone()),
        InlineCommand::ApplySuggestedPrompt(value) => {
            CoreCommand::ApplySuggestedPrompt(value.clone())
        }
        InlineCommand::ClearInput => CoreCommand::ClearInput,
        InlineCommand::ForceRedraw => CoreCommand::ForceRedraw,
        InlineCommand::CloseOverlay => CoreCommand::CloseOverlay,
        InlineCommand::ClearScreen => CoreCommand::ClearScreen,
        InlineCommand::SuspendEventLoop => CoreCommand::SuspendEventLoop,
        InlineCommand::ResumeEventLoop => CoreCommand::ResumeEventLoop,
        InlineCommand::ClearInputQueue => CoreCommand::ClearInputQueue,
        InlineCommand::SetEditingMode(mode) => CoreCommand::SetEditingMode(*mode),
        InlineCommand::SetAutonomousMode(enabled) => CoreCommand::SetAutonomousMode(*enabled),
        InlineCommand::SetSkipConfirmations(skip) => CoreCommand::SetSkipConfirmations(*skip),
        InlineCommand::Shutdown => CoreCommand::Shutdown,
        InlineCommand::SetReasoningStage(stage) => CoreCommand::SetReasoningStage(stage.clone()),
        InlineCommand::SetTaskPanelVisible(_)
        | InlineCommand::SetTaskPanelLines(_)
        | InlineCommand::LoadFilePalette { .. }
        | InlineCommand::OpenHistoryPicker
        | InlineCommand::ShowOverlay { .. } => return None,
    })
}

impl TuiSessionDriver for AppSession {
    type Command = InlineCommand;
    type Event = InlineEvent;

    fn handle_command(&mut self, command: Self::Command) {
        AppSession::handle_command(self, command);
    }

    fn handle_event(
        &mut self,
        event: CrosstermEvent,
        events: &UnboundedSender<Self::Event>,
        callback: Option<&(dyn Fn(&Self::Event) + Send + Sync + 'static)>,
    ) {
        AppSession::handle_event(self, event, events, callback);
    }

    fn handle_tick(&mut self) {
        self.core.handle_tick();
    }

    fn render(&mut self, frame: &mut Frame<'_>) {
        AppSession::render(self, frame);
    }

    fn take_redraw(&mut self) -> bool {
        self.core.take_redraw()
    }

    fn use_steady_cursor(&self) -> bool {
        self.core.use_steady_cursor()
    }

    fn should_exit(&self) -> bool {
        self.core.should_exit()
    }

    fn request_exit(&mut self) {
        self.core.request_exit();
    }

    fn mark_dirty(&mut self) {
        self.core.mark_dirty();
    }

    fn update_terminal_title(&mut self) {
        self.core.update_terminal_title();
    }

    fn clear_terminal_title(&mut self) {
        self.core.clear_terminal_title();
    }

    fn is_running_activity(&self) -> bool {
        self.core.is_running_activity()
    }

    fn has_status_spinner(&self) -> bool {
        self.core.has_status_spinner()
    }

    fn thinking_spinner_active(&self) -> bool {
        self.core.thinking_spinner.is_active
    }

    fn has_active_navigation_ui(&self) -> bool {
        self.core.has_active_overlay()
            || self.diff_preview_state.is_some()
            || self.file_palette_active
            || self.history_picker_state.active
            || slash::slash_navigation_available(self)
    }

    fn apply_coalesced_scroll(&mut self, line_delta: i32, page_delta: i32) {
        self.core.apply_coalesced_scroll(line_delta, page_delta);
    }

    fn set_show_logs(&mut self, show: bool) {
        self.core.show_logs = show;
    }

    fn set_active_pty_sessions(
        &mut self,
        sessions: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    ) {
        self.core.active_pty_sessions = sessions;
    }

    fn set_workspace_root(&mut self, root: Option<std::path::PathBuf>) {
        self.core.set_workspace_root(root);
    }

    fn set_log_receiver(&mut self, receiver: UnboundedReceiver<crate::core_tui::log::LogEntry>) {
        self.core.set_log_receiver(receiver);
    }
}
