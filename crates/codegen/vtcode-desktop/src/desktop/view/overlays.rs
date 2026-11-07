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

/// Panel navigation history with a cursor: entries hold the workspace base
/// plus opened panels. Opening a panel truncates forward entries and replaces
/// an open panel, so routes stay mutually exclusive; closing truncates back
/// to the workspace base. Mirrors [`NavigationHistory`]'s cursor discipline
/// for the route domain. Invariant: non-empty with [`Route::Workspace`] at
/// index 0 and the cursor inside the entries.
pub(super) struct RouteHistory {
    entries: Vec<Route>,
    cursor: usize,
}

impl RouteHistory {
    pub(super) fn new() -> Self {
        Self { entries: vec![Route::Workspace], cursor: 0 }
    }

    /// Currently visible route: the entry under the cursor.
    pub(super) fn current(&self) -> Route {
        self.entries.get(self.cursor).copied().unwrap_or(Route::Workspace)
    }

    /// Record opening a panel: drop forward entries, then replace an open
    /// panel or push over the workspace base. Callers tear down the previous
    /// top first. A Workspace input resets to the base.
    pub(super) fn open(&mut self, route: Route) {
        if route == Route::Workspace {
            self.dismiss_to_workspace();
            return;
        }
        self.entries.truncate(self.cursor.saturating_add(1));
        match self.entries.get_mut(self.cursor) {
            Some(top) if *top != Route::Workspace => *top = route,
            _ => {
                self.entries.push(route);
                self.cursor = self.entries.len() - 1;
            }
        }
        self.check_invariant();
    }

    /// Whether Back can move the cursor toward the workspace base.
    pub(super) fn can_back(&self) -> bool {
        self.cursor > 0
    }

    /// Whether Forward can replay a backed-out panel entry.
    pub(super) fn can_forward(&self) -> bool {
        self.cursor.saturating_add(1) < self.entries.len()
    }

    /// Move the cursor one entry back and return the route to show.
    pub(super) fn back(&mut self) -> Route {
        self.cursor = self.cursor.saturating_sub(1);
        self.check_invariant();
        self.current()
    }

    /// Move the cursor one entry forward and return the route to show.
    pub(super) fn forward(&mut self) -> Route {
        if self.can_forward() {
            self.cursor += 1;
        }
        self.check_invariant();
        self.current()
    }

    /// Drop every panel entry and park the cursor on the workspace base.
    /// Callers tear down the previous top first.
    pub(super) fn dismiss_to_workspace(&mut self) {
        self.entries.truncate(1);
        self.cursor = 0;
        self.check_invariant();
    }

    fn check_invariant(&self) {
        debug_assert!(!self.entries.is_empty(), "route history keeps its workspace base");
        debug_assert!(self.entries.first() == Some(&Route::Workspace), "route history starts at the workspace base");
        debug_assert!(self.cursor < self.entries.len().max(1), "route cursor stays inside its entries");
    }
}

impl Default for RouteHistory {
    fn default() -> Self {
        Self::new()
    }
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
        if self.license_viewer.is_some() {
            self.discard_license_viewer();
            cx.notify();
        }
        self.close_attachment_picker(window, cx);
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Currently visible route: the entry under the history cursor.
    pub(super) fn current_route(&self) -> Route {
        self.route_history.current()
    }

    /// State cleanup for the top route without navigating: opening a
    /// replacement panel tears down the old top while the history cursor and
    /// focus move to the new panel instead of the composer.
    pub(super) fn teardown_current_route(&mut self) {
        match self.current_route() {
            Route::Settings => self.teardown_settings(),
            Route::Review => self.workspace_review = None,
            Route::Browser | Route::Workspace => {}
        }
    }

    fn teardown_settings(&mut self) {
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
        // A staged custom theme is page-local too; reopening always starts fresh.
        self.theme_editor.draft = None;
    }

    /// Focus the entry point of a route: the panel focus for panels, the
    /// composer for the workspace. The browser lands in its session list so
    /// arrows work immediately; header buttons stay one Shift+Tab away.
    pub(super) fn focus_route(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        match route {
            Route::Settings => self.settings_navigation_focus.focus(window, cx),
            Route::Browser => self.focus_browser_list(window, cx),
            Route::Review => self.review_focus.focus(window, cx),
            Route::Workspace => self.focus_composer(window, cx),
        }
    }

