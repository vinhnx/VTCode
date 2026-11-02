use super::*;

impl AppSession {
    pub(crate) fn set_transient_activity_signal(&mut self, signal: Arc<TransientActivitySignal>) {
        self.transient_active_signal = Some(signal);
        self.sync_transient_activity_signal();
    }

    pub(crate) fn inline_lists_visible(&self) -> bool {
        self.inline_lists_visible
    }

    pub(super) fn toggle_inline_lists_visibility(&mut self) {
        self.inline_lists_visible = !self.inline_lists_visible;
        self.core.mark_dirty();
    }

    pub(super) fn ensure_inline_lists_visible_for_trigger(&mut self) {
        if !self.inline_lists_visible {
            self.inline_lists_visible = true;
            self.core.mark_dirty();
        }
    }

    #[cfg_attr(feature = "profiling", hotpath::measure)]
    pub(super) fn update_input_triggers(&mut self) {
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

    pub(crate) fn toggle_task_panel(&mut self) {
        let target = !self.show_task_panel;
        self.set_task_panel_visible(target);
    }

    pub(super) fn visible_transient_surface(&self) -> Option<TransientSurface> {
        self.transient_host.top()
    }

    pub(super) fn visible_bottom_docked_surface(&self) -> Option<TransientSurface> {
        self.transient_host.visible_bottom_docked()
    }

    pub(super) fn history_picker_visible(&self) -> bool {
        self.history_picker_state.active && self.transient_host.is_visible(TransientSurface::HistoryPicker)
    }

    pub(crate) fn local_agents_visible(&self) -> bool {
        self.transient_host.is_visible(TransientSurface::LocalAgents)
    }

    /// Whether any background task (managed subagent, background subprocess,
    /// or retained exec session) is still running, independent of drawer
    /// visibility. Background work is asynchronous, so it feeds the loading
    /// shimmer and animation tick rate but never the turn-busy guards.
    pub(super) fn background_activity_active(&self) -> bool {
        self.local_agents_state.loading_count() > 0
    }

    pub(crate) fn file_palette_visible(&self) -> bool {
        self.file_palette_active && self.transient_host.is_visible(TransientSurface::FilePalette)
    }

    pub(super) fn agent_palette_visible(&self) -> bool {
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

    pub(super) fn modal_state_mut(&mut self) -> Option<&mut crate::tui::core_tui::session::modal::ModalState> {
        if !self.has_active_overlay() {
            return None;
        }
        self.core.modal_state_mut()
    }

    pub(super) fn wizard_overlay(&self) -> Option<&crate::tui::core_tui::session::modal::WizardModalState> {
        self.has_active_overlay().then(|| self.core.wizard_overlay()).flatten()
    }

    pub(super) fn wizard_overlay_mut(&mut self) -> Option<&mut crate::tui::core_tui::session::modal::WizardModalState> {
        if !self.has_active_overlay() {
            return None;
        }
        self.core.wizard_overlay_mut()
    }

    pub(super) fn close_overlay(&mut self) {
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
    pub(super) fn sync_theme_from_runtime(&mut self) {
        let inline_theme = theme_from_styles(&theme::active_styles());
        self.core.theme = inline_theme;
        self.core.styles.set_theme(self.core.theme.clone());
    }

    /// End a live theme preview when a list modal is cancelled.
    ///
    /// The callback owns application-specific preview state, while the
    /// runtime fallback guarantees that a stale global preview cannot survive
    /// a dismissed palette if that callback is absent or fails.
    pub(super) fn cancel_theme_preview(&mut self) {
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

    pub(super) fn diff_preview_state_mut(&mut self) -> Option<&mut DiffPreviewState> {
        if !self.transient_host.is_visible(TransientSurface::DiffPreview) {
            return None;
        }
        self.diff_preview_state.as_mut()
    }

    pub(super) fn tool_output_viewer_state(&self) -> Option<&ToolOutputViewerState> {
        self.transient_host
            .is_visible(TransientSurface::ToolOutputViewer)
            .then_some(())
            .and(self.tool_output_viewer_state.as_ref())
    }

    pub(super) fn tool_output_viewer_state_mut(&mut self) -> Option<&mut ToolOutputViewerState> {
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
}
