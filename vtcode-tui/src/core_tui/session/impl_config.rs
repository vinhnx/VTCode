use super::*;

impl Session {
    pub fn apply_config(&mut self, config: &crate::config::loader::VTCodeConfig) {
        if let Err(err) = crate::notifications::apply_global_notification_config_from_vtcode(config)
        {
            tracing::warn!("Failed to apply notification config at runtime: {}", err);
        }

        // Apply theme changes in real-time
        if crate::ui::theme::set_active_theme(&config.agent.theme).is_ok() {
            let active_styles = crate::ui::theme::active_styles();
            let inline_theme = crate::ui::tui::style::theme_from_styles(&active_styles);

            self.theme = inline_theme.clone();
            self.styles.set_theme(inline_theme);

            // Re-apply theme to prompt prefix if needed (though it usually uses self.theme)
            self.prompt_style.color = self.theme.primary.or(self.theme.foreground);
        }

        // Sync UI appearance settings from VTCodeConfig
        self.appearance.show_sidebar = config.ui.show_sidebar;
        self.appearance.dim_completed_todos = config.ui.dim_completed_todos;
        // Convert message_block_spacing bool to u8 (0 or 1)
        self.appearance.message_block_spacing = if config.ui.message_block_spacing {
            1
        } else {
            0
        };

        // Sync UI mode from VTCodeConfig
        self.appearance.ui_mode = match config.ui.display_mode {
            crate::config::UiDisplayMode::Full => config::UiMode::Full,
            crate::config::UiDisplayMode::Minimal => config::UiMode::Minimal,
            crate::config::UiDisplayMode::Focused => config::UiMode::Focused,
        };

        self.recalculate_transcript_rows();
        self.mark_dirty();
    }
}