    /// Focus the browser's actionable list when it renders. The empty
    /// browser shows no list, so focus keeps the panel instead of a handle
    /// with no element.
    fn focus_browser_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sessions.is_empty() {
            self.browser_focus.focus(window, cx);
        } else {
            self.browser_list_focus.focus(window, cx);
        }
    }

    /// Close whichever non-workspace route is active, running its teardown
    /// (state cleanup + focus restore). No-op on the workspace route.
    pub(super) fn close_active_route(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_route() == Route::Workspace {
            return;
        }
        self.teardown_current_route();
        self.route_history.dismiss_to_workspace();
        // See close_browser: route close always lands in the composer.
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Staged page state a pointer dismissal must confirm before dropping:
    /// unsaved shortcut edits or an open custom-theme draft. Interior layers
    /// (config editor, capture) unwind first instead — see
    /// [`Self::request_panel_dismiss`].
    fn panel_has_unsaved_changes(&self) -> bool {
        self.shortcuts_editing || self.custom_theme_editor_open()
    }

    /// One dismiss step for pointer gestures (dismiss buttons, gutter press,
    /// session rows): unwind one interior layer when one is open, else close
    /// the route — prompting first when staged state would be lost. A config
    /// write in flight pins everything, like [`Self::go_route_back`].
    pub(super) fn request_panel_dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let route = self.current_route();
        if route == Route::Workspace {
            return;
        }
        if self.config_write_pending {
            return;
        }
        if self.config_editor.is_some() {
            self.request_config_editor_dismiss(window, cx);
            return;
        }
        if self.shortcut_capture.is_some() {
            self.cancel_shortcut_capture(window, cx);
            return;
        }
        if !self.panel_has_unsaved_changes() {
            self.close_active_route(window, cx);
            return;
        }
        // The prompt copy names settings edits: only Settings stages page
        // state today, so any other route reaching this is a future dirt
        // source that needs its own copy.
        self.prompt_discard_changes(DiscardIntent::CloseRoute(route), window, cx);
    }

    /// Native warning prompt guarding staged state, mirroring the delete
    /// confirms. Headless-untestable like them, so the post-confirm step
    /// lives in [`Self::discard_after_confirm`], which tests drive directly.
    pub(super) fn prompt_discard_changes(
        &mut self,
        intent: DiscardIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (title, detail) = discard_prompt_text(&intent);
        let discard = rust_i18n::t!("app.navigation.discard").to_string();
        let cancel = rust_i18n::t!(i18n::CONFIGURATION_CANCEL).to_string();
        let answer = window.prompt(
            gpui_kit::PromptLevel::Warning,
            title.as_str(),
            Some(detail.as_str()),
            &[discard.as_str(), cancel.as_str()],
            cx,
        );
        cx.spawn_in(window, async move |view, cx| {
            if matches!(answer.await, Ok(0)) {
                let _updated = view.update_in(cx, |view, window, cx| {
                    view.discard_after_confirm(intent, window, cx);
                });
            }
        })
        .detach();
    }

    /// Run a confirmed discard. Verifies the context is unchanged first —
    /// the prompt answer arrives async, so the surface may have moved on.
    pub(super) fn discard_after_confirm(&mut self, intent: DiscardIntent, window: &mut Window, cx: &mut Context<Self>) {
        match intent {
            DiscardIntent::CloseRoute(route) if self.current_route() == route => {
                self.close_active_route(window, cx);
            }
            DiscardIntent::CloseConfigEditor(path) => {
                self.discard_config_editor_after_confirm(&path, window, cx);
            }
            DiscardIntent::CancelRename(rel_path) => {
                self.discard_rename_after_confirm(&rel_path, window, cx);
            }
            _ => {}
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
        if self.current_route() == Route::Settings {
            return;
        }
        self.teardown_current_route();
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        self.close_attachment_picker(window, cx);
        // Entries can reach Settings while the viewer is open (deeplinks, the
        // slash command); the viewer would layer over the page it came from.
        self.discard_license_viewer();
        self.route_history.open(Route::Settings);
        if self.settings_tab == settings::SettingsTab::Storage {
            self.maybe_request_storage_summary(cx);
        }
        // The page embeds in the content slot (see root_render), so opening
        // only moves focus; body-level actions stay reachable, unlike the
        // former sheet.
        self.focus_route(Route::Settings, window, cx);
        cx.notify();
    }

    pub(super) fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_route() == Route::Settings {
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
    /// Whether Back has a route-level destination: panel-interior state or
    /// a history entry behind the cursor. The toolbar enables Back when this
    /// or session history can move.
    pub(super) fn route_back_available(&self) -> bool {
        self.current_route() != Route::Workspace
            && (self.config_editor.is_some()
                || self.shortcut_capture.is_some()
                || self.palette.is_some()
                || self.license_viewer.is_some()
                || self.config_page_open
                || self.route_history.can_back())
    }

    /// Back one route step, walking panel-interior layers before the history
    /// cursor: typed editor, shortcut capture, palette, config page to its
    /// parent tab, then the previous history entry. Returns whether anything
    /// moved; a write in flight swallows the press like Escape does.
    pub(super) fn go_route_back(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.current_route() == Route::Workspace {
            return false;
        }
        if self.license_viewer.is_some() {
            self.close_license_viewer(window, cx);
            return true;
        }
        if self.config_editor.is_some() {
            if self.config_write_pending {
                return true;
            }
            self.close_cli_config_editor(window, cx);
        } else if self.shortcut_capture.is_some() {
            self.cancel_shortcut_capture(window, cx);
        } else if self.custom_theme_editor_open() {
            self.cancel_custom_theme_editor(cx);
        } else if self.palette.is_some() {
            self.close_palette(window, cx);
        } else if self.config_page_open {
            self.config_page_open = false;
            self.focus_route(Route::Settings, window, cx);
            cx.notify();
        } else if self.route_history.can_back() {
            self.teardown_current_route();
            let route = self.route_history.back();
            self.focus_route(route, window, cx);
            cx.notify();
        } else {
            return false;
        }
        true
    }

    /// Forward one history entry, re-showing a backed-out panel. Interior
    /// pages are not history: Forward restores the panel, not its layers.
    /// Returns whether the cursor moved.
    pub(super) fn go_route_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.route_history.can_forward() {
            return false;
        }
        let route = self.route_history.forward();
        self.focus_route(route, window, cx);
        cx.notify();
        true
    }

    fn finish_settings_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_route() != Route::Settings {
            return;
        }
        self.discard_license_viewer();
        self.teardown_settings();
        self.route_history.dismiss_to_workspace();
        // See close_browser: route close always lands in the composer.
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(super) fn toggle_inspector(&mut self, cx: &mut Context<Self>) {
        // The inspector hides while a panel owns the content slot; flipping
        // invisible state would surprise on return, so the toggle waits.
        if self.workspace.is_some() && self.current_route() == Route::Workspace {
            if self.zen_mode {
                // Zen hides the inspector regardless of state, so there is
                // nothing visible to close: reveal the current tab instead of
                // flipping (and persisting) invisible state.
                self.open_inspector_tab(self.inspector_tab, cx);
            } else {
                self.inspector_open = !self.inspector_open;
            }
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
        self.save_preferences();
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
        self.disarm_double_escape();
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

    /// Rebuild the native menus when full-screen state flips so the View item
    /// reads Enter or Exit Full Screen (HIG). No-op while unchanged.
    pub(super) fn refresh_full_screen_menus(&mut self, window: &Window, cx: &mut Context<Self>) {
        let full_screen = window.is_fullscreen();
        if full_screen != self.menus_show_exit_full_screen {
            self.menus_show_exit_full_screen = full_screen;
            menu_bar::set_native_menus(cx, full_screen);
        }
    }

    /// An overlay lifecycle event invalidates an armed double-Esc: clearing
    /// requires two consecutive bare presses with nothing opening or
    /// dismissing in between.
    pub(super) fn disarm_double_escape(&mut self) {
        self.last_escape_press = None;
    }

    pub(super) fn dismiss_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // About is the topmost modal: it opens over any route from the
        // application menu, so Escape dismisses it before other layers.
        if self.about_open {
            self.disarm_double_escape();
            self.close_about(window, cx);
        } else if self.running_confirm.is_some() {
            self.disarm_double_escape();
            self.cancel_running_confirm(window, cx);
        } else if self.full_access_confirm.is_some() {
            self.disarm_double_escape();
            self.cancel_full_access_confirm(window, cx);
        } else if self.file_rename.is_some() {
            self.disarm_double_escape();
            self.cancel_file_rename(window, cx);
        } else if self.changelog_open {
            // Painted above the license viewer, so Escape reaches it first.
            self.disarm_double_escape();
            self.close_changelog(window, cx);
        } else if self.license_viewer.is_some() {
            self.disarm_double_escape();
            self.close_license_viewer(window, cx);
        } else if self.attachment_picker.is_some() {
            self.disarm_double_escape();
            self.close_attachment_picker(window, cx);
        } else if self.go_route_back(window, cx) {
            // Panel-interior layers first (typed editor, capture, palette,
            // config page to its tab), then the previous history entry.
            self.disarm_double_escape();
        } else if self.navigation_peek {
            self.disarm_double_escape();
            self.navigation_peek = false;
            cx.notify();
        } else if self.palette.is_some() {
            self.disarm_double_escape();
            self.close_palette(window, cx);
        } else if self.file_preview.is_some() {
            self.disarm_double_escape();
            self.close_file_preview(window, cx);
        } else if self.composer.read(cx).focus_handle(cx).is_focused(window)
            && !self.composer.read(cx).value().is_empty()
        {
            // CLI `Esc Esc` parity: consecutive presses with nothing to
            // dismiss clear the draft text (attachments keep their own
            // remove controls). Dismissals above never arm the timer and
            // disarm a pending one, and a changed draft re-arms instead
            // of clearing.
            const DOUBLE_ESCAPE_WINDOW: Duration = Duration::from_millis(800);
            let draft = self.composer.read(cx).value();
            let now = Instant::now();
            let consecutive = self
                .last_escape_press
                .as_ref()
                .is_some_and(|(at, snapshot)| *snapshot == draft && now.duration_since(*at) < DOUBLE_ESCAPE_WINDOW);
            if consecutive {
                self.last_escape_press = None;
                // Undoable clear: `replace_all` keeps the draft on the undo
                // stack where `set_value` would erase it.
                self.replace_composer_text("", window, cx);
                self.reset_history_recall();
                cx.notify();
            } else {
                self.last_escape_press = Some((now, draft));
                cx.propagate();
            }
        } else if self.transcript_focus.is_focused(window) || self.session_focus.is_focused(window) {
            // Leave list navigation where it is and return to the composer.
            self.focus_composer(window, cx);
        } else {
            cx.propagate();
        }
    }

    pub(super) fn open_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_route() == Route::Review {
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
        self.teardown_current_route();
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        self.close_attachment_picker(window, cx);
        self.route_history.open(Route::Review);
        self.focus_route(Route::Review, window, cx);
        cx.notify();
    }

    pub(super) fn close_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.teardown_current_route();
        self.route_history.dismiss_to_workspace();
        // See close_browser: route close always lands in the composer.
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(super) fn open_thread_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_route() == Route::Browser {
            self.close_browser(window, cx);
            return;
        }
        self.teardown_current_route();
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        if self.file_preview.is_some() {
            self.close_file_preview(window, cx);
        }
        self.close_attachment_picker(window, cx);
        self.route_history.open(Route::Browser);
        self.focus_route(Route::Browser, window, cx);
        cx.notify();
    }

    pub(super) fn close_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.teardown_current_route();
        self.route_history.dismiss_to_workspace();
        // Routes unmount the workspace, so Kit keyed focus handles captured
        // at open time go stale; like closing an editor tab, Back always
        // lands focus deterministically in the composer.
        self.focus_composer(window, cx);
        cx.notify();
    }

    /// Return focus when a state transition removes the focused review surface.
    pub(super) fn restore_focus_after_review_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.current_route() != Route::Review && self.review_focus.contains_focused(window, cx) {
            self.focus_composer(window, cx);
        }
    }
}

