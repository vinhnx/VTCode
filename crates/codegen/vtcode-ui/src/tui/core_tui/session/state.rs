/// State management and lifecycle operations for Session
///
/// This module handles session state including:
/// - Session initialization and configuration
/// - Exit management
/// - Redraw and dirty state tracking
/// - Screen clearing
/// - Modal management
/// - Timeline pane toggling
/// - Scroll management operations
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ratatui::layout::{Position, Rect};
use tokio::sync::mpsc::UnboundedSender;

use super::super::types::{
    ActivityState, InlineEvent, InlineListSelection, ListOverlayRequest, LocalAgentEntry, LocalAgentKind,
    ModalOverlayRequest, OverlayRequest, WizardOverlayRequest,
};
use super::mouse_selection::MouseSelectionState;
use super::reflow::is_info_box_line;
use super::status_requires_shimmer;
use super::{
    ActiveOverlay, InlinePromptSuggestionState, MouseDragTarget, Session, SuggestedPromptState,
    action::Action,
    modal::{ModalListState, ModalSearchState, ModalState, WizardModalState},
};
use crate::tui::config::constants::ui;
use crate::tui::options::FullscreenInteractionSettings;

const COPY_NOTIFICATION_DURATION: Duration = Duration::from_secs(2);
const COPY_FAILURE_NOTIFICATION_TEXT: &str = "Copy failed";
const ACTION_REQUIRED_STATUS_TEXT: &str = "Action required";
const APPROVAL_REQUIRED_STATUS_TEXT: &str = "Approval required";
const INPUT_REQUIRED_STATUS_TEXT: &str = "Input required";
const ACTIVE_PTY_STATUS_TEXT: &str = "Running PTY command...";

impl Session {
    /// Mark dirty after task-panel body content changed. Terminal-title progress
    /// is owned by typed panel metadata, not by parsing body lines.
    pub(crate) fn mark_task_panel_content_dirty(&mut self) {
        self.mark_dirty();
    }

    /// Drive terminal-title task progress from typed panel metadata (`N/M`).
    /// Panel body lines no longer carry a parseable `Progress:` row.
    pub(crate) fn set_task_panel_progress_label(&mut self, label: Option<String>) {
        self.terminal_title_task_progress = label;
        self.mark_dirty();
    }

    pub(crate) fn clear_inline_prompt_suggestion(&mut self) {
        if self.inline_prompt_suggestion.suggestion.is_none() {
            return;
        }
        self.inline_prompt_suggestion = InlinePromptSuggestionState::default();
        self.mark_dirty();
    }

    pub(crate) fn copy_input_selection_to_clipboard(&mut self) -> bool {
        let Some(selected) = self.input_manager.selected_text() else {
            return false;
        };
        let char_count = selected.chars().count();

        // Swallow the key even on hard failure so Ctrl+C never degrades into an
        // interrupt while the user is trying to copy a selection.
        if self.input_manager.copy_selected_text_to_clipboard() {
            self.show_copy_notification(char_count);
        } else {
            self.show_copy_failure_notification();
        }
        true
    }

    pub(crate) fn copy_text_to_clipboard(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let char_count = text.chars().count();
        if MouseSelectionState::copy_to_clipboard(text) {
            self.show_copy_notification(char_count);
        } else {
            self.show_copy_failure_notification();
        }
    }

    /// Dismiss any in-progress or completed mouse text selection.
    ///
    /// Returns whether a visible selection was actually cleared. Clicking any
    /// UI target (file link, overlay control, bottom panel, jump affordance)
    /// and pressing Esc while idle both route through this so the highlight
    /// never lingers on screen after the user has moved on.
    ///
    /// This is the single dismissal boundary for a mouse interaction: it also
    /// clears the drag target and the edge auto-scroll the drag armed. Leaving
    /// either set would keep scrolling the transcript (or keep routing drags to
    /// the selection path) after the highlight is gone — reachable by pressing
    /// Esc mid-drag with the button still held. `MouseSelectionState::clear`
    /// drops the click history too, so a dismissal cannot arm a stale
    /// double-click.
    pub(crate) fn clear_mouse_selection(&mut self) -> bool {
        let had_selection = self.mouse_selection.has_selection || self.mouse_selection.is_selecting;
        if !had_selection {
            return false;
        }
        self.mouse_selection.clear();
        self.mouse_drag_target = MouseDragTarget::None;
        self.cancel_drag_auto_scroll();
        self.mark_dirty();
        true
    }

