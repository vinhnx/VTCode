use super::*;

use super::super::types::{ListOverlayRequest, ModalOverlayRequest, WizardOverlayRequest};
use super::modal::{ModalListState, ModalSearchState};

impl Session {
    /// Show a simple modal dialog
    pub(crate) fn show_modal(
        &mut self,
        title: String,
        lines: Vec<String>,
        secure_prompt: Option<super::super::types::SecurePromptConfig>,
    ) {
        self.show_overlay(OverlayRequest::Modal(ModalOverlayRequest {
            title,
            lines,
            secure_prompt,
            is_help_modal: false,
        }));
    }

    /// Show a help modal using the ratatui-cheese Help widget.
    ///
    /// The help flag travels on the request so a queued help modal still
    /// renders as help when it activates instead of clobbering the overlay
    /// that is currently visible.
    pub(crate) fn show_help_modal(&mut self) {
        self.show_overlay(OverlayRequest::Modal(ModalOverlayRequest {
            title: "Keyboard Shortcuts".to_string(),
            lines: Vec::new(),
            secure_prompt: None,
            is_help_modal: true,
        }));
    }

    pub(crate) fn show_overlay(&mut self, request: OverlayRequest) {
        if self.has_active_overlay() {
            self.overlay_queue.push_back(request);
            self.mark_dirty();
            return;
        }
        self.activate_overlay(request);
    }

    pub(crate) fn has_active_overlay(&self) -> bool {
        self.active_overlay.is_some()
    }

    pub(crate) fn modal_state(&self) -> Option<&ModalState> {
        self.active_overlay.as_ref().and_then(ActiveOverlay::as_modal)
    }

    pub(crate) fn modal_state_mut(&mut self) -> Option<&mut ModalState> {
        self.active_overlay.as_mut().and_then(ActiveOverlay::as_modal_mut)
    }

    pub(crate) fn wizard_overlay(&self) -> Option<&WizardModalState> {
        self.active_overlay.as_ref().and_then(ActiveOverlay::as_wizard)
    }

    pub(crate) fn wizard_overlay_mut(&mut self) -> Option<&mut WizardModalState> {
        self.active_overlay.as_mut().and_then(ActiveOverlay::as_wizard_mut)
    }

    pub(crate) fn take_modal_state(&mut self) -> Option<ModalState> {
        if !self
            .active_overlay
            .as_ref()
            .is_some_and(|overlay| matches!(overlay, ActiveOverlay::Modal(_)))
        {
            return None;
        }

        match self.active_overlay.take() {
            Some(ActiveOverlay::Modal(state)) => Some(*state),
            Some(ActiveOverlay::Wizard(_)) | None => None,
        }
    }

    fn activate_overlay(&mut self, request: OverlayRequest) {
        match request {
            OverlayRequest::Modal(request) => {
                self.clear_last_overlay_list_cache();
                self.activate_modal_overlay(request);
            }
            OverlayRequest::List(request) => self.activate_list_overlay(request),
            OverlayRequest::Wizard(request) => {
                self.clear_last_overlay_list_cache();
                self.activate_wizard_overlay(request);
            }
        }
    }

    pub(crate) fn close_overlay(&mut self) {
        let Some(state) = self.active_overlay.take() else {
            return;
        };

        self.areas.clear_modal_areas();
        self.modal_link_targets.clear();
        self.cache_last_overlay_list_state(&state);
        self.input_enabled = state.restore_input() && !self.activity_state.is_busy();
        self.cursor_visible = state.restore_cursor() && !self.activity_state.is_busy();

        if let Some(next_request) = self.overlay_queue.pop_front() {
            self.activate_overlay(next_request);
            return;
        }

        self.mark_dirty();
    }

    fn activate_modal_overlay(&mut self, request: ModalOverlayRequest) {
        let state = ModalState {
            title: request.title,
            lines: request.lines,
            footer_hint: None,
            hotkeys: Vec::new(),
            list: None,
            search: None,
            secure_prompt: request.secure_prompt,
            is_help_modal: request.is_help_modal,
            restore_input: true,
            restore_cursor: true,
            status: None,
        };
        if state.secure_prompt.is_none() {
            self.input_enabled = false;
        }
        self.cursor_visible = false;
        self.active_overlay = Some(ActiveOverlay::Modal(Box::new(state)));
        self.mark_dirty();
    }

    fn activate_list_overlay(&mut self, request: ListOverlayRequest) {
        let anchor_to_bottom = self.should_anchor_list_to_bottom(request.selected.as_ref());
        let mut list_state = ModalListState::new(request.items, request.selected.clone());
        let search_state = request.search.map(ModalSearchState::from);
        if let Some(search) = &search_state {
            list_state.apply_search_with_preference(&search.query, request.selected, search.fuzzy);
        }
        if anchor_to_bottom {
            list_state.select_last();
        }
        self.clear_last_overlay_list_cache();
        let state = ModalState {
            title: request.title,
            lines: request.lines,
            footer_hint: request.footer_hint,
            hotkeys: request.hotkeys,
            list: Some(list_state),
            search: search_state,
            secure_prompt: None,
            restore_input: true,
            restore_cursor: true,
            is_help_modal: false,
            status: request.status,
        };
        self.input_enabled = false;
        self.cursor_visible = false;
        self.active_overlay = Some(ActiveOverlay::Modal(Box::new(state)));
        self.mark_dirty();
    }

    fn cache_last_overlay_list_state(&mut self, overlay: &ActiveOverlay) {
        if let ActiveOverlay::Modal(state) = overlay
            && let Some(list) = state.list.as_ref()
        {
            self.last_overlay_list_selection = list.current_selection();
            self.last_overlay_list_was_last = list.selected_is_last();
            return;
        }

        self.last_overlay_list_selection = None;
        self.last_overlay_list_was_last = false;
    }

    fn should_anchor_list_to_bottom(&self, preferred: Option<&InlineListSelection>) -> bool {
        self.last_overlay_list_was_last && self.last_overlay_list_selection.as_ref() == preferred
    }

    fn clear_last_overlay_list_cache(&mut self) {
        self.last_overlay_list_selection = None;
        self.last_overlay_list_was_last = false;
    }

    fn activate_wizard_overlay(&mut self, request: WizardOverlayRequest) {
        let wizard =
            WizardModalState::new(request.title, request.steps, request.current_step, request.search, request.mode);
        self.active_overlay = Some(ActiveOverlay::Wizard(Box::new(wizard)));
        self.input_enabled = false;
        self.cursor_visible = false;
        self.mark_dirty();
    }
}