/// What a discard prompt protects: the staged state dropped when the user
/// confirms. Carries enough identity to verify the context is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DiscardIntent {
    /// Close the route page, dropping staged shortcut/theme edits.
    CloseRoute(Route),
    /// Close the CLI config editor, dropping the typed value.
    CloseConfigEditor(String),
    /// Cancel the rename dialog, dropping the typed name.
    CancelRename(String),
}

/// Localized title/detail for one discard intent. Pure so unit tests pin
/// the interpolation without rendering.
fn discard_prompt_text(intent: &DiscardIntent) -> (String, String) {
    match intent {
        DiscardIntent::CloseRoute(_) => (
            rust_i18n::t!("app.settings.discard_title").to_string(),
            rust_i18n::t!("app.settings.discard_detail").to_string(),
        ),
        DiscardIntent::CloseConfigEditor(path) => (
            rust_i18n::t!("app.configuration.discard_title").to_string(),
            rust_i18n::t!("app.configuration.discard_detail", path = path).to_string(),
        ),
        DiscardIntent::CancelRename(_) => (
            rust_i18n::t!("app.files.discard_title").to_string(),
            rust_i18n::t!("app.files.discard_detail").to_string(),
        ),
    }
}

/// Route container with gutter dismissal: presses on the padded area around
/// the centered panel dismiss through [`DesktopView::request_panel_dismiss`].
/// Each page stops its own presses, so only true background presses arrive
/// here — toolbar and sidebar chrome never does.
pub(super) fn dismissable_route_page(panel: impl IntoElement, cx: &mut Context<DesktopView>) -> impl IntoElement {
    div()
        .id("route-page-gutter")
        .test_support()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .on_mouse_down(MouseButton::Left, cx.listener(|view, _, window, cx| view.request_panel_dismiss(window, cx)))
        .child(route_page(panel))
}