    pub(crate) fn clear_suggested_prompt_state(&mut self) {
        if !self.suggested_prompt_state.active {
            return;
        }
        self.suggested_prompt_state = SuggestedPromptState::default();
        self.mark_dirty();
    }

    /// Get the next revision counter for message tracking
    pub(crate) fn next_revision(&mut self) -> u64 {
        self.line_revision_counter = self.line_revision_counter.wrapping_add(1);
        self.line_revision_counter
    }

    /// Check if the session should exit
    pub(crate) fn should_exit(&self) -> bool {
        self.should_exit
    }

    /// Request session exit
    pub(crate) fn request_exit(&mut self) {
        self.should_exit = true;
    }

    /// Take the redraw flag and reset it
    ///
    /// Returns true if a redraw was needed
    pub(crate) fn take_redraw(&mut self) -> bool {
        self.render_state.take_redraw()
    }

    /// Mark the session as needing a redraw (visual-only, no cache invalidation)
    ///
    /// Use this for changes that only affect the visual output without changing
    /// content data: cursor movement, scroll position, hover state, mouse selection.
    pub(crate) fn mark_visual_dirty(&mut self) {
        self.render_state.request_redraw();
    }

    pub(crate) fn request_full_clear(&mut self) {
        self.render_state.request_full_clear();
    }

    /// Mark the session as needing a redraw with full cache invalidation
    ///
    /// Use this for changes that affect content data: new messages, text changes,
    /// config changes, queue updates. Clears header, sidebar, and subprocess caches.
    pub(crate) fn mark_dirty(&mut self) {
        self.render_state.request_redraw();
        self.header_lines_cache = None;
        self.header_block_title_cache = None;
        self.header_height_cache.clear();
        self.input_render_cache = None;
        self.queued_inputs_preview_cache = None;
        self.subprocess_entries_preview_cache = None;
    }

    /// Mark a specific line as dirty to optimize reflow scans, without
    /// invalidating header or sidebar caches. Use this for transcript-only
    /// changes (e.g. streaming chunks) where chrome hasn't changed.
    pub(crate) fn mark_transcript_line_dirty(&mut self, index: usize) {
        let index = self.reflow_dirty_index(index);
        self.first_dirty_line = match self.first_dirty_line {
            Some(current) => Some(current.min(index)),
            None => Some(index),
        };
        self.record_transcript_change(index);
        self.render_state.request_redraw();
        self.invalidate_transcript_viewport();
    }

    /// Invalidate only the header cache (e.g. when provider/model changes)
    pub(crate) fn invalidate_header_cache(&mut self) {
        self.header_lines_cache = None;
        self.header_block_title_cache = None;
        self.header_height_cache.clear();
        self.input_render_cache = None;
        self.render_state.request_redraw();
    }

    /// Invalidate only the sidebar cache (e.g. when queue changes)
    pub(crate) fn invalidate_sidebar_cache(&mut self) {
        self.queued_inputs_preview_cache = None;
        self.subprocess_entries_preview_cache = None;
        self.render_state.request_redraw();
    }

    pub(crate) fn set_local_agents(&mut self, entries: Vec<LocalAgentEntry>) {
        if self.local_agents != entries {
            self.program_status.children(&entries);
            self.local_agents = entries;
            self.invalidate_sidebar_cache();
        }
    }

    pub(crate) fn has_local_agents(&self) -> bool {
        self.local_agents
            .iter()
            .any(|entry| matches!(entry.kind, LocalAgentKind::Delegated | LocalAgentKind::ExecSession))
    }

    pub(crate) fn set_local_agents_drawer_visible(&mut self, visible: bool) {
        if self.local_agents_drawer_visible != visible {
            self.local_agents_drawer_visible = visible;
            self.mark_dirty();
        }
    }

    pub(crate) fn set_transcript_area(&mut self, area: Option<Rect>) {
        if self.areas.transcript() != area {
            self.sticky_prompt_target = None;
        }
        self.areas.set_transcript(area);
    }

    pub(crate) fn transcript_area(&self) -> Option<Rect> {
        self.areas.transcript()
    }

