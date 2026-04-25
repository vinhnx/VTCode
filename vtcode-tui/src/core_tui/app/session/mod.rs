use std::collections::VecDeque;

pub(super) use ratatui::crossterm::event::{
    Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind,
};
pub(super) use ratatui::prelude::*;
pub(super) use ratatui::widgets::Clear;
pub(super) use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::core_tui::app::types::{
    DiffOverlayRequest, DiffPreviewState, InlineCommand, InlineEvent, LocalAgentsTransientRequest,
    SlashCommandItem, TaskPanelTransientRequest, TransientRequest,
};
use crate::core_tui::runner::TuiSessionDriver;
use crate::core_tui::session::Session as CoreSessionState;

mod agent_palette;
pub mod diff_preview;
mod events;
pub mod file_palette;
pub mod history_picker;
mod impl_events;
mod impl_render;
mod layout;
mod local_agents;
mod palette;
pub mod render;
pub mod slash;
pub mod slash_palette;
mod transcript_review;
mod transient;
pub mod trust;

use self::file_palette::FilePalette;
use self::history_picker::HistoryPickerState;
use self::local_agents::LocalAgentsState;
use self::slash_palette::SlashPalette;
use self::transcript_review::TranscriptReviewState;
use self::transient::{
    TransientFocusPolicy, TransientHost, TransientSurface, TransientVisibilityChange,
};
use crate::options::FullscreenInteractionSettings;
use agent_palette::AgentPalette;