#[cfg(test)]
mod tests {
    use super::{DiscardIntent, Route, RouteHistory, discard_prompt_text};

    #[test]
    fn discard_prompt_text_names_each_surface() {
        crate::desktop::localization::initialize(crate::desktop::localization::UiLanguage::English);
        let (title, detail) = discard_prompt_text(&DiscardIntent::CloseRoute(Route::Settings));
        assert_eq!(title, "Discard unsaved changes?");
        assert_eq!(detail, "Unsaved shortcut and theme edits will be lost.");
        // Asymmetric path proves the interpolation lands in the detail.
        let (title, detail) = discard_prompt_text(&DiscardIntent::CloseConfigEditor("agent.default_model".into()));
        assert_eq!(title, "Discard unsaved value?");
        assert_eq!(detail, "Changes to agent.default_model will be lost.");
        let (title, detail) = discard_prompt_text(&DiscardIntent::CancelRename("README.md".into()));
        assert_eq!(title, "Discard rename?");
        assert_eq!(detail, "The name change will be lost.");
    }

    #[test]
    fn starts_on_the_workspace_base() {
        let history = RouteHistory::new();
        assert_eq!(history.current(), Route::Workspace);
        assert_eq!(RouteHistory::default().current(), Route::Workspace);
    }

    #[test]
    fn opening_a_panel_replaces_an_open_panel() {
        let mut history = RouteHistory::new();
        history.open(Route::Settings);
        history.open(Route::Browser);
        assert_eq!(history.current(), Route::Browser);
        assert_eq!(history.entries, vec![Route::Workspace, Route::Browser]);
    }