    pub(crate) fn set_input_area(&mut self, area: Option<Rect>) {
        self.areas.set_input(area);
    }

    pub(crate) fn input_area(&self) -> Option<Rect> {
        self.areas.input()
    }

    pub(crate) fn set_bottom_panel_area(&mut self, area: Option<Rect>) {
        self.areas.set_bottom_panel(area);
    }

    pub(crate) fn bottom_panel_area(&self) -> Option<Rect> {
        self.areas.bottom_panel()
    }

    pub(crate) fn set_modal_list_area(&mut self, area: Option<Rect>) {
        self.areas.set_modal_list(area);
    }

    pub(crate) fn modal_list_area(&self) -> Option<Rect> {
        self.areas.modal_list()
    }

    pub(crate) fn set_modal_text_areas(&mut self, areas: Vec<Rect>) {
        self.areas.set_modal_text(areas);
    }

    pub(crate) fn modal_text_areas(&self) -> &[Rect] {
        self.areas.modal_text()
    }

    pub(crate) fn set_modal_link_targets(&mut self, targets: Vec<super::TranscriptFileLinkTarget>) {
        self.modal_link_targets = targets;
    }

    #[cfg(test)]
    pub(crate) fn modal_link_targets(&self) -> &[super::TranscriptFileLinkTarget] {
        &self.modal_link_targets
    }

    pub(crate) fn input_owner(&self) -> super::input_manager::InputOwner {
        use super::input_manager::InputOwner;
        if self.has_active_overlay() {
            InputOwner::Overlay
        } else if self.input_enabled && !self.activity_state.is_busy() {
            InputOwner::Composer
        } else {
            InputOwner::Runtime
        }
    }

    pub(crate) fn input_enabled(&self) -> bool {
        self.input_owner() == super::input_manager::InputOwner::Composer
    }

    pub(crate) fn set_input_enabled(&mut self, enabled: bool) {
        if let Some(ActiveOverlay::Modal(state)) = self.active_overlay.as_mut() {
            state.restore_input = enabled;
        }
        self.input_enabled = enabled && !self.has_active_overlay();
        self.input_render_cache = None;
    }

    pub(crate) fn image_input_enabled(&self) -> bool {
        self.image_input_enabled
    }

    pub(crate) fn input_compact_mode(&self) -> bool {
        self.input_compact_mode
    }

    pub(crate) fn set_input_compact_mode(&mut self, enabled: bool) {
        self.input_compact_mode = enabled;
        self.input_render_cache = None;
    }

