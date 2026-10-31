//! Overlay and chrome state: focus, full-window routes, palette, file
//! preview, and inspector/zen view-mode toggles.

use super::*;

/// Full-window route. Exactly one is active at a time; transient overlays
/// (palette, pickers, file preview, config editor, shortcut capture) layer
/// above whichever route is active.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Route {
    #[default]
    Workspace,
    Settings,
    Browser,
    Review,
}

impl DesktopView {
    pub(super) fn global_loading_active(&self) -> bool {
        vtcode_commons::ui_protocol::LoadingActivity {
            blocked: self.interaction.is_some(),
            turn: self.running,
            thinking: self.turn_awaiting_content(),
            progress: self.ui_progress_active,
            background: self.background_activity_count > 0,
        }
        .is_active()
    }

    pub(super) fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.read(cx).focus_handle(cx).focus(window, cx);
    }

    pub(super) fn activate_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_active_route(window, cx);
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Close whichever non-workspace route is active, running its teardown
    /// (state cleanup + focus restore). No-op on the workspace route.
    pub(super) fn close_active_route(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.route {
            Route::Settings => self.close_settings(window, cx),
            Route::Browser => self.close_browser(window, cx),
            Route::Review => self.close_review(window, cx),
            Route::Workspace => {}
        }
    }

    /// Persist the native zoomed state when it changes. Fullscreen is not
    /// maximized: skipping it keeps leaving fullscreen from clobbering the
    /// saved launch state.
    pub(super) fn persist_window_maximized(&mut self, window: &Window) {
        if window.is_fullscreen() {
            return;
        }
        let maximized = window.is_maximized();
        if maximized != self.preferences.window_maximized() {
            self.preferences.set_window_maximized(maximized);
            self.preferences.save();
        }
    }

    /// Recompute the day-period greeting so a window left open across a
    /// morning/afternoon/evening boundary greets correctly when reactivated.
    pub(super) fn refresh_welcome_period(&mut self, cx: &mut Context<Self>) {
        let period = welcome_period_for_hour(chrono::Local::now().hour());
        if period != self.welcome_period {
            self.welcome_period = period;
            cx.notify();
        }
    }

    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.route == Route::Settings {
            return;
        }
        self.close_active_route(window, cx);
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        self.route = Route::Settings;
        if self.settings_tab == settings::SettingsTab::Storage {
            self.maybe_request_storage_summary(cx);
        }
        // The page is a full-window route (see root_render), so opening only
        // moves focus; body-level actions stay reachable, unlike the former
        // sheet.
        self.settings_navigation_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.route == Route::Settings {
            self.close_settings(window, cx);
        } else {
            self.open_settings(window, cx);
        }
    }

    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_settings_close(window, cx);
    }

    /// Back-button close: an open typed editor closes first (unless a
    /// write is in flight), mirroring `dismiss_overlay` order so a stray
    /// click never discards an edit.
    pub(super) fn close_settings_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.config_editor.is_some() {
            if !self.config_write_pending {
                self.config_editor = None;
                self.config_editor_error = None;
                self.config_editor_return_focus = None;
                cx.notify();
            }
            return;
        }
        self.finish_settings_close(window, cx);
    }

    fn finish_settings_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.route != Route::Settings {
            return;
        }
        self.route = Route::Workspace;
        if self.palette_kind == PaletteKind::SettingsSearch && self.palette.is_some() {
            self.clear_palette_state();
            self.palette_return_focus = None;
        }
        self.config_page_open = false;
        // Capture never survives the page, staged edits or not, so reopening never shows an
        // armed pill without focus.
        self.shortcut_capture = None;
        if self.shortcuts_editing {
            self.shortcut_draft = shortcuts::draft_from(&self.preferences.shortcuts);
            self.shortcuts_editing = false;
            self.shortcut_message = None;
        }
        // See close_browser: route close always lands in the composer.
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(super) fn toggle_inspector(&mut self, cx: &mut Context<Self>) {
        if self.workspace.is_some() {
            self.inspector_open = !self.inspector_open;
            self.save_preferences();
            cx.notify();
        }
    }

    pub(super) fn toggle_zen_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.zen_mode = !self.zen_mode;
        self.navigation_peek = false;
        if self.zen_mode {
            self.activate_composer(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    /// Centered content width for the transcript and composer. Zen mode uses
    /// a narrower column so both stay on one shared spine.
    pub(super) fn content_width(&self) -> f32 {
        if self.zen_mode {
            ZEN_CONTENT_WIDTH
        } else {
            CONTENT_WIDTH
        }
    }

    pub(super) fn close_file_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview_task = None;
        self.file_preview = None;
        self.preview_editor_error = None;
        if let Some(focus) = self.preview_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    pub(super) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let return_to_settings = matches!(
            self.palette_kind,
            PaletteKind::ConfigSettings | PaletteKind::Fonts(_) | PaletteKind::SettingsSearch
        ) && self.palette.is_some();
        self.clear_palette_state();
        if let Some(focus) = self.palette_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        if return_to_settings {
            self.open_settings(window, cx);
        }
        cx.notify();
    }

    pub(super) fn clear_palette_state(&mut self) {
        self.palette = None;
        self.palette_query = gpui_kit::SharedString::default();
        self._effort_slider_subscriptions.clear();
        self.effort_slider = None;
        self.palette_groups.clear();
    }

    pub(super) fn dismiss_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.full_access_confirm.is_some() {
            self.cancel_full_access_confirm(window, cx);
        } else if self.config_editor.is_some() {
            self.close_cli_config_editor(window, cx);
        } else if self.shortcut_capture.is_some() {
            // The armed capture pill is settings-interior, like the config
            // editor: Escape cancels the capture before it may close the
            // page. (Settings is a route, so the body's DismissOverlay
            // binding wins over the pill's own Cancel action.)
            self.cancel_shortcut_capture(window, cx);
        } else if self.navigation_peek {
            self.navigation_peek = false;
            cx.notify();
        } else if self.route == Route::Settings {
            self.close_settings(window, cx);
        } else if self.palette.is_some() {
            self.close_palette(window, cx);
        } else if self.route != Route::Workspace {
            self.close_active_route(window, cx);
        } else if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        } else {
            cx.propagate();
        }
    }

    pub(super) fn open_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.route == Route::Review {
            self.close_review(window, cx);
            return;
        }
        if self.change_viewer.is_none() {
            return;
        }
        self.workspace_review = None;
        self.show_review(window, cx);
    }

    pub(super) fn show_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_active_route(window, cx);
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        self.route = Route::Review;
        self.review_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.route = Route::Workspace;
        self.workspace_review = None;
        // See close_browser: route close always lands in the composer.
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(super) fn open_thread_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.route == Route::Browser {
            self.close_browser(window, cx);
            return;
        }
        self.close_active_route(window, cx);
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        self.route = Route::Browser;
        self.browser_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.route = Route::Workspace;
        // Routes unmount the workspace, so Kit keyed focus handles captured
        // at open time go stale; like closing an editor tab, Back always
        // lands focus deterministically in the composer.
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Return focus when a state transition removes the focused review surface.
    pub(super) fn restore_focus_after_review_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.route != Route::Review && self.review_focus.contains_focused(window, cx) {
            self.focus_composer(window, cx);
        }
    }
}
