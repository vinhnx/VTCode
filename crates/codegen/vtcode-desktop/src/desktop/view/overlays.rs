//! Overlay and chrome state: focus, settings sheet, palette, file preview,
//! review surface, and inspector/zen view-mode toggles.

use super::*;

impl DesktopView {
    pub(super) fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.read(cx).focus_handle(cx).focus(window, cx);
    }

    pub(super) fn activate_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review_open {
            self.close_review(window, cx);
        }
        if self.settings_open {
            self.close_settings(window, cx);
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        self.focus_composer(window, cx);
        cx.notify();
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
        if self.settings_open {
            return;
        }
        if self.review_open {
            self.close_review(window, cx);
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        if !self.settings_open {
            self.settings_return_focus = window.focused(cx);
        }
        self.settings_open = true;
        let settings_view = cx.entity().clone();
        let close_view = settings_view.clone();
        let initial_focus = std::cell::Cell::new(true);
        let settings_navigation_focus = self.settings_navigation_focus.clone();
        window.open_sheet_at(gpui_kit::component::Placement::Right, cx, move |sheet, window, cx| {
            if initial_focus.replace(false) {
                settings_navigation_focus.focus(window, cx);
            }
            // The Kit builder runs again as the window resizes; keep the sheet
            // within the current viewport instead of retaining its opening width.
            let is_config_page = settings_view.read(cx).config_page_open;
            let max_width = if is_config_page { px(1200.) } else { px(860.) };
            let sheet_width = (window.viewport_size().width.min(max_width) - px(24.)).max(px(0.));
            let content = settings_view.update(cx, |view, cx| {
                if view.config_page_open {
                    view.render_cli_config_page(window, cx)
                } else {
                    view.render_settings_tabs(window, cx)
                }
            });
            let on_close_view = close_view.clone();
            let sheet_title = if is_config_page {
                rust_i18n::t!("app.palette.title.configuration").to_string()
            } else {
                rust_i18n::t!("app.settings.title").to_string()
            };
            sheet
                .title(
                    DialogTitle::new().text_lg().child(
                        div()
                            .id("settings-sheet-heading")
                            .test_support()
                            .role(gpui_kit::Role::Label)
                            .aria_label(sheet_title.clone())
                            .child(sheet_title),
                    ),
                )
                .size(sheet_width)
                .p_0()
                .child(content)
                .on_close(move |_, window, cx| {
                    on_close_view.update(cx, |view, cx| view.handle_settings_sheet_close(window, cx));
                })
        });
        cx.notify();
    }

    pub(super) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_sheet(cx) {
            window.close_sheet(cx);
        }
        self.finish_settings_close(window, cx);
    }

    fn finish_settings_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_open {
            return;
        }
        self.settings_open = false;
        self.config_page_open = false;
        if self.shortcuts_editing {
            self.shortcut_inputs = shortcuts::inputs(&self.preferences.shortcuts, window, cx);
            self.shortcuts_editing = false;
            self.shortcut_message = None;
        }
        if let Some(focus) = self.settings_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    fn handle_settings_sheet_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.config_editor.is_none() {
            self.finish_settings_close(window, cx);
            return;
        }

        if !self.config_write_pending {
            self.config_editor = None;
            self.config_editor_error = None;
            self.config_editor_return_focus = None;
        }
        let settings_return_focus = self.settings_return_focus.take();
        self.settings_open = false;
        self.open_settings(window, cx);
        self.settings_return_focus = settings_return_focus;
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

    /// Both side panels are collapsed by settings, leaving the main container
    /// full-bleed. The zen toggle then floats over the container instead of
    /// living in the rail, header, or title bar, so it never moves between
    /// modes and can never collide with a side panel.
    pub(super) fn side_panels_collapsed(&self) -> bool {
        !self.preferences.navigation_visible() && !self.inspector_open
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
        if self.config_editor.is_some() {
            self.close_cli_config_editor(window, cx);
        } else if self.navigation_peek {
            self.navigation_peek = false;
            cx.notify();
        } else if self.settings_open {
            self.close_settings(window, cx);
        } else if self.palette.is_some() {
            self.close_palette(window, cx);
        } else if self.review_open {
            self.close_review(window, cx);
        } else if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        } else {
            cx.propagate();
        }
    }

    pub(super) fn open_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.change_viewer.is_none() {
            return;
        }
        self.workspace_review = None;
        self.show_review(window, cx);
    }

    pub(super) fn show_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open {
            self.close_settings(window, cx);
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        if !self.review_open {
            self.review_return_focus = window.focused(cx);
        }
        self.review_open = true;
        self.review_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.review_open = false;
        self.workspace_review = None;
        if let Some(focus) = self.review_return_focus.take() {
            focus.focus(window, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    /// Return focus when a state transition removes the focused review surface.
    pub(super) fn restore_focus_after_review_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.review_open && self.review_focus.contains_focused(window, cx) {
            self.review_return_focus = None;
            self.focus_composer(window, cx);
        }
    }
}
