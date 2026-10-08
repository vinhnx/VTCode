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

    pub(crate) fn set_transient_activity_signal(&mut self, signal: Arc<TransientActivitySignal>) {
        self.transient_active_signal = Some(signal);
        self.sync_transient_activity_signal();
    }

    pub(crate) fn inline_lists_visible(&self) -> bool {
        self.inline_lists_visible
    }

    fn toggle_inline_lists_visibility(&mut self) {
        self.inline_lists_visible = !self.inline_lists_visible;
        self.core.mark_dirty();
    }

    fn ensure_inline_lists_visible_for_trigger(&mut self) {
        if !self.inline_lists_visible {
            self.inline_lists_visible = true;
            self.core.mark_dirty();
        }
    }

    fn update_input_triggers(&mut self) {
        if !self.core.input_enabled() {
            return;
        }

        self.check_agent_reference_trigger();
        self.check_file_reference_trigger();
        slash::update_slash_suggestions(self);
    }

    fn show_transient_surface(&mut self, surface: TransientSurface) -> bool {
        let change = self.transient_host.show(surface);
        if !change.changed() {
            return false;
        }

        self.apply_transient_visibility_change(change);
        true
    }

    fn close_transient_surface(&mut self, surface: TransientSurface) -> bool {
        let change = self.transient_host.hide(surface);
        if !change.changed() {
            return false;
        }

        self.apply_transient_visibility_change(change);
        true
    }

    fn finish_history_picker_interaction(&mut self, was_active: bool) {
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

    pub(crate) fn toggle_task_panel(&mut self) {
        let target = !self.show_task_panel;
        self.set_task_panel_visible(target);
    }

    fn visible_transient_surface(&self) -> Option<TransientSurface> {
        self.transient_host.top()
    }

    fn visible_bottom_docked_surface(&self) -> Option<TransientSurface> {
        self.transient_host.visible_bottom_docked()
    }

    fn history_picker_visible(&self) -> bool {
        self.history_picker_state.active && self.transient_host.is_visible(TransientSurface::HistoryPicker)
    }

    pub(crate) fn local_agents_visible(&self) -> bool {
        self.transient_host.is_visible(TransientSurface::LocalAgents)
    }

    /// Whether any background task (managed subagent, background subprocess,
    /// or retained exec session) is still running, independent of drawer
    /// visibility. Background work is asynchronous, so it feeds the loading
    /// shimmer and animation tick rate but never the turn-busy guards.
    fn background_activity_active(&self) -> bool {
        self.local_agents_state.loading_count() > 0
    }

    pub(crate) fn file_palette_visible(&self) -> bool {
        self.file_palette_active && self.transient_host.is_visible(TransientSurface::FilePalette)
    }

    fn agent_palette_visible(&self) -> bool {
        self.agent_palette_active && self.transient_host.is_visible(TransientSurface::AgentPalette)
    }

    pub(crate) fn slash_palette_visible(&self) -> bool {
        !self.slash_palette.is_empty() && self.transient_host.is_visible(TransientSurface::SlashPalette)
    }

    pub(crate) fn has_active_overlay(&self) -> bool {
        self.core.has_active_overlay() && self.transient_host.is_visible(TransientSurface::FloatingOverlay)
    }

    pub(crate) fn modal_state(&self) -> Option<&crate::tui::core_tui::session::modal::ModalState> {
        self.has_active_overlay().then(|| self.core.modal_state()).flatten()
    }

    fn modal_state_mut(&mut self) -> Option<&mut crate::tui::core_tui::session::modal::ModalState> {
        if !self.has_active_overlay() {
            return None;
        }
        self.core.modal_state_mut()
    }

    fn wizard_overlay(&self) -> Option<&crate::tui::core_tui::session::modal::WizardModalState> {
        self.has_active_overlay().then(|| self.core.wizard_overlay()).flatten()
    }

    fn wizard_overlay_mut(&mut self) -> Option<&mut crate::tui::core_tui::session::modal::WizardModalState> {
        if !self.has_active_overlay() {
            return None;
        }
        self.core.wizard_overlay_mut()
    }

    fn close_overlay(&mut self) {
        if !self.has_active_overlay() {
            return;
        }

        self.core.close_overlay();
        if !self.core.has_active_overlay() {
            self.close_transient_surface(TransientSurface::FloatingOverlay);
        }
    }

    /// Apply the process-global theme styles to this session after a palette
    /// preview changes or is dismissed.
    fn sync_theme_from_runtime(&mut self) {
        let inline_theme = theme_from_styles(&theme::active_styles());
        self.core.theme = inline_theme;
        self.core.styles.set_theme(self.core.theme.clone());
    }

    /// End a live theme preview when a list modal is cancelled.
    ///
    /// The callback owns application-specific preview state, while the
    /// runtime fallback guarantees that a stale global preview cannot survive
    /// a dismissed palette if that callback is absent or fails.
    fn cancel_theme_preview(&mut self) {
        if !theme::has_preview_theme() {
            return;
        }

        if let Some(callback) = self.preview_callback.as_ref() {
            let _ = callback(None);
        }
        theme::clear_preview_theme();
        self.sync_theme_from_runtime();
    }

    pub(crate) fn diff_preview_state(&self) -> Option<&DiffPreviewState> {
        self.transient_host
            .is_visible(TransientSurface::DiffPreview)
            .then_some(())
            .and(self.diff_preview_state.as_ref())
    }

    fn diff_preview_state_mut(&mut self) -> Option<&mut DiffPreviewState> {
        if !self.transient_host.is_visible(TransientSurface::DiffPreview) {
            return None;
        }
        self.diff_preview_state.as_mut()
    }

    fn tool_output_viewer_state(&self) -> Option<&ToolOutputViewerState> {
        self.transient_host
            .is_visible(TransientSurface::ToolOutputViewer)
            .then_some(())
            .and(self.tool_output_viewer_state.as_ref())
    }

    fn tool_output_viewer_state_mut(&mut self) -> Option<&mut ToolOutputViewerState> {
        if !self.transient_host.is_visible(TransientSurface::ToolOutputViewer) {
            return None;
        }
        self.tool_output_viewer_state.as_mut()
    }

    pub(crate) fn show_diff_overlay(&mut self, request: DiffOverlayRequest) {
        if self.diff_preview_state.is_some() {
            self.diff_overlay_queue.push_back(request);
            return;
        }

        let mut state = match request.unified.as_deref() {
            Some(unified) => DiffPreviewState::from_unified(request.file_path, unified, request.mode),
            None => DiffPreviewState::new_with_mode(
                request.file_path,
                request.before,
                request.after,
                request.hunks,
                request.mode,
            ),
        };
        state.focus_hunk(request.current_hunk);
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

    fn open_tool_output_viewer(&mut self, width: u16, height: u16, review_anchor: Option<ToolOutputId>) {
        self.tool_output_viewer_state = Some(ToolOutputViewerState::open_focused(self, width, height, review_anchor));
        self.show_transient_surface(TransientSurface::ToolOutputViewer);
        self.core.mark_dirty();
    }

    pub(crate) fn record_tool_output_block(&mut self, id: ToolOutputId, mut lines: Vec<String>) {
        if lines.is_empty() {
            return;
        }
        // Bound retained capture size so long-running floods cannot grow the
        // TUI heap without limit. Keep the tail — that is what review shows.
        let max_lines = ui::TUI_TOOL_OUTPUT_CAPTURE_MAX_LINES;
        if lines.len() > max_lines {
            let drop = lines.len() - max_lines;
            lines.drain(..drop);
        }

        // A non-PTY capture is recorded after the live PTY stream has rendered
        // its command header. Restrict the fallback search to the current
        // trailing PTY block so repeated commands cannot borrow an older
        // header from the conversation.
        let recorded_at_line = self.core.lines.len();
        let mut anchor_search_start = recorded_at_line;
        while anchor_search_start > 0
            && self
                .core
                .lines
                .get(anchor_search_start - 1)
                .is_some_and(|line| line.kind == InlineMessageKind::Pty)
        {
            anchor_search_start -= 1;
        }
        let anchor_line = self.find_live_pty_anchor(lines.first(), anchor_search_start);

        self.tool_output_blocks.push(ToolOutputBlock {
            id,
            anchor_line,
            anchor_search_start,
            recorded_at_line: Some(recorded_at_line),
            lines,
        });
        self.trim_tool_output_blocks();
        self.tool_output_revision = self.tool_output_revision.wrapping_add(1);
        self.core.mark_dirty();
    }

    /// FIFO-bound capture blocks. Blocks shown in an open tool-output viewer
    /// are pinned so review does not lose content mid-read; the newest block
    /// is never dropped. Call again after the viewer closes to re-trim.
    fn trim_tool_output_blocks(&mut self) {
        let max_blocks = ui::TUI_TOOL_OUTPUT_BLOCKS_MAX;
        if self.tool_output_blocks.len() <= max_blocks {
            return;
        }
        let pinned = self
            .tool_output_viewer_state
            .as_ref()
            .map(|viewer| viewer.retained_tool_ids())
            .unwrap_or_default();
        while self.tool_output_blocks.len() > max_blocks {
            let newest = self.tool_output_blocks.len() - 1;
            let Some(pos) = self
                .tool_output_blocks
                .iter()
                .take(newest)
                .position(|block| !pinned.contains(&block.id))
            else {
                break;
            };
            self.tool_output_blocks.remove(pos);
        }
    }

    fn find_live_pty_anchor(&self, header: Option<&String>, search_start: usize) -> Option<usize> {
        let header = header.map(|header| normalize_tool_output_header(header))?;
        let mut index = self.core.lines.len();
        while index > search_start {
            let line_index = index - 1;
            let line = self.core.lines.get(line_index)?;
            if line.kind != InlineMessageKind::Pty {
                break;
            }
            if folded_pty_header(&self.core, line_index).trim_end() == header.trim_end() {
                return Some(line_index);
            }
            index = line_index;
        }
        None
    }

    fn handle_core_command(&mut self, command: crate::tui::core_tui::types::InlineCommand) {
        let evicted_before = self.core.evicted_message_count;
        self.core.handle_command(command);
        let evicted = self.core.evicted_message_count.saturating_sub(evicted_before);
        if evicted == 0 {
            return;
        }

        self.compact_activity_entries.retain_mut(|entry| {
            if entry.line_index < evicted {
                return false;
            }
            entry.line_index -= evicted;
            true
        });
        for block in &mut self.tool_output_blocks {
            block.anchor_line = block.anchor_line.filter(|&line| line >= evicted).map(|line| line - evicted);
            block.anchor_search_start = block.anchor_search_start.saturating_sub(evicted);
            block.recorded_at_line = block.recorded_at_line.map(|line| line.saturating_sub(evicted));
        }
        self.compact_activity_hit_regions.clear();
        self.tool_output_revision = self.tool_output_revision.wrapping_add(1);
    }

    fn append_tool_output_line(&mut self, id: ToolOutputId, kind: InlineMessageKind, segments: Vec<InlineSegment>) {
        self.handle_core_command(crate::tui::core_tui::types::InlineCommand::AppendLine { kind, segments });
        let line_index = self.core.lines.len().saturating_sub(1);
        // An expand notice must win over an earlier live-PTY header anchor:
        // the notice is the row the user clicks to open this capture.
        let is_expand_notice = self
            .core
            .lines
            .get(line_index)
            .is_some_and(|line| line.segments.iter().any(|segment| segment.text.contains("click to expand")));
        if let Some(block) = self.tool_output_blocks.iter_mut().find(|block| block.id == id)
            && (block.anchor_line.is_none() || is_expand_notice)
        {
            block.anchor_line = Some(line_index);
            self.tool_output_revision = self.tool_output_revision.wrapping_add(1);
            self.core.mark_dirty();
        }
    }

    fn update_tool_output_anchors(&mut self, metadata: &CompactActivityMetadata, line_index: usize) {
        let mut ids = metadata.review_anchors.clone();
        if ids.is_empty()
            && let Some(review_anchor) = metadata.review_anchor
        {
            ids.push(review_anchor);
        }
        for id in ids {
            if let Some(block) = self.tool_output_blocks.iter_mut().find(|block| block.id == id) {
                block.anchor_line = Some(line_index);
            }
        }
    }

    fn refresh_compact_activity_presentations(&mut self) {
        let entries = self.compact_activity_entries.clone();
        for entry in entries {
            let is_compact_line = self
                .core
                .lines
                .get(entry.line_index)
                .is_some_and(|line| line.kind == InlineMessageKind::Info);
            if !is_compact_line {
                continue;
            }

            let segments = tool_output_viewer::compact_activity_segments(self, &entry.metadata);
            let revision = self.core.next_revision();
            if let Some(line) = self.core.lines.get_mut(entry.line_index) {
                line.segments = segments;
                line.link_ranges.clear();
                line.revision = revision;
                self.core.mark_line_dirty(entry.line_index);
            }
        }
        self.core.invalidate_scroll_metrics();
    }

    /// Visual-only redraw (cursor, scroll, hover). Does not drop header/sidebar caches.
    pub(crate) fn mark_visual_dirty(&mut self) {
        self.core.mark_visual_dirty();
    }

    fn append_compact_activity(&mut self, metadata: CompactActivityMetadata) {
        let segments = tool_output_viewer::compact_activity_segments(self, &metadata);
        self.handle_core_command(crate::tui::core_tui::types::InlineCommand::AppendLine {
            kind: InlineMessageKind::Info,
            segments,
        });
        let line_index = self.core.lines.len().saturating_sub(1);
        self.compact_activity_entries
            .push(CompactActivityEntry { line_index, metadata: metadata.clone() });
        let max_entries = ui::TUI_COMPACT_ACTIVITY_MAX_ENTRIES;
        if self.compact_activity_entries.len() > max_entries {
            let drop = self.compact_activity_entries.len() - max_entries;
            self.compact_activity_entries.drain(..drop);
        }
        self.update_tool_output_anchors(&metadata, line_index);
    }

    fn replace_compact_activity(&mut self, metadata: CompactActivityMetadata) {
        let can_replace = self
            .compact_activity_entries
            .last()
            .is_some_and(|entry| entry.line_index + 1 == self.core.lines.len());
        if !can_replace {
            self.append_compact_activity(metadata);
            return;
        }

        let segments = tool_output_viewer::compact_activity_segments(self, &metadata);
        self.handle_core_command(crate::tui::core_tui::types::InlineCommand::ReplaceLast {
            count: 1,
            kind: InlineMessageKind::Info,
            lines: vec![segments],
            link_ranges: None,
        });
        if let Some(entry) = self.compact_activity_entries.last_mut() {
            entry.metadata = metadata.clone();
            entry.line_index = self.core.lines.len().saturating_sub(1);
        }
        let line_index = self.core.lines.len().saturating_sub(1);
        self.update_tool_output_anchors(&metadata, line_index);
    }

    fn collapse_pty_block(&mut self, metadata: CompactActivityMetadata) {
        let pty_count = self
            .core
            .lines
            .iter()
            .rev()
            .take_while(|line| line.kind == InlineMessageKind::Pty)
            .count();
        if pty_count == 0 {
            self.replace_compact_activity(metadata);
            return;
        }

        let old_first_removed = self.core.lines.len().saturating_sub(pty_count);
        let preceding_activity = self.compact_activity_entries.last().is_some_and(|entry| {
            entry.line_index + 1 == old_first_removed && entry.metadata.group_id == metadata.group_id
        });
        let remove_count = pty_count + usize::from(preceding_activity);
        let first_removed = self.core.lines.len().saturating_sub(remove_count);
        let segments = tool_output_viewer::compact_activity_segments(self, &metadata);
        self.handle_core_command(crate::tui::core_tui::types::InlineCommand::ReplaceLast {
            count: remove_count,
            kind: InlineMessageKind::Info,
            lines: vec![segments],
            link_ranges: None,
        });
        self.compact_activity_entries.retain(|entry| entry.line_index < first_removed);
        let line_index = self.core.lines.len().saturating_sub(1);
        self.compact_activity_entries
            .push(CompactActivityEntry { line_index, metadata: metadata.clone() });
        self.update_tool_output_anchors(&metadata, line_index);
    }

    pub(crate) fn compact_activity_for_line(&self, line_index: usize) -> Option<&CompactActivityMetadata> {
        self.compact_activity_entries
            .iter()
            .find(|entry| entry.line_index == line_index)
            .map(|entry| &entry.metadata)
    }

    pub(crate) fn compact_activity_review_anchor_at(&self, column: u16, row: u16) -> Option<ToolOutputId> {
        self.compact_activity_hit_regions
            .iter()
            .find(|region| region.area.contains(Position { x: column, y: row }))
            .map(|region| region.review_anchor)
    }

    fn close_tool_output_viewer(&mut self) {
        if self.tool_output_viewer_state.is_none() {
            return;
        }
        self.tool_output_viewer_state = None;
        self.close_transient_surface(TransientSurface::ToolOutputViewer);
        // Unpin and drop any over-cap blocks that were held for the viewer.
        self.trim_tool_output_blocks();
        self.core.mark_dirty();
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

    fn sync_transient_activity_signal(&self) {
        let Some(signal) = self.transient_active_signal.as_ref() else {
            return;
        };
        let active = self
            .visible_transient_surface()
            .is_some_and(|surface| !matches!(surface, TransientSurface::TaskPanel));
        signal.set_active(active);
    }

    fn apply_transient_visibility_change(&mut self, change: TransientVisibilityChange) {
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

fn to_core_command(command: &InlineCommand) -> Option<crate::tui::core_tui::types::InlineCommand> {
    use crate::tui::core_tui::types::InlineCommand as CoreCommand;

    Some(match command {
        InlineCommand::AppendLine { kind, segments } => {
            CoreCommand::AppendLine { kind: *kind, segments: segments.clone() }
        }
        InlineCommand::AppendPastedMessage { kind, text, line_count } => CoreCommand::AppendPastedMessage {
            kind: *kind,
            text: text.clone(),
            line_count: *line_count,
        },
        InlineCommand::Inline { kind, segment } => CoreCommand::Inline { kind: *kind, segment: segment.clone() },
        InlineCommand::ReplaceLast { count, kind, lines, link_ranges } => CoreCommand::ReplaceLast {
            count: *count,
            kind: *kind,
            lines: lines.clone(),
            link_ranges: link_ranges.clone(),
        },
        InlineCommand::RecordToolOutput { .. }
        | InlineCommand::FocusTranscriptReview { .. }
        | InlineCommand::AppendToolOutputLine { .. }
        | InlineCommand::AppendCompactActivity(_)
        | InlineCommand::ReplaceCompactActivity(_)
        | InlineCommand::CollapsePtyBlock(_)
        | InlineCommand::SetKeyBindings { .. } => return None,
        InlineCommand::SetPrompt { prefix, style } => {
            CoreCommand::SetPrompt { prefix: prefix.clone(), style: style.clone() }
        }
        InlineCommand::SetPlaceholder { hint, style } => {
            CoreCommand::SetPlaceholder { hint: hint.clone(), style: style.clone() }
        }
        InlineCommand::SetMessageLabels { agent, user } => {
            CoreCommand::SetMessageLabels { agent: agent.clone(), user: user.clone() }
        }
        InlineCommand::SetHeaderContext { context } => CoreCommand::SetHeaderContext { context: context.clone() },
        InlineCommand::SetInputStatus { left, right } => {
            CoreCommand::SetInputStatus { left: left.clone(), right: right.clone() }
        }
        InlineCommand::SetConfiguredInputStatus { left, right } => {
            CoreCommand::SetConfiguredInputStatus { left: left.clone(), right: right.clone() }
        }
        InlineCommand::ProgramStatus(update) => CoreCommand::ProgramStatus(*update),
        InlineCommand::SetActivityState(state) => CoreCommand::SetActivityState(*state),
        InlineCommand::UpdateProgress(update) => CoreCommand::UpdateProgress(*update),
        InlineCommand::SetTerminalTitleItems { items } => CoreCommand::SetTerminalTitleItems { items: items.clone() },
        InlineCommand::SetTerminalTitleThreadLabel { label } => {
            CoreCommand::SetTerminalTitleThreadLabel { label: label.clone() }
        }
        InlineCommand::SetTerminalTitleGitBranch { branch } => {
            CoreCommand::SetTerminalTitleGitBranch { branch: branch.clone() }
        }
        InlineCommand::SetTheme { theme } => CoreCommand::SetTheme { theme: theme.clone() },
        InlineCommand::SetColorSchemeAuto { enabled } => CoreCommand::SetColorSchemeAuto { enabled: *enabled },
        InlineCommand::SetAppearance { appearance } => CoreCommand::SetAppearance { appearance: appearance.clone() },
        InlineCommand::SetFullscreenInteraction { interaction } => {
            CoreCommand::SetFullscreenInteraction { interaction: *interaction }
        }
        InlineCommand::SetVimModeEnabled(enabled) => CoreCommand::SetVimModeEnabled(*enabled),
        InlineCommand::SetQueuedInputs { entries } => CoreCommand::SetQueuedInputs { entries: entries.clone() },
        InlineCommand::SetSubprocessEntries { entries } => {
            CoreCommand::SetSubprocessEntries { entries: entries.clone() }
        }
        InlineCommand::SetSubagentPreview { text } => CoreCommand::SetSubagentPreview { text: text.clone() },
        InlineCommand::SetLocalAgents { .. } => return None,
        InlineCommand::SetArchivedHistory { .. } => return None,
        InlineCommand::UpdateFilePaletteSearch { .. } => return None,
        InlineCommand::SetSlashCommands { .. } => return None,
        InlineCommand::SetPrimaryAgent { name, color } => {
            CoreCommand::SetPrimaryAgent { name: name.clone(), color: color.clone() }
        }
        InlineCommand::SetCursorVisible(value) => CoreCommand::SetCursorVisible(*value),
        InlineCommand::SetInputEnabled(value) => CoreCommand::SetInputEnabled(*value),
        InlineCommand::SetImageInputEnabled(value) => CoreCommand::SetImageInputEnabled(*value),
        InlineCommand::SetInput(value) => CoreCommand::SetInput(value.clone()),
        InlineCommand::RestoreInputDraft(input) => CoreCommand::RestoreInputDraft(input.clone()),
        InlineCommand::ApplySuggestedPrompt(value) => CoreCommand::ApplySuggestedPrompt(value.clone()),
        InlineCommand::SetInlinePromptSuggestion { suggestion, llm_generated } => {
            CoreCommand::SetInlinePromptSuggestion {
                suggestion: suggestion.clone(),
                llm_generated: *llm_generated,
            }
        }
        InlineCommand::ClearInlinePromptSuggestion => CoreCommand::ClearInlinePromptSuggestion,
        InlineCommand::ClearInput => CoreCommand::ClearInput,
        InlineCommand::ForceRedraw => CoreCommand::ForceRedraw,
        InlineCommand::ClearScreen => CoreCommand::ClearScreen,
        InlineCommand::SuspendEventLoop => CoreCommand::SuspendEventLoop,
        InlineCommand::ResumeEventLoop => CoreCommand::ResumeEventLoop,
        InlineCommand::ClearInputQueue => CoreCommand::ClearInputQueue,
        InlineCommand::StopEventStream => CoreCommand::StopEventStream,
        InlineCommand::StartEventStream => CoreCommand::StartEventStream,
        InlineCommand::SetSkipConfirmations(skip) => CoreCommand::SetSkipConfirmations(*skip),
        InlineCommand::Shutdown => CoreCommand::Shutdown,
        InlineCommand::SetReasoningStage(stage) => CoreCommand::SetReasoningStage(stage.clone()),
        InlineCommand::ShowTransient { .. } | InlineCommand::CloseTransient | InlineCommand::RecordDiffReview(_) => {
            return None;
        }
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
        // commands, or convert plain submissions into queued/steered input.
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