/// App-level session that layers VT Code features on top of the core session.
pub struct AppSession {
    pub(crate) core: CoreSessionState,
    pub(crate) agent_palette: Option<AgentPalette>,
    pub(crate) agent_palette_active: bool,
    pub(crate) file_palette: Option<FilePalette>,
    pub(crate) file_palette_active: bool,
    pub(crate) inline_lists_visible: bool,
    pub(crate) slash_palette: SlashPalette,
    pub(crate) history_picker_state: HistoryPickerState,
    local_agents_state: LocalAgentsState,
    local_agents_auto_opened: bool,
    pub(crate) show_task_panel: bool,
    pub(crate) task_panel_lines: Vec<String>,
    pub(crate) diff_preview_state: Option<DiffPreviewState>,
    pub(crate) transcript_review_state: Option<TranscriptReviewState>,
    pub(crate) diff_overlay_queue: VecDeque<DiffOverlayRequest>,
    pub(crate) transient_host: TransientHost,
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
            agent_palette: None,
            agent_palette_active: false,
            file_palette: None,
            file_palette_active: false,
            inline_lists_visible: true,
            slash_palette: SlashPalette::with_commands(slash_commands),
            history_picker_state: HistoryPickerState::new(),
            local_agents_state: LocalAgentsState::default(),
            local_agents_auto_opened: false,
            show_task_panel: false,
            task_panel_lines: Vec::new(),
            diff_preview_state: None,
            transcript_review_state: None,
            diff_overlay_queue: VecDeque::new(),
            transient_host: TransientHost::default(),
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
            return;
        }

        self.check_agent_reference_trigger();
        self.check_file_reference_trigger();
        slash::update_slash_suggestions(self);
    }

    pub(super) fn show_transient_surface(&mut self, surface: TransientSurface) -> bool {
        let change = self.transient_host.show(surface);
        if !change.changed() {
            return false;
        }

        self.apply_transient_visibility_change(change);
        true
    }

    pub(super) fn close_transient_surface(&mut self, surface: TransientSurface) -> bool {
        let change = self.transient_host.hide(surface);
        if !change.changed() {
            return false;
        }

        self.apply_transient_visibility_change(change);
        true
    }

    pub(super) fn finish_history_picker_interaction(&mut self, was_active: bool) {
        if was_active && !self.history_picker_state.active {
            self.close_transient_surface(TransientSurface::HistoryPicker);
            self.update_input_triggers();
        }
    }

    pub(crate) fn set_task_panel_visible(&mut self, visible: bool) {
        if self.show_task_panel != visible {
            self.show_task_panel = visible;
            if visible {
                self.show_transient_surface(TransientSurface::TaskPanel);
            } else {
                self.close_transient_surface(TransientSurface::TaskPanel);
            }
            self.core.mark_dirty();
        }
    }

    pub(crate) fn visible_transient_surface(&self) -> Option<TransientSurface> {
        self.transient_host.top()
    }

    pub(crate) fn visible_bottom_docked_surface(&self) -> Option<TransientSurface> {
        self.transient_host.visible_bottom_docked()
    }

    pub(crate) fn history_picker_visible(&self) -> bool {
        self.history_picker_state.active
            && self
                .transient_host
                .is_visible(TransientSurface::HistoryPicker)
    }

    pub(crate) fn local_agents_visible(&self) -> bool {
        self.transient_host
            .is_visible(TransientSurface::LocalAgents)
    }

    pub(super) fn local_agents_loading_active(&self) -> bool {
        self.local_agents_visible()
            && self
                .local_agents_state
                .entries()
                .iter()
                .any(crate::core_tui::types::LocalAgentEntry::is_loading)
    }

    pub(crate) fn file_palette_visible(&self) -> bool {
        self.file_palette_active
            && self
                .transient_host
                .is_visible(TransientSurface::FilePalette)
    }

    pub(crate) fn agent_palette_visible(&self) -> bool {
        self.agent_palette_active
            && self
                .transient_host
                .is_visible(TransientSurface::AgentPalette)
    }

    pub(crate) fn slash_palette_visible(&self) -> bool {
        !self.slash_palette.is_empty()
            && self
                .transient_host
                .is_visible(TransientSurface::SlashPalette)
    }

    pub(crate) fn has_active_overlay(&self) -> bool {
        self.core.has_active_overlay()
            && self
                .transient_host
                .is_visible(TransientSurface::FloatingOverlay)
    }

    pub(crate) fn modal_state(&self) -> Option<&crate::core_tui::session::modal::ModalState> {
        self.has_active_overlay()
            .then(|| self.core.modal_state())
            .flatten()
    }

    pub(crate) fn modal_state_mut(
        &mut self,
    ) -> Option<&mut crate::core_tui::session::modal::ModalState> {
        if !self.has_active_overlay() {
            return None;
        }
        self.core.modal_state_mut()
    }

    pub(crate) fn wizard_overlay(
        &self,
    ) -> Option<&crate::core_tui::session::modal::WizardModalState> {
        self.has_active_overlay()
            .then(|| self.core.wizard_overlay())
            .flatten()
    }

    pub(crate) fn wizard_overlay_mut(
        &mut self,
    ) -> Option<&mut crate::core_tui::session::modal::WizardModalState> {
        if !self.has_active_overlay() {
            return None;
        }
        self.core.wizard_overlay_mut()
    }

    pub(crate) fn close_overlay(&mut self) {
        if !self.has_active_overlay() {
            return;
        }

        self.core.close_overlay();
        if !self.core.has_active_overlay() {
            self.close_transient_surface(TransientSurface::FloatingOverlay);
        }
    }

    pub(crate) fn diff_preview_state(&self) -> Option<&DiffPreviewState> {
        self.transient_host
            .is_visible(TransientSurface::DiffPreview)
            .then_some(())
            .and(self.diff_preview_state.as_ref())
    }

    pub(crate) fn diff_preview_state_mut(&mut self) -> Option<&mut DiffPreviewState> {
        if !self
            .transient_host
            .is_visible(TransientSurface::DiffPreview)
        {
            return None;
        }
        self.diff_preview_state.as_mut()
    }

    pub(crate) fn transcript_review_state(&self) -> Option<&TranscriptReviewState> {
        self.transient_host
            .is_visible(TransientSurface::TranscriptReview)
            .then_some(())
            .and(self.transcript_review_state.as_ref())
    }

    pub(crate) fn transcript_review_state_mut(&mut self) -> Option<&mut TranscriptReviewState> {
        if !self
            .transient_host
            .is_visible(TransientSurface::TranscriptReview)
        {
            return None;
        }
        self.transcript_review_state.as_mut()
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
        self.show_transient_surface(TransientSurface::DiffPreview);
        self.core.mark_dirty();
    }

    pub(crate) fn close_diff_overlay(&mut self) {
        if self.diff_preview_state.is_none() {
            return;
        }
        self.diff_preview_state = None;
        if let Some(next) = self.diff_overlay_queue.pop_front() {
            self.show_diff_overlay(next);
            return;
        }
        self.close_transient_surface(TransientSurface::DiffPreview);
        self.core.mark_dirty();
    }

    pub(crate) fn close_history_picker(&mut self) {
        if !self.history_picker_state.active {
            return;
        }
        self.history_picker_state
            .cancel(&mut self.core.input_manager);
        self.close_transient_surface(TransientSurface::HistoryPicker);
        self.update_input_triggers();
        self.mark_dirty();
    }

    pub(crate) fn open_transcript_review(&mut self, width: u16, height: u16) {
        self.transcript_review_state = Some(TranscriptReviewState::open(self, width, height));
        self.show_transient_surface(TransientSurface::TranscriptReview);
        self.core.mark_dirty();
    }

    pub(crate) fn close_transcript_review(&mut self) {
        if self.transcript_review_state.is_none() {
            return;
        }
        self.transcript_review_state = None;
        self.close_transient_surface(TransientSurface::TranscriptReview);
        self.core.mark_dirty();
    }

    pub(crate) fn show_transient(&mut self, request: TransientRequest) {
        self.core.clear_inline_prompt_suggestion();
        match request {
            TransientRequest::Modal(request) => {
                self.core
                    .show_overlay(crate::core_tui::types::OverlayRequest::Modal(
                        request.into(),
                    ));
                self.show_transient_surface(TransientSurface::FloatingOverlay);
            }
            TransientRequest::List(request) => {
                self.core
                    .show_overlay(crate::core_tui::types::OverlayRequest::List(request.into()));
                self.show_transient_surface(TransientSurface::FloatingOverlay);
            }
            TransientRequest::Wizard(request) => {
                self.core
                    .show_overlay(crate::core_tui::types::OverlayRequest::Wizard(
                        request.into(),
                    ));
                self.show_transient_surface(TransientSurface::FloatingOverlay);
            }
            TransientRequest::Diff(request) => {
                self.show_diff_overlay(request);
            }
            TransientRequest::FilePalette(request) => {
                self.load_file_palette(request.files, request.workspace);
                match request.visible {
                    Some(true) => {
                        self.ensure_inline_lists_visible_for_trigger();
                        self.file_palette_active = true;
                        self.show_transient_surface(TransientSurface::FilePalette);
                    }
                    Some(false) => {
                        self.close_file_palette();
                    }
                    None => {}
                }
            }
            TransientRequest::AgentPalette(request) => {
                self.load_agent_palette(request.agents);
                match request.visible {
                    Some(true) => {
                        self.ensure_inline_lists_visible_for_trigger();
                        self.agent_palette_active = true;
                        self.show_transient_surface(TransientSurface::AgentPalette);
                    }
                    Some(false) => {
                        self.close_agent_palette();
                    }
                    None => {}
                }
            }
            TransientRequest::HistoryPicker => {
                events::open_history_picker(self);
            }
            TransientRequest::SlashPalette => {
                self.ensure_inline_lists_visible_for_trigger();
                self.show_transient_surface(TransientSurface::SlashPalette);
            }
            TransientRequest::TaskPanel(TaskPanelTransientRequest { lines, visible }) => {
                self.core.set_task_panel_lines(lines.clone());
                self.task_panel_lines = lines;
                if let Some(visible) = visible {
                    self.set_task_panel_visible(visible);
                } else {
                    self.core.mark_dirty();
                }
            }
            TransientRequest::LocalAgents(LocalAgentsTransientRequest { visible }) => {
                if let Some(visible) = visible {
                    if visible {
                        self.ensure_inline_lists_visible_for_trigger();
                        self.open_local_agents_drawer(false);
                    } else {
                        self.close_local_agents_drawer(true);
                    }
                } else {
                    self.core.mark_dirty();
                }
            }
        }
        self.core.mark_dirty();
    }

    fn should_auto_open_local_agents(&self) -> bool {
        if self.has_active_overlay() {
            return false;
        }

        matches!(
            self.visible_transient_surface(),
            None | Some(TransientSurface::TaskPanel | TransientSurface::LocalAgents)
        )
    }

    pub(crate) fn close_transient(&mut self) {
        match self.visible_transient_surface() {
            Some(TransientSurface::FloatingOverlay) => self.close_overlay(),
            Some(TransientSurface::DiffPreview) => self.close_diff_overlay(),
            Some(TransientSurface::TranscriptReview) => self.close_transcript_review(),
            Some(TransientSurface::HistoryPicker) => self.close_history_picker(),
            Some(TransientSurface::AgentPalette) => self.close_agent_palette(),
            Some(TransientSurface::FilePalette) => self.close_file_palette(),
            Some(TransientSurface::SlashPalette) => slash::clear_slash_suggestions(self),
            Some(TransientSurface::TaskPanel) => self.set_task_panel_visible(false),
            Some(TransientSurface::LocalAgents) => {
                self.close_local_agents_drawer(true);
            }
            None => {}
        }
    }

    pub(super) fn open_local_agents_drawer(&mut self, auto_opened: bool) {
        self.local_agents_auto_opened = auto_opened;
        self.show_transient_surface(TransientSurface::LocalAgents);
    }

    pub(super) fn close_local_agents_drawer(&mut self, clear_auto_opened: bool) {
        if clear_auto_opened {
            self.local_agents_auto_opened = false;
        }
        self.close_transient_surface(TransientSurface::LocalAgents);
    }

    pub(crate) fn sync_transient_focus(&mut self) {
        let Some(surface) = self.visible_transient_surface() else {
            self.core.set_input_enabled(true);
            self.core.set_cursor_visible(true);
            return;
        };

        match surface.focus_policy() {
            TransientFocusPolicy::Modal | TransientFocusPolicy::CapturedInput => {
                self.core.set_input_enabled(false);
                self.core.set_cursor_visible(false);
            }
            TransientFocusPolicy::SharedInput | TransientFocusPolicy::Passive => {
                self.core.set_input_enabled(true);
                self.core.set_cursor_visible(true);
            }
        }
    }

    fn apply_transient_visibility_change(&mut self, change: TransientVisibilityChange) {
        if matches!(
            change.previous_visible,
            Some(TransientSurface::FilePalette | TransientSurface::AgentPalette)
        ) || matches!(
            change.current_visible,
            Some(TransientSurface::FilePalette | TransientSurface::AgentPalette)
        ) {
            self.core.needs_full_clear = true;
        }
        self.core.set_local_agents_drawer_visible(
            change.current_visible == Some(TransientSurface::LocalAgents),
        );
        self.sync_transient_focus();
    }

    pub fn handle_command(&mut self, command: InlineCommand) {
        match command {
            InlineCommand::SetLocalAgents { entries } => {
                let has_delegated_entries = entries
                    .iter()
                    .any(|entry| entry.kind == crate::core_tui::types::LocalAgentKind::Delegated);
                let update = self.local_agents_state.set_entries(entries.clone());
                self.core.set_local_agents(entries);
                if update.has_new_delegated_entries && self.should_auto_open_local_agents() {
                    self.ensure_inline_lists_visible_for_trigger();
                    self.open_local_agents_drawer(true);
                } else if self.local_agents_auto_opened && !has_delegated_entries {
                    self.close_local_agents_drawer(true);
                } else if !self.local_agents_visible() && !has_delegated_entries {
                    self.local_agents_auto_opened = false;
                }
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
            InlineCommand::SetInlinePromptSuggestion {
                suggestion,
                llm_generated,
            } => {
                self.core.handle_command(
                    crate::core_tui::types::InlineCommand::SetInlinePromptSuggestion {
                        suggestion,
                        llm_generated,
                    },
                );
                self.update_input_triggers();
            }
            InlineCommand::ClearInlinePromptSuggestion => {
                self.core.handle_command(
                    crate::core_tui::types::InlineCommand::ClearInlinePromptSuggestion,
                );
                self.update_input_triggers();
            }
            InlineCommand::ClearInput => {
                self.core
                    .handle_command(crate::core_tui::types::InlineCommand::ClearInput);
                self.update_input_triggers();
            }
            InlineCommand::CloseTransient => self.close_transient(),
            InlineCommand::ShowTransient { request } => self.show_transient(*request),
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
        InlineCommand::SetTerminalTitleItems { items } => CoreCommand::SetTerminalTitleItems {
            items: items.clone(),
        },
        InlineCommand::SetTerminalTitleThreadLabel { label } => {
            CoreCommand::SetTerminalTitleThreadLabel {
                label: label.clone(),
            }
        }
        InlineCommand::SetTerminalTitleGitBranch { branch } => {
            CoreCommand::SetTerminalTitleGitBranch {
                branch: branch.clone(),
            }
        }
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
        InlineCommand::SetSubprocessEntries { entries } => CoreCommand::SetSubprocessEntries {
            entries: entries.clone(),
        },
        InlineCommand::SetSubagentPreview { text } => {
            CoreCommand::SetSubagentPreview { text: text.clone() }
        }
        InlineCommand::SetLocalAgents { .. } => return None,
        InlineCommand::SetCursorVisible(value) => CoreCommand::SetCursorVisible(*value),
        InlineCommand::SetInputEnabled(value) => CoreCommand::SetInputEnabled(*value),
        InlineCommand::SetInput(value) => CoreCommand::SetInput(value.clone()),
        InlineCommand::ApplySuggestedPrompt(value) => {
            CoreCommand::ApplySuggestedPrompt(value.clone())
        }
        InlineCommand::SetInlinePromptSuggestion {
            suggestion,
            llm_generated,
        } => CoreCommand::SetInlinePromptSuggestion {
            suggestion: suggestion.clone(),
            llm_generated: *llm_generated,
        },
        InlineCommand::ClearInlinePromptSuggestion => CoreCommand::ClearInlinePromptSuggestion,
        InlineCommand::ClearInput => CoreCommand::ClearInput,
        InlineCommand::ForceRedraw => CoreCommand::ForceRedraw,
        InlineCommand::ClearScreen => CoreCommand::ClearScreen,
        InlineCommand::SuspendEventLoop => CoreCommand::SuspendEventLoop,
        InlineCommand::ResumeEventLoop => CoreCommand::ResumeEventLoop,
        InlineCommand::ClearInputQueue => CoreCommand::ClearInputQueue,
        InlineCommand::SetEditingMode(mode) => CoreCommand::SetEditingMode(*mode),
        InlineCommand::SetAutonomousMode(enabled) => CoreCommand::SetAutonomousMode(*enabled),
        InlineCommand::SetSkipConfirmations(skip) => CoreCommand::SetSkipConfirmations(*skip),
        InlineCommand::Shutdown => CoreCommand::Shutdown,
        InlineCommand::SetReasoningStage(stage) => CoreCommand::SetReasoningStage(stage.clone()),
        InlineCommand::ShowTransient { .. } | InlineCommand::CloseTransient => return None,
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
        if self.local_agents_loading_active()
            && self.core.appearance.should_animate_progress_status()
            && !self.core.is_shimmer_active()
            && self.core.shimmer_state.update()
        {
            self.core.mark_dirty();
        }
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

    fn is_hovering_link(&self) -> bool {
        self.core.is_hovering_link()
    }

    fn is_selecting_text(&self) -> bool {
        self.core.is_selecting_text()
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
        self.core.is_running_activity() || self.local_agents_loading_active()
    }

    fn has_status_spinner(&self) -> bool {
        self.core.has_status_spinner() || self.local_agents_loading_active()
    }

    fn thinking_spinner_active(&self) -> bool {
        self.core.thinking_spinner.is_active
    }

    fn has_active_navigation_ui(&self) -> bool {
        self.transient_host.has_active_navigation_surface()
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

    fn set_fullscreen_active(&mut self, active: bool) {
        self.core.set_fullscreen_active(active);
    }

    fn set_fullscreen_interaction(&mut self, config: FullscreenInteractionSettings) {
        self.core.set_fullscreen_interaction(config);
    }
}
