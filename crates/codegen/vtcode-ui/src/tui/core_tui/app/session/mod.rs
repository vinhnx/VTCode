use std::collections::VecDeque;

pub(super) use ratatui::crossterm::event::{
    Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, MouseEvent, MouseEventKind,
};
pub(super) use ratatui::prelude::*;
pub(super) use ratatui::widgets::Clear;
pub(super) use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::tui::config::constants::ui;
use crate::tui::core_tui::app::types::{
    CompactActivityMetadata, DiffOverlayRequest, DiffPreviewMode, DiffPreviewState, InlineCommand, InlineEvent,
    InlineMessageKind, InlineSegment, LocalAgentsTransientRequest, SlashCommandItem, TaskPanelMetadata,
    TaskPanelTransientRequest, ToolOutputId, TransientActivitySignal, TransientRequest,
};
use crate::tui::core_tui::runner::TuiSessionDriver;
use crate::tui::core_tui::session::Session as CoreSessionState;
use crate::tui::core_tui::session::action::BindingStore;
use vtcode_commons::ui_protocol::TaskItemStatus;

mod agent_palette;
/// Diff preview overlay for file changes.
pub(crate) mod diff_preview;
mod events;
/// File palette for workspace file selection.
pub mod file_palette;
/// Command history picker state and rendering.
pub mod history_picker;
mod impl_events;
mod impl_render;
mod layout;
mod local_agents;
mod palette;
/// Session rendering logic and layout composition.
pub(crate) mod render;
/// Slash command detection and suggestion UI.
pub(crate) mod slash;
/// Slash command palette widget.
pub mod slash_palette;
mod task_panel;
#[path = "transcript_review/mod.rs"]
mod tool_output_viewer;
mod transient;
/// Workspace trust state management.
pub mod trust;

