//! Saved-preference load/persist steps split from view construction.

use super::*;

pub(super) fn apply_initial_theme(
    preferences: &preferences::Preferences,
    id: &str,
    window: &mut Window,
    cx: &mut App,
) -> (ThemeMode, String) {
    let mode =
        themes::apply_selection_with_contrast(id, preferences.contrast(), &preferences.custom_themes, Some(window), cx);
    (mode, themes::canonical_selection(id, mode, &preferences.custom_themes))
}

impl DesktopView {
    /// Both Settings pickers share one scan. Their saved and built-in choices
    /// are available immediately, including a saved app that has been removed.
    pub(in crate::desktop::view) fn discover_launch_destinations(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.destination_discovery_task.is_some() {
            return;
        }
        let roots = files::app_roots();
        self.destination_discovery_task = Some(cx.spawn_in(window, async move |view, cx| {
            let detected = cx.background_spawn(async move { files::scan_app_roots(&roots) }).await;
            let _updated = view.update_in(cx, |view, window, cx| {
                let destinations =
                    preferences::FileOpenDestination::offered(view.preferences.file_open_destination(), &detected);
                view.file_destination_select.update(cx, |state, cx| {
                    state.set_items(preferences::FileDestinationOption::for_destinations(&destinations), window, cx);
                });
                let terminals = preferences::CliTerminal::offered(view.preferences.cli_terminal(), &detected);
                view.cli_terminal_select.update(cx, |state, cx| {
                    state.set_items(preferences::CliTerminalOption::for_terminals(&terminals), window, cx);
                });
            });
        }));
    }

    #[cfg(test)]
    pub(in crate::desktop::view) fn load_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preferences.theme.is_some() || self.preferences.contrast() > 0 {
            let id = self.preferences.theme.as_deref().unwrap_or(&self.theme_id);
            (self.theme_mode, self.theme_id) = apply_initial_theme(&self.preferences, id, window, cx);
        }
        self.refresh_composer_highlight(cx);
        self.apply_loaded_preferences(cx);
    }

    /// Apply preferences after the initial palette has been selected.
    pub(super) fn apply_loaded_preferences(&mut self, cx: &mut Context<Self>) {
        let preferences = &mut self.preferences;
        preferences.follow_cli_theme = Some(preferences.follow_cli_theme.unwrap_or(preferences.theme.is_none()));
        self.sidebar_width = preferences.sidebar_width();
        self.terminal_panel_height = preferences.terminal_panel_height();
        self.terminal_panel_width = preferences.terminal_panel_width();
        self.inspector_width = preferences.inspector_width();
        self.apply_typography(cx);
        self.refresh_dock_icon();
        self.inspector_open = false;
        cx.notify();
    }

    /// Persist the current comfort settings. This is the one write point for
    /// every layout change (pointer drop, key release, and discrete toggles),
    /// so it also retires any deferred splitter-key write.
    pub(in crate::desktop::view) fn save_preferences(&mut self) {
        self.layout_save_pending = false;
        self.preferences.theme = Some(self.theme_id.clone());
        self.preferences.inspector_open = Some(self.inspector_open);
        self.preferences.zen_mode = Some(self.zen_mode);
        self.preferences.set_widths(self.sidebar_width, self.inspector_width);
        self.preferences.set_terminal_panel_height(self.terminal_panel_height);
        self.preferences.set_terminal_panel_width(self.terminal_panel_width);
        self.preferences.save();
    }
}