    pub(crate) fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
    }

    pub(crate) fn current_transcript_revision(&self) -> u64 {
        self.line_revision_counter
    }

    pub(crate) fn invalidate_transcript_viewport(&mut self) {
        self.visible_lines_cache = None;
        self.sticky_prompt_target = None;
    }

    pub(crate) fn request_transcript_clear(&mut self) {
        self.transcript_clear_required = true;
    }

    pub(crate) fn set_fullscreen_active(&mut self, active: bool) {
        self.fullscreen.active = active;
    }

    pub(crate) fn set_fullscreen_interaction(&mut self, config: FullscreenInteractionSettings) {
        self.fullscreen.interaction = config;
    }

    /// Advance animation state on tick and request redraw when a frame changes.
    pub(crate) fn handle_tick(&mut self) {
        let animate_progress = self.appearance.should_animate_progress_status();
        self.step_drag_auto_scroll();
        let mut animation_updated = self.progress.tick();
        if animate_progress && self.thinking_spinner.is_active && self.thinking_spinner.update() {
            animation_updated = true;
            // Refresh collapsed thinking summaries so the live spinner frame and
            // line count stay current during streaming instead of freezing until
            // a click. `mark_thinking_run_starts_dirty` bumps the run-start
            // revision and drops the visible-window cache; the redraw is
            // requested below via `animation_updated`.
            self.mark_thinking_run_starts_dirty();
        }
        let shimmer_active = animate_progress
            && (self.is_shimmer_active() || self.background_status_shimmer_active() || self.progress.is_animated());
        if shimmer_active && self.shimmer_state.update() {
            animation_updated = true;
        }
        if let Some(until) = self.scroll_cursor_steady_until
            && Instant::now() >= until
        {
            self.scroll_cursor_steady_until = None;
            self.render_state.request_redraw();
        }
        if let Some(until) = self.copy_notification_until
            && Instant::now() >= until
        {
            self.copy_notification_until = None;
            self.copy_notification_chars = 0;
            self.render_state.request_redraw();
        }
        if self.last_shimmer_active && !shimmer_active {
            self.render_state.request_redraw();
        }
        self.last_shimmer_active = shimmer_active;
        if animation_updated {
            self.render_state.request_redraw();
        }
    }

    /// Whether the event loop must keep ticking at the active rate.
    ///
    /// Event-driven rendering already draws input, commands (new messages),
    /// and crossterm events immediately without waiting for a tick; ticks are
    /// only needed while something animates or a transient deadline is armed.
    /// The drive loop calls this every turn to extend active-mode ticking
    /// independently of recent user input, so a truly idle session (no input,
    /// no animation, no pending expiry) can rest at the low upkeep rate.
    pub(crate) fn needs_animation_tick(&self) -> bool {
        if self.progress.is_animated() && self.appearance.should_animate_progress_status() {
            return true;
        }
        // Drag auto-scroll must keep stepping while the pointer rests at an edge.
        if self.drag_auto_scroll.is_some() {
            return true;
        }
        // Pending expiries need a future tick to clear promptly (scroll-steady
        // 250ms, copy notification 2s).
        if self.scroll_cursor_steady_until.is_some() || self.copy_notification_until.is_some() {
            return true;
        }
        // Shimmer deactivation needs one final tick to repaint without shimmer.
        if self.last_shimmer_active {
            return true;
        }
        if !self.appearance.should_animate_progress_status() {
            return false;
        }
        self.thinking_spinner.is_active || self.is_shimmer_active() || self.background_status_shimmer_active()
    }

    pub(crate) fn show_copy_notification(&mut self, char_count: usize) {
        self.copy_notification_chars = char_count;
        self.show_copy_result_notification(false);
    }

    pub(crate) fn show_copy_failure_notification(&mut self) {
        self.show_copy_result_notification(true);
    }

    fn show_copy_result_notification(&mut self, failed: bool) {
        self.copy_notification_until = Some(Instant::now() + COPY_NOTIFICATION_DURATION);
        self.copy_notification_failed = failed;
        self.render_state.request_redraw();
    }

    pub(crate) fn format_copy_notification(char_count: usize) -> String {
        let unit = if char_count == 1 { "char" } else { "chars" };
        format!("copied {char_count} {unit} to clipboard")
    }

    pub(crate) fn copy_notification_text(&self) -> Option<String> {
        self.copy_notification_until.filter(|until| Instant::now() < *until)?;
        if self.copy_notification_failed {
            Some(COPY_FAILURE_NOTIFICATION_TEXT.to_string())
        } else {
            Some(Self::format_copy_notification(self.copy_notification_chars))
        }
    }

    fn overlay_attention_status_text(&self) -> Option<&'static str> {
        if let Some(modal) = self.modal_state() {
            let normalized_title = modal.title.trim().to_ascii_lowercase();

            if normalized_title.contains("input required") {
                Some(INPUT_REQUIRED_STATUS_TEXT)
            } else if normalized_title.contains("approval") || normalized_title.contains("permission") {
                Some(APPROVAL_REQUIRED_STATUS_TEXT)
            } else {
                Some(ACTION_REQUIRED_STATUS_TEXT)
            }
        } else if self.wizard_overlay().is_some() || self.has_active_overlay() {
            Some(ACTION_REQUIRED_STATUS_TEXT)
        } else {
            None
        }
    }

    pub(crate) fn status_left_text(&self) -> Option<&str> {
        let active_pty_status = self.active_pty_status_text();

        // During stage states (Planning/Building), show the live activity status
        // (e.g. "Running grep...") when the agent is actively working so the
        // footer reflects what is happening. Fall back to the stage label
        // ("Planning...") when idle between turns.
        if self.activity_state.is_stage() {
            if let Some(left) = self.input_status_left.as_deref() {
                let trimmed = left.trim();
                if !trimmed.is_empty() && status_requires_shimmer(trimmed) {
                    return Some(trimmed);
                }
            }
            if let Some(status) = active_pty_status {
                return Some(status);
            }
            return self.activity_state.status();
        }

        // Compact PTY rendering intentionally keeps the live output row hidden.
        // Keep the global PTY observer visible in the footer in that case, while
        // preserving a more specific loading/approval status when one exists.
        if self.activity_state.status().is_none()
            && active_pty_status.is_some()
            && !self.input_status_left.as_deref().is_some_and(status_requires_shimmer)
        {
            return active_pty_status;
        }

        self.activity_state
            .status()
            .or(self.input_status_left.as_deref())
            .map(str::trim)
            .filter(|value: &&str| !value.is_empty())
            .or_else(|| self.overlay_attention_status_text())
            .or(active_pty_status)
    }

    pub(crate) fn status_right_text(&self) -> Option<&str> {
        self.input_status_right
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    /// Status text used to decide whether the footer should animate.
    ///
    /// During a stage state (Planning/Building) the displayed text may be the
    /// stage label when idle, so the raw input status (e.g. "Running tool:
    /// ...") is inspected directly so the spinner keeps animating while tools
    /// execute even before the displayed text has been updated.
    fn animation_status_text(&self) -> &str {
        if self.activity_state.is_stage() {
            if let Some(left) = self.input_status_left.as_deref()
                && !left.trim().is_empty()
                && status_requires_shimmer(left)
            {
                return left;
            }
            self.active_pty_status_text().unwrap_or("")
        } else {
            self.status_left_text().unwrap_or("")
        }
    }

    pub(crate) fn is_running_activity(&self) -> bool {
        if self.activity_state.is_busy() {
            return true;
        }
        // Blocked keeps a persistent status label containing "blocked", but
        // it is waiting for user input rather than running a turn.
        if matches!(self.activity_state, ActivityState::Blocked) {
            return false;
        }
        let status = self.animation_status_text();
        self.appearance.should_animate_progress_status()
            && status != ACTIVE_PTY_STATUS_TEXT
            && status_requires_shimmer(status)
    }

    pub(crate) fn active_pty_session_count(&self) -> usize {
        self.active_pty_sessions
            .as_ref()
            .map(|counter| counter.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    pub(crate) fn has_active_foreground_pty(&self) -> bool {
        self.active_pty_session_count() > 0
    }

    /// Record the number of live background tasks from the latest local-agents
    /// refresh. Background work is asynchronous: it drives the global loading
    /// shimmer but must never look like an in-flight turn, so this is kept out
    /// of [`Self::is_running_activity`].
    pub(crate) fn set_background_activity_count(&mut self, count: usize) {
        if self.background_activity_count != count {
            self.background_activity_count = count;
            self.mark_dirty();
        }
    }

    /// Retained finished agents/processes (completed, failed, stopped, exited).
    pub(crate) fn set_background_finished_count(&mut self, count: usize) {
        if self.background_finished_count != count {
            self.background_finished_count = count;
            self.mark_dirty();
        }
    }

    pub(crate) fn has_background_activity(&self) -> bool {
        self.background_activity_count > 0
    }

    pub(crate) fn has_background_history(&self) -> bool {
        self.background_finished_count > 0
    }

    pub(crate) fn set_background_indicator_hits(&mut self, hits: Vec<Rect>) {
        if self.background_indicator_hits != hits {
            self.background_indicator_hits = hits;
            self.mark_dirty();
        }
    }

    pub(crate) fn background_indicator_hits(&self) -> &[Rect] {
        &self.background_indicator_hits
    }

    pub(crate) fn background_indicator_contains(&self, column: u16, row: u16) -> bool {
        let pos = Position { x: column, y: row };
        self.background_indicator_hits.iter().any(|area| area.contains(pos))
    }

    /// Input-status indicator while background tasks run. The wording contains
    /// a shimmer needle (`running `) so [`status_requires_shimmer`] animates it
    /// through the shared loading path. When nothing is live but finished
    /// history remains, show a finished summary instead so the indicator stays
    /// openable.
    ///
    /// The bottom-line copy is idle-only: while `progress` owns the transcript
    /// loading row the count moves to that row (plus a static header badge),
    /// so the composer line never blinks as turn phases toggle underneath.
    pub(crate) fn background_activity_status_text(&self) -> Option<String> {
        if self.has_background_activity() {
            return Some(format!(
                "Running {} background task{}...",
                self.background_activity_count,
                if self.background_activity_count == 1 { "" } else { "s" }
            ));
        }
        self.has_background_history().then(|| {
            format!(
                "{} agent{} finished",
                self.background_finished_count,
                if self.background_finished_count == 1 { "" } else { "s" }
            )
        })
    }

    /// Short static header badge for background work (`• N bg` live,
    /// `✓ N done` retained). Intentionally free of shimmer needles so the
    /// header never animates or reflows while a long-lived task runs.
    pub(crate) fn background_header_badge_text(&self) -> Option<String> {
        if self.has_background_activity() {
            return Some(format!("• {} bg", self.background_activity_count));
        }
        self.has_background_history()
            .then(|| format!("✓ {} done", self.background_finished_count))
    }

    pub(crate) fn background_shortcut_label(&self) -> &str {
        self.primary_binding_label(Action::BackgroundOperation).unwrap_or("Ctrl+B")
    }

    pub(crate) fn foreground_pty_background_hint(&self) -> Option<String> {
        // The shared foreground counter covers both PTY and pipe exec sessions
        // (see `ExecSessionManager::insert_session`); the name stays `pty` for
        // surgical compatibility with existing callers.
        self.has_active_foreground_pty()
            .then(|| format!("{} background", self.background_shortcut_label()))
    }

    fn active_pty_status_text(&self) -> Option<&'static str> {
        self.has_active_foreground_pty().then_some(ACTIVE_PTY_STATUS_TEXT)
    }

    /// Semantic loading indicator derived from the status text and activity
    /// state, without the animation policy. Animation-side gating belongs to
    /// the appearance settings; loading classification must not depend on
    /// accessibility settings. A blocked turn is quiescent and commandable,
    /// so the persistent blocked label never looks like loading.
    fn status_indicator_active(&self) -> bool {
        self.activity_state.is_busy()
            || (!matches!(self.activity_state, ActivityState::Blocked)
                && status_requires_shimmer(self.animation_status_text()))
    }

    pub(crate) fn has_status_spinner(&self) -> bool {
        self.appearance.should_animate_progress_status() && self.status_indicator_active()
    }

    pub(crate) fn is_shimmer_active(&self) -> bool {
        self.has_status_spinner() || self.thinking_spinner.is_active
    }

    /// Whether any canonical global loading observer is live: turn status
    /// activity, thinking indicator, an animated transcript progress row, or
    /// background work. Deliberately animation-independent — accessibility
    /// policy only suppresses the animation frame, never the fact that work
    /// is in flight. This is the shared clock the terminal title spinner and
    /// the footer shimmer both follow; seeding a new animated surface here
    /// keeps them from drifting.
    pub(crate) fn loading_activity_active(&self) -> bool {
        vtcode_commons::ui_protocol::LoadingActivity {
            blocked: matches!(self.activity_state, ActivityState::Blocked),
            turn: self.status_indicator_active(),
            thinking: self.thinking_spinner.is_active,
            progress: self.progress.is_animated(),
            background: self.background_status_shimmer_active(),
        }
        .is_active()
    }

    /// Terminal-title attention signal: only real user attention (blocked
    /// turn, approval/permission/input-required modal, or wizard) qualifies.
    /// Ordinary overlays such as config pickers and setup modals stay out so
    /// configuring the title never flashes "action required" at itself.
    pub(crate) fn title_attention_required(&self) -> bool {
        if matches!(self.activity_state, ActivityState::Blocked) {
            return true;
        }
        if let Some(modal) = self.modal_state() {
            // Typed HITL selections are authoritative; the title-wording
            // needles below keep word-only legacy prompts working.
            if modal.attention_required() {
                return true;
            }
            let normalized_title = modal.title.trim().to_ascii_lowercase();
            if normalized_title.contains("approval")
                || normalized_title.contains("permission")
                || normalized_title.contains("input required")
            {
                return true;
            }
        }
        self.wizard_overlay().is_some()
    }

    /// Whether live background tasks must keep the loading shimmer animating.
    ///
    /// Kept out of [`Self::is_shimmer_active`] so a long-lived background task
    /// does not pin the cursor to steady mode; the tick handler ORs this with
    /// the turn shimmer instead, and short-circuiting avoids updating the
    /// shared shimmer phase twice in one tick. Suppressed while `progress`
    /// owns the transcript loading row: that row already drives the tick when
    /// animated, and its `· N bg` suffix plus the header badge are static.
    pub(crate) fn background_status_shimmer_active(&self) -> bool {
        self.has_background_activity() && !self.progress.is_active()
    }

    pub(crate) fn use_steady_cursor(&self) -> bool {
        if !self.appearance.should_animate_progress_status() {
            self.scroll_cursor_steady_until.is_some()
        } else {
            self.is_shimmer_active() || self.scroll_cursor_steady_until.is_some()
        }
    }

    pub(crate) fn mark_scrolling(&mut self) {
        let steady_duration = Duration::from_millis(ui::TUI_SCROLL_CURSOR_STEADY_MS);
        if steady_duration.is_zero() {
            self.scroll_cursor_steady_until = None;
        } else {
            self.scroll_cursor_steady_until = Some(Instant::now() + steady_duration);
        }
    }

    /// Mark a specific line as dirty to optimize reflow scans
    pub(crate) fn mark_line_dirty(&mut self, index: usize) {
        self.sticky_prompt_target = None;
        let index = self.reflow_dirty_index(index);
        self.first_dirty_line = match self.first_dirty_line {
            Some(current) => Some(current.min(index)),
            None => Some(index),
        };
        self.record_transcript_change(index);
        self.mark_dirty();
    }

    /// Record a transcript modification for Jump to last change.
    ///
    /// `line_idx` is the post-reflow logical line index. Minimal tracking:
    /// only the most recent index is kept; streaming chunks to the same line
    /// simply overwrite it.
    pub(crate) fn record_transcript_change(&mut self, line_idx: usize) {
        self.last_change_line_idx = Some(line_idx);
        self.clamp_tracked_change();
    }

    /// Shift the tracked change index after front-eviction. Drops the target
    /// when it pointed into the evicted prefix.
    pub(crate) fn shift_tracked_change_after_eviction(&mut self, remove_count: usize) {
        if remove_count == 0 {
            return;
        }
        if let Some(idx) = self.last_change_line_idx {
            self.last_change_line_idx = idx.checked_sub(remove_count);
        }
        self.clamp_tracked_change();
    }

    /// Keep the tracked index inside the live line range, clearing it when the
    /// transcript is empty. Single source of truth for every tracking writer.
    fn clamp_tracked_change(&mut self) {
        let len = self.lines.len();
        self.last_change_line_idx = match self.last_change_line_idx {
            Some(_) if len == 0 => None,
            Some(idx) if idx >= len => Some(len - 1),
            other => other,
        };
    }

    fn reflow_dirty_index(&mut self, index: usize) -> usize {
        // Info, warning, and error messages are rendered as one contiguous
        // block. When a later line is appended, reflow must start at the
        // block head so the cached head can see the new lines.
        let index = self.grouped_message_start(index);
        if let Some(cache) = self.transcript_cache.as_mut() {
            cache.invalidate_message(index);
        }
        index
    }

    fn grouped_message_start(&self, index: usize) -> usize {
        let Some(line) = self.lines.get(index) else {
            return index;
        };

        if !is_info_box_line(line) {
            return index;
        }

        let kind = line.kind;
        let mut start = index;
        while start > 0 && self.lines[start - 1].kind == kind && is_info_box_line(&self.lines[start - 1]) {
            start -= 1;
        }
        start
    }

    /// Ensure the prompt style has a color set
    pub(crate) fn ensure_prompt_style_color(&mut self) {
        if self.prompt_style.color.is_none() {
            self.prompt_style.color = self.theme.primary.or(self.theme.foreground);
        }
    }

    /// Clear the screen and reset scroll
    pub(crate) fn clear_screen(&mut self) {
        self.lines.clear();
        self.leading_user_prompt_truncated = false;
        self.collapsed_pastes.clear();
        self.thinking_runs.clear();
        self.user_scrolled = false;
        self.pending_new_messages = 0;
        self.last_change_line_idx = None;
        self.scroll_manager.set_offset(0);
        self.invalidate_transcript_cache();
        self.invalidate_scroll_metrics();
        self.render_state.request_full_clear();
        self.mark_dirty();
    }

    /// Toggle logs panel visibility
    pub(crate) fn toggle_logs(&mut self) {
        self.show_logs = !self.show_logs;
        self.invalidate_scroll_metrics();
        self.mark_dirty();
    }
}