    #[test]
    fn opening_the_workspace_resets_to_the_base() {
        let mut history = RouteHistory::new();
        history.open(Route::Review);
        history.open(Route::Workspace);
        assert_eq!(history.current(), Route::Workspace);
        assert_eq!(history.entries, vec![Route::Workspace]);
    }

    #[test]
    fn dismissing_clears_panels_back_to_the_base() {
        let mut history = RouteHistory::new();
        history.open(Route::Settings);
        history.dismiss_to_workspace();
        assert_eq!(history.current(), Route::Workspace);
        assert_eq!(history.entries, vec![Route::Workspace]);
        history.dismiss_to_workspace();
        assert_eq!(history.current(), Route::Workspace);
    }

    #[test]
    fn current_falls_back_to_workspace_off_the_entries() {
        let history = RouteHistory { entries: Vec::new(), cursor: 5 };
        assert_eq!(history.current(), Route::Workspace);
    }

    #[test]
    fn back_and_forward_move_the_cursor_without_dropping_entries() {
        let mut history = RouteHistory::new();
        history.open(Route::Settings);
        assert!(history.can_back());
        assert!(!history.can_forward());
        assert_eq!(history.back(), Route::Workspace);
        assert!(!history.can_back());
        assert!(history.can_forward());
        assert_eq!(history.forward(), Route::Settings);
        assert!(history.can_back());
        assert!(!history.can_forward());
    }

    #[test]
    fn opening_truncates_forward_entries() {
        let mut history = RouteHistory::new();
        history.open(Route::Settings);
        assert_eq!(history.back(), Route::Workspace);
        history.open(Route::Browser);
        assert_eq!(history.current(), Route::Browser);
        assert!(!history.can_forward());
        assert_eq!(history.back(), Route::Workspace);
        assert_eq!(history.forward(), Route::Browser);
    }

    #[test]
    fn back_and_forward_clamp_at_the_ends() {
        let mut history = RouteHistory::new();
        assert_eq!(history.back(), Route::Workspace);
        assert_eq!(history.forward(), Route::Workspace);
    }
}
