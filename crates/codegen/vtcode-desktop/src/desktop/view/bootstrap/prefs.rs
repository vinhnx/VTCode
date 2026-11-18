//! Saved-preference load/persist steps split from view construction.

use super::*;

impl DesktopView {
    /// Apply saved preferences: theme, app icon, pane widths, and inspector state.
    pub(in crate::desktop::view) fn load_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preferences = self.preferences.clone();
        self.preferences.follow_cli_theme = Some(preferences.follow_cli_theme.unwrap_or(preferences.theme.is_none()));
        self.restore_saved_theme(window, cx);
        self.apply_typography(cx);
        // Startup restore rewrites the palette (theme and saved contrast), so
        // composer token colors must follow it just like an in-app change.
        self.refresh_composer_highlight(cx);
        self.refresh_dock_icon();
        self.sidebar_width = preferences.sidebar_width();
        self.terminal_panel_height = preferences.terminal_panel_height();
        self.terminal_panel_width = preferences.terminal_panel_width();
        self.inspector_width = preferences.inspector_width();
        self.inspector_open = false;
        cx.notify();
    }

    /// Persist the current comfort settings.
    pub(in crate::desktop::view) fn save_preferences(&mut self) {
        self.preferences.theme = Some(self.theme_id.clone());
        self.preferences.inspector_open = Some(self.inspector_open);
        self.preferences.zen_mode = Some(self.zen_mode);
        self.preferences.set_widths(self.sidebar_width, self.inspector_width);
        self.preferences.set_terminal_panel_height(self.terminal_panel_height);
        self.preferences.set_terminal_panel_width(self.terminal_panel_width);
        self.preferences.save();
    }
}