use self::file_palette::FilePalette;
use self::history_picker::HistoryPickerState;
use self::local_agents::LocalAgentsState;
use self::slash_palette::SlashPalette;
use self::tool_output_viewer::ToolOutputViewerState;
use self::transient::{TransientFocusPolicy, TransientHost, TransientSurface, TransientVisibilityChange};
use crate::tui::core_tui::style::theme_from_styles;
use crate::tui::options::FullscreenInteractionSettings;
use crate::tui::ui::theme;
use agent_palette::AgentPalette;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Debug, Default)]
pub(crate) struct ToolOutputBlock {
    /// Session-local identifier used to keep a completed capture tied to its
    /// corresponding transcript entry while the overlay is refreshed.
    pub(crate) id: u64,
    /// Index of the summary line when the capture was recorded. The review
    /// layer uses this identity anchor without reverse-matching command text,
    /// which is ambiguous when the same command runs more than once.
    pub(crate) anchor_line: Option<usize>,
    /// Number of core transcript lines present when this capture was queued.
    /// PTY previews may use this boundary to find their already-rendered
    /// header without searching through earlier calls.
    pub(crate) anchor_search_start: usize,
    /// Original transcript position for a capture that has no identity anchor.
    /// `None` is retained for blocks constructed by older callers/tests, which
    /// are appended as genuine orphan content.
    pub(crate) recorded_at_line: Option<usize>,
    pub(crate) lines: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct CompactActivityEntry {
    pub(crate) line_index: usize,
    pub(crate) metadata: CompactActivityMetadata,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CompactActivityHitRegion {
    pub(crate) area: Rect,
    pub(crate) review_anchor: ToolOutputId,
}

/// App-level session that layers VT Code features on top of the core session.
pub struct AppSession {
    pub(crate) core: CoreSessionState,
    agent_palette: Option<AgentPalette>,
    agent_palette_active: bool,
    file_palette: Option<FilePalette>,
    pub(crate) file_palette_active: bool,
    inline_lists_visible: bool,
    pub(crate) slash_palette: SlashPalette,
    pub(crate) history_picker_state: HistoryPickerState,
    local_agents_state: LocalAgentsState,
    local_agents_auto_opened: bool,
    pub(crate) show_task_panel: bool,
    pub(crate) task_panel_lines: Vec<String>,
    pub(crate) task_panel_statuses: Vec<TaskItemStatus>,
    pub(crate) task_panel_current: Option<usize>,
    pub(crate) task_panel_metadata: Option<TaskPanelMetadata>,
    diff_preview_state: Option<DiffPreviewState>,
    tool_output_viewer_state: Option<ToolOutputViewerState>,
    pub(crate) tool_output_blocks: Vec<ToolOutputBlock>,
    pub(crate) tool_output_revision: u64,
    pub(crate) compact_activity_entries: Vec<CompactActivityEntry>,
    pub(crate) compact_activity_hit_regions: Vec<CompactActivityHitRegion>,
    pub(crate) diff_review_anchors: Vec<vtcode_commons::ui_protocol::DiffReviewAnchor>,
    diff_overlay_queue: VecDeque<DiffOverlayRequest>,
    transient_host: TransientHost,
    transient_active_signal: Option<Arc<TransientActivitySignal>>,
    preview_callback: Option<crate::tui::core_tui::types::PreviewCallback>,
}

pub(super) type Session = AppSession;

mod commands;
mod overlays;
mod tool_output;

use commands::to_core_command;

impl AppSession {
    /// Create a new session with explicit log visibility, theme, and slash commands.
    pub(crate) fn new_with_logs(
        theme: crate::tui::core_tui::types::InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_logs: bool,
        appearance: Option<crate::tui::core_tui::session::config::AppearanceConfig>,
        slash_commands: Vec<SlashCommandItem>,
        app_name: String,
    ) -> Self {
        let core = CoreSessionState::new_with_logs(theme, placeholder, view_rows, show_logs, appearance, app_name);

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
            task_panel_statuses: Vec::new(),
            task_panel_current: None,
            task_panel_metadata: None,
            diff_preview_state: None,
            tool_output_viewer_state: None,
            tool_output_blocks: Vec::new(),
            tool_output_revision: 0,
            compact_activity_entries: Vec::new(),
            compact_activity_hit_regions: Vec::new(),
            diff_review_anchors: Vec::new(),
            diff_overlay_queue: VecDeque::new(),
            transient_host: TransientHost::default(),
            transient_active_signal: None,
            preview_callback: None,
        }
    }

    /// Create a new session with explicit log visibility, theme, slash commands, and key bindings.
    pub(crate) fn new_with_logs_and_bindings(
        theme: crate::tui::core_tui::types::InlineTheme,
        placeholder: Option<String>,
        view_rows: u16,
        show_logs: bool,
        appearance: Option<crate::tui::core_tui::session::config::AppearanceConfig>,
        slash_commands: Vec<SlashCommandItem>,
        app_name: String,
        bindings: BindingStore,
    ) -> Self {
        let core = CoreSessionState::new_with_bindings(
            theme,
            placeholder,
            view_rows,
            show_logs,
            appearance,
            app_name,
            bindings,
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
            task_panel_statuses: Vec::new(),
            task_panel_current: None,
            task_panel_metadata: None,
            diff_preview_state: None,
            tool_output_viewer_state: None,
            tool_output_blocks: Vec::new(),
            tool_output_revision: 0,
            compact_activity_entries: Vec::new(),
            compact_activity_hit_regions: Vec::new(),
            diff_review_anchors: Vec::new(),
            diff_overlay_queue: VecDeque::new(),
            transient_host: TransientHost::default(),
            transient_active_signal: None,
            preview_callback: None,
        }
    }

    pub fn new(theme: crate::tui::core_tui::types::InlineTheme, placeholder: Option<String>, view_rows: u16) -> Self {
        Self::new_with_logs(theme, placeholder, view_rows, true, None, Vec::new(), "Agent TUI".to_string())
    }

    pub fn core(&self) -> &CoreSessionState {
        &self.core
    }

    pub fn core_mut(&mut self) -> &mut CoreSessionState {
        &mut self.core
    }

    pub(crate) fn record_diff_review(&mut self, anchor: vtcode_commons::ui_protocol::DiffReviewAnchor) {
        self.diff_review_anchors.push(anchor);
        // Bound session-local anchors so a long edit-heavy session cannot grow
        // without limit; the newest notices stay available for explicit expand.
        const MAX_DIFF_REVIEW_ANCHORS: usize = 32;
        if self.diff_review_anchors.len() > MAX_DIFF_REVIEW_ANCHORS {
            let excess = self.diff_review_anchors.len() - MAX_DIFF_REVIEW_ANCHORS;
            self.diff_review_anchors.drain(..excess);
        }
        // Anchors are activation-only; recording them does not change visible
        // transcript content, so skip mark_dirty to avoid cache invalidation
        // on every clipped preview.
    }

    /// Open full-viewport ReadonlyReview for a completed-edit expand notice.
    ///
    /// Matching order:
    /// 1. stored `notice` contained in the clicked transcript text
    /// 2. longest specific workspace `file_path` contained in that text
    /// 3. the sole stored anchor, when only one exists
    ///
    /// Refuse rather than open an arbitrary last anchor when multiple payloads
    /// are present and the notice carries no distinctive path. Takes the
    /// matching anchor out of the session list so unified content is moved,
    /// not cloned.
    pub(crate) fn open_diff_review_for_notice(&mut self, notice_text: &str) -> bool {
        if !notice_text.contains("review full diff") {
            return false;
        }

        fn is_generic_label(path: &str) -> bool {
            vtcode_commons::ui_protocol::is_generic_diff_review_path(path)
        }

        let by_notice = self
            .diff_review_anchors
            .iter()
            .enumerate()
            .rev()
            .find(|(_, anchor)| !anchor.notice.is_empty() && notice_text.contains(anchor.notice.as_str()))
            .map(|(index, _)| index);

        let mut by_path: Option<usize> = None;
        for (index, anchor) in self.diff_review_anchors.iter().enumerate() {
            if is_generic_label(anchor.file_path.as_str()) {
                continue;
            }
            if !notice_text.contains(anchor.file_path.as_str()) {
                continue;
            }
            let better = match by_path {
                None => true,
                Some(prev_index) => anchor.file_path.len() > self.diff_review_anchors[prev_index].file_path.len(),
            };
            if better {
                by_path = Some(index);
            }
        }

        let index = by_notice
            .or(by_path)
            .or_else(|| (self.diff_review_anchors.len() == 1).then_some(0));
        let Some(index) = index else {
            return false;
        };
        let anchor = self.diff_review_anchors.swap_remove(index);
        self.open_diff_review_anchor(anchor)
    }

    fn open_diff_review_anchor(&mut self, anchor: vtcode_commons::ui_protocol::DiffReviewAnchor) -> bool {
        self.show_diff_overlay(DiffOverlayRequest {
            file_path: anchor.file_path,
            before: String::new(),
            after: String::new(),
            hunks: Vec::new(),
            current_hunk: 0,
            mode: DiffPreviewMode::ReadonlyReview,
            unified: Some(anchor.unified),
        });
        true
    }

    fn close_history_picker(&mut self) {
        if !self.history_picker_state.active {
            return;
        }
        self.history_picker_state.cancel(&mut self.core.input_manager);
        self.close_transient_surface(TransientSurface::HistoryPicker);
        self.update_input_triggers();
        self.mark_dirty();
    }

    pub(crate) fn show_transient(&mut self, request: TransientRequest) {
        self.core.clear_inline_prompt_suggestion();
        match request {
            TransientRequest::Modal(request) => {
                self.core
                    .show_overlay(crate::tui::core_tui::types::OverlayRequest::Modal(request.into()));
                self.show_transient_surface(TransientSurface::FloatingOverlay);
            }
            TransientRequest::List(request) => {
                self.core
                    .show_overlay(crate::tui::core_tui::types::OverlayRequest::List(request.into()));
                self.show_transient_surface(TransientSurface::FloatingOverlay);
            }
            TransientRequest::Wizard(request) => {
                self.core
                    .show_overlay(crate::tui::core_tui::types::OverlayRequest::Wizard(request.into()));
                self.show_transient_surface(TransientSurface::FloatingOverlay);
            }
            TransientRequest::Diff(request) => {
                self.show_diff_overlay(request);
            }
            TransientRequest::FilePalette(request) => {
                self.load_file_palette(request.dir_lister, request.workspace);
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
            TransientRequest::TaskPanel(TaskPanelTransientRequest { lines, statuses, current, visible, metadata }) => {
                // Visibility-only requests (show/hide) must not mutate panel body,
                // metadata, or terminal-title progress. Content updates own those
                // fields; typed metadata is the authoritative progress source.
                if visible.is_none() {
                    // Parallel statuses must align 1:1 with lines; mismatched
                    // legacy payloads fall back to the uniform base style.
                    let aligned = statuses.len() == lines.len();
                    self.task_panel_statuses = if aligned { statuses } else { Vec::new() };
                    self.task_panel_current =
                        (aligned.then_some(current).flatten()).filter(|index| *index < lines.len());
                    self.task_panel_lines = lines;
                    self.core.mark_task_panel_content_dirty();
                    match metadata {
                        Some(metadata) => {
                            self.core.set_task_panel_progress_label(Some(format!(
                                "{}/{}",
                                metadata.completed, metadata.total
                            )));
                            let percent = if metadata.total == 0 {
                                None
                            } else {
                                let completed = metadata.completed.min(metadata.total) as u64;
                                let total = metadata.total as u64;
                                let percent = completed.saturating_mul(100) / total.max(1);
                                Some(u8::try_from(percent.min(100)).unwrap_or(100))
                            };
                            self.core
                                .program_status
                                .apply(vtcode_commons::program_status::ProgramStatusUpdate::Progress { percent });
                            self.task_panel_metadata = Some(metadata);
                        }
                        None => {
                            // Content updates without typed metadata (session
                            // clear) must not leave stale title/progress on the
                            // panel header or terminal title.
                            self.task_panel_metadata = None;
                            self.core.set_task_panel_progress_label(None);
                            self.core
                                .program_status
                                .apply(vtcode_commons::program_status::ProgramStatusUpdate::Progress { percent: None });
                        }
                    }
                }
                if let Some(visible) = visible {
                    let suppressed = visible && !self.core.appearance.should_show_task_panel();
                    if !suppressed {
                        self.set_task_panel_visible(visible);
                    }
                    // When suppressed, the lines are retained so the manual
                    // `toggle_task_panel` keybinding can reveal them later.
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

    /// Show a help modal using the ratatui-cheese Help widget.
    ///
    /// The help flag travels on the request so a queued help modal still
    /// renders as help when it activates instead of clobbering the overlay
    /// that is currently visible.
    fn show_help_modal(&mut self) {
        self.show_transient(TransientRequest::Modal(crate::tui::core_tui::app::types::ModalOverlayRequest {
            title: "Keyboard Shortcuts".to_string(),
            lines: Vec::new(),
            secure_prompt: None,
            is_help_modal: true,
        }));
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
            Some(TransientSurface::ToolOutputViewer) => self.close_tool_output_viewer(),
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

    fn open_local_agents_drawer(&mut self, auto_opened: bool) {
        self.local_agents_auto_opened = auto_opened;
        self.show_transient_surface(TransientSurface::LocalAgents);
    }

    fn close_local_agents_drawer(&mut self, clear_auto_opened: bool) {
        if clear_auto_opened {
            self.local_agents_auto_opened = false;
        }
        self.close_transient_surface(TransientSurface::LocalAgents);
    }

    fn sync_transient_focus(&mut self) {
        if self.core.activity_state.is_busy() {
            self.core.set_input_enabled(false);
            self.core.set_cursor_visible(false);
            return;
        }
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

    pub(super) fn sync_transient_activity_signal(&self) {
        let Some(signal) = self.transient_active_signal.as_ref() else {
            return;
        };
        let active = self
            .visible_transient_surface()
            .is_some_and(|surface| !matches!(surface, TransientSurface::TaskPanel));
        signal.set_active(active);
    }

    pub(super) fn apply_transient_visibility_change(&mut self, change: TransientVisibilityChange) {
        if matches!(change.previous_visible, Some(TransientSurface::FilePalette | TransientSurface::AgentPalette))
            || matches!(change.current_visible, Some(TransientSurface::FilePalette | TransientSurface::AgentPalette))
        {
            self.core.request_full_clear();
        }
        self.core
            .set_local_agents_drawer_visible(change.current_visible == Some(TransientSurface::LocalAgents));
        self.sync_transient_focus();
        self.sync_transient_activity_signal();
    }

    pub fn handle_command(&mut self, command: InlineCommand) {
        match command {
            InlineCommand::SetLocalAgents { entries } => {
                let has_delegated_entries = entries
                    .iter()
                    .any(|entry| entry.kind == crate::tui::core_tui::types::LocalAgentKind::Delegated);
                // Auto-open is for live delegated work attention. Finished rows
                // stay listed for history, so close the auto-opened window when
                // nothing delegated is still loading (not when the list empties).
                let has_live_delegated = entries.iter().any(|entry| {
                    entry.kind == crate::tui::core_tui::types::LocalAgentKind::Delegated && entry.is_loading()
                });
                let update = self.local_agents_state.set_entries(entries.clone());
                let background_count = self.local_agents_state.loading_count();
                let finished_count = self.local_agents_state.finished_count();
                self.core.set_local_agents(entries);
                self.core.set_background_activity_count(background_count);
                self.core.set_background_finished_count(finished_count);
                if update.has_new_delegated_entries && self.should_auto_open_local_agents() {
                    self.ensure_inline_lists_visible_for_trigger();
                    self.open_local_agents_drawer(true);
                } else if self.local_agents_auto_opened && !has_live_delegated {
                    self.close_local_agents_drawer(true);
                } else if !self.local_agents_visible() && !has_delegated_entries {
                    self.local_agents_auto_opened = false;
                }
            }
            InlineCommand::SetArchivedHistory { entries } => {
                let mut input_entries = Vec::new();
                let archived = entries
                    .into_iter()
                    .map(|e| {
                        input_entries.push(
                            crate::tui::core_tui::session::input_manager::InputHistoryEntry::from_content_and_timestamp(
                                e.content.clone(),
                                e.created_at,
                            ),
                        );
                        history_picker::ArchivedPrompt {
                            content: e.content,
                            created_at: e.created_at,
                            session_label: e.session_label,
                            in_workspace: e.in_workspace,
                        }
                    })
                    .collect();
                self.history_picker_state.set_archived_prompts(archived);
                self.core.input_manager.prepend_archived_history(input_entries);
                self.core.mark_dirty();
            }
            InlineCommand::SetInput(value) => {
                self.handle_core_command(crate::tui::core_tui::types::InlineCommand::SetInput(value));
                self.update_input_triggers();
            }
            InlineCommand::RestoreInputDraft(input) => {
                self.handle_core_command(crate::tui::core_tui::types::InlineCommand::RestoreInputDraft(input));
                self.update_input_triggers();
            }
            InlineCommand::ApplySuggestedPrompt(value) => {
                self.handle_core_command(crate::tui::core_tui::types::InlineCommand::ApplySuggestedPrompt(value));
                self.update_input_triggers();
            }
            InlineCommand::SetInlinePromptSuggestion { suggestion, llm_generated } => {
                self.handle_core_command(crate::tui::core_tui::types::InlineCommand::SetInlinePromptSuggestion {
                    suggestion,
                    llm_generated,
                });
                self.update_input_triggers();
            }
            InlineCommand::ClearInlinePromptSuggestion => {
                self.handle_core_command(crate::tui::core_tui::types::InlineCommand::ClearInlinePromptSuggestion);
                self.update_input_triggers();
            }
            InlineCommand::ClearInput => {
                self.handle_core_command(crate::tui::core_tui::types::InlineCommand::ClearInput);
                self.update_input_triggers();
            }
            InlineCommand::RecordToolOutput { id, lines } => {
                self.record_tool_output_block(id, lines);
            }
            InlineCommand::AppendToolOutputLine { id, kind, segments } => {
                self.append_tool_output_line(id, kind, segments);
            }
            InlineCommand::AppendCompactActivity(metadata) => {
                self.append_compact_activity(metadata);
            }
            InlineCommand::ReplaceCompactActivity(metadata) => {
                self.replace_compact_activity(metadata);
            }
            InlineCommand::CollapsePtyBlock(metadata) => {
                self.collapse_pty_block(metadata);
            }
            InlineCommand::SetKeyBindings { bindings } => {
                self.core.set_bindings(BindingStore::new(bindings));
                self.refresh_compact_activity_presentations();
            }
            InlineCommand::SetAppearance { appearance } => {
                let enabling_screen_reader = appearance.screen_reader_mode && !self.core.appearance.screen_reader_mode;
                if enabling_screen_reader && let Some(viewer) = self.tool_output_viewer_state.as_mut() {
                    viewer.set_render_mode(tool_output_viewer::TranscriptRenderMode::Raw);
                }
                self.handle_core_command(crate::tui::core_tui::types::InlineCommand::SetAppearance { appearance });
                self.refresh_compact_activity_presentations();
            }
            InlineCommand::SetFullscreenInteraction { interaction } => {
                self.handle_core_command(crate::tui::core_tui::types::InlineCommand::SetFullscreenInteraction {
                    interaction,
                });
            }
            InlineCommand::ReplaceLast { count, kind, lines, link_ranges } => {
                let remove_count = count.min(self.core.lines.len());
                let first_removed = self.core.lines.len().saturating_sub(remove_count);
                self.compact_activity_entries.retain(|entry| entry.line_index < first_removed);
                self.handle_core_command(crate::tui::core_tui::types::InlineCommand::ReplaceLast {
                    count,
                    kind,
                    lines,
                    link_ranges,
                });
            }
            InlineCommand::ClearScreen => {
                self.tool_output_blocks.clear();
                self.compact_activity_entries.clear();
                self.compact_activity_hit_regions.clear();
                self.diff_review_anchors.clear();
                self.tool_output_revision = self.tool_output_revision.wrapping_add(1);
                self.handle_core_command(crate::tui::core_tui::types::InlineCommand::ClearScreen);
            }
            InlineCommand::CloseTransient => self.close_transient(),
            InlineCommand::ShowTransient { request } => self.show_transient(*request),
            InlineCommand::RecordDiffReview(anchor) => self.record_diff_review(anchor),
            InlineCommand::FocusTranscriptReview { id } => self.open_tool_output_viewer(
                self.core.transcript_width.max(1),
                self.core.transcript_rows.max(1),
                Some(id),
            ),
            InlineCommand::UpdateFilePaletteSearch { files } => {
                if let Some(palette) = &mut self.file_palette {
                    palette.set_search_index(files);
                }
            }
            InlineCommand::SetSlashCommands { commands } => {
                self.slash_palette.set_commands(commands);
            }
            _ => {
                if let Some(core_cmd) = to_core_command(&command) {
                    self.handle_core_command(core_cmd);
                }
            }
        }
    }
}

fn normalize_tool_output_header(header: &str) -> String {
    crate::tui::core_tui::session::text_utils::strip_ansi_codes(header)
        .trim_end()
        .to_owned()
}

fn rendered_core_line_text(core: &CoreSessionState, line_index: usize) -> String {
    let Some(line) = core.lines.get(line_index) else {
        return String::new();
    };
    core.render_message_spans_for_line(line)
        .into_iter()
        .map(|span| crate::tui::core_tui::session::text_utils::strip_ansi_codes(span.content.as_ref()).into_owned())
        .collect()
}

fn folded_pty_header(core: &CoreSessionState, line_index: usize) -> String {
    let first = rendered_core_line_text(core, line_index);
    if !first.trim_start().starts_with('•') {
        return first;
    }

    let mut folded = first.trim_end().to_owned();
    let mut continuation_index = line_index + 1;
    while let Some(line) = core.lines.get(continuation_index) {
        if line.kind != InlineMessageKind::Pty {
            break;
        }
        let continuation = rendered_core_line_text(core, continuation_index);
        let Some(fragment) = continuation.trim_start().strip_prefix('│') else {
            break;
        };
        folded.push(' ');
        folded.push_str(fragment.trim());
        continuation_index += 1;
    }
    folded
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
        // Background work (managed subagents, background subprocesses, retained
        // exec sessions) animates the same shimmer phase: `core.handle_tick`
        // already ORs `background_status_shimmer_active` into its single update
        // per tick, so no separate drawer-visible pass is needed here.
        self.core.handle_tick();
    }

    fn needs_animation_tick(&self) -> bool {
        // Core covers drag, spinner, shimmer, core background, and expiries;
        // AppSession adds its own drawer-local background loading count, which
        // feeds `has_status_spinner` but lives outside `core`.
        self.core.needs_animation_tick()
            || (self.core.appearance.should_animate_progress_status() && self.background_activity_active())
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

    fn attach_program_status_terminal(&mut self) {
        self.core.program_status.attach_terminal();
    }

    fn flush_program_status(&mut self) {
        self.core.program_status.flush();
    }

    fn shutdown_program_status(&mut self) {
        self.core.program_status.shutdown();
    }

    fn clear_terminal_title(&mut self) {
        self.core.clear_terminal_title();
    }

    fn is_running_activity(&self) -> bool {
        // Turn-scoped only. Background tasks and drawer previews are
        // asynchronous, so they must not lock mode switches, block slash
        // commands, or convert plain submissions into queued input.
        self.core.is_running_activity()
    }

    fn has_status_spinner(&self) -> bool {
        // Drives the animation tick rate and the drawer row shimmer, so it
        // includes background activity even when the drawer is closed.
        self.core.has_status_spinner() || self.background_activity_active()
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

    fn set_active_pty_sessions(&mut self, sessions: Option<Arc<AtomicUsize>>) {
        self.core.active_pty_sessions = sessions;
    }

    fn set_workspace_root(&mut self, root: Option<std::path::PathBuf>) {
        self.core.set_workspace_root(root);
    }

    fn set_log_receiver(&mut self, receiver: UnboundedReceiver<crate::tui::core_tui::log::LogEntry>) {
        self.core.set_log_receiver(receiver);
    }

    fn set_fullscreen_active(&mut self, active: bool) {
        self.core.set_fullscreen_active(active);
    }

    fn set_fullscreen_interaction(&mut self, config: FullscreenInteractionSettings) {
        self.core.set_fullscreen_interaction(config);
    }

    fn set_preview_callback(&mut self, callback: Option<crate::tui::core_tui::types::PreviewCallback>) {
        self.preview_callback = callback;
    }
}
