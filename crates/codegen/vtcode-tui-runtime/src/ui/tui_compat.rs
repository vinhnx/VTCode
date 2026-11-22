//! Compatibility layer between vtcode-core config types and TUI types.
//!
//! These conversions bridge the core configuration layer with the UI surface
//! types shared via `vtcode-commons::ui_protocol`.

use crate::ui::theme::ThemeStyles;
use crate::ui::tui::{
    InlineTheme, KeyboardProtocolSettings, LayoutModeOverride, ReasoningDisplayMode, SessionSurface, SlashCommandItem,
    UiMode,
};
use vtcode_core::config::KeyboardProtocolConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_skills::command_slash::SlashCommandInfo;
use vtcode_ui::tui::ui::theme::ThemeStyles as TuiThemeStyles;

use crate::ui::tui::{FullscreenInteractionSettings, SessionAppearanceConfig};

/// Convert a config `ReasoningEffortLevel` to the string form used by
/// `InlineListSelection::Reasoning`.
pub fn reasoning_to_selection_string(level: ReasoningEffortLevel) -> String {
    level.as_str().to_owned()
}

/// Convert a reasoning selection string back to a `ReasoningEffortLevel`.
pub fn reasoning_from_selection_string(s: &str) -> ReasoningEffortLevel {
    ReasoningEffortLevel::parse(s).unwrap_or_default()
}

/// Convert a config `UiSurfacePreference` to the TUI `SessionSurface` enum.
///
/// The `From` impl is owned by `vtcode-config` alongside the config type, so
/// the conversion has a single definition shared by every surface.
pub fn to_tui_surface(preference: vtcode_core::config::types::UiSurfacePreference) -> SessionSurface {
    preference.into()
}

/// Convert a [`KeyboardProtocolConfig`] to the TUI [`KeyboardProtocolSettings`].
///
/// The `From` impl is owned by `vtcode-config` alongside the config type, so
/// the conversion has a single definition shared by every surface.
pub fn to_tui_keyboard_protocol(keyboard: KeyboardProtocolConfig) -> KeyboardProtocolSettings {
    keyboard.into()
}

/// Build [`FullscreenInteractionSettings`] from the loaded [`VTCodeConfig`].
pub fn to_tui_fullscreen(config: &VTCodeConfig) -> FullscreenInteractionSettings {
    FullscreenInteractionSettings {
        mouse_capture: config.ui.fullscreen.mouse_capture,
        copy_on_select: config.ui.fullscreen.copy_on_select,
        scroll_speed: config.ui.fullscreen.scroll_speed,
    }
}

/// Build an [`InlineTheme`] from core [`ThemeStyles`].
pub fn inline_theme_from_core_styles(styles: &ThemeStyles) -> InlineTheme {
    crate::ui::tui::theme_from_color_fields(
        styles.foreground,
        styles.background,
        styles.primary,
        styles.secondary,
        styles.tool,
        styles.tool_detail,
        styles.pty_output,
        styles.error,
        styles.warning,
    )
}

/// Convert core [`ThemeStyles`] to the TUI `ThemeStyles` variant (TUI build).
pub fn tui_theme_styles_from_core(styles: &ThemeStyles) -> TuiThemeStyles {
    TuiThemeStyles {
        info: styles.info,
        error: styles.error,
        warning: styles.warning,
        output: styles.output,
        response: styles.response,
        reasoning: styles.reasoning,
        tool: styles.tool,
        tool_detail: styles.tool_detail,
        tool_output: styles.tool_output,
        pty_output: styles.pty_output,
        status: styles.status,
        mcp: styles.mcp,
        user: styles.user,
        primary: styles.primary,
        secondary: styles.secondary,
        foreground: styles.foreground,
        background: styles.background,
    }
}

/// Convert a slice of [`SlashCommandInfo`] to TUI [`SlashCommandItem`] values.
pub fn to_tui_slash_commands(commands: &[SlashCommandInfo]) -> Vec<SlashCommandItem> {
    commands
        .iter()
        .map(|cmd| SlashCommandItem::new(cmd.name, cmd.description))
        .collect()
}

/// Build a [`SessionAppearanceConfig`] from the loaded [`VTCodeConfig`].
pub fn to_tui_appearance(config: &VTCodeConfig) -> SessionAppearanceConfig {
    let reduce_motion_mode = config.ui.reduce_motion_mode || matches!(config.tui.animations, Some(false));
    SessionAppearanceConfig {
        theme: config.agent.theme.clone(),
        ui_mode: match config.ui.display_mode {
            vtcode_core::config::UiDisplayMode::Full => UiMode::Full,
            vtcode_core::config::UiDisplayMode::Minimal | vtcode_core::config::UiDisplayMode::Unknown => {
                UiMode::Minimal
            }
            vtcode_core::config::UiDisplayMode::Focused => UiMode::Focused,
        },
        show_sidebar: config.ui.show_sidebar,
        min_content_width: 40,
        min_navigation_width: 20,
        navigation_width_percent: 25,
        transcript_bottom_padding: 0,
        dim_completed_todos: config.ui.dim_completed_todos,
        show_task_panel: config.ui.show_task_panel,
        message_block_spacing: if config.ui.message_block_spacing { 1 } else { 0 },
        layout_mode: match config.ui.layout_mode {
            vtcode_core::config::LayoutModeOverride::Auto | vtcode_core::config::LayoutModeOverride::Unknown => {
                LayoutModeOverride::Auto
            }
            vtcode_core::config::LayoutModeOverride::Compact => LayoutModeOverride::Compact,
            vtcode_core::config::LayoutModeOverride::Standard => LayoutModeOverride::Standard,
            vtcode_core::config::LayoutModeOverride::Wide => LayoutModeOverride::Wide,
        },
        reasoning_display_mode: match config.ui.reasoning_display_mode {
            vtcode_core::config::ReasoningDisplayMode::Always => ReasoningDisplayMode::Always,
            vtcode_core::config::ReasoningDisplayMode::Toggle | vtcode_core::config::ReasoningDisplayMode::Unknown => {
                ReasoningDisplayMode::Toggle
            }
            vtcode_core::config::ReasoningDisplayMode::Hidden => ReasoningDisplayMode::Hidden,
        },
        reasoning_visible_default: config.ui.reasoning_visible_default,
        thinking_display: config.ui.thinking_display,
        communication_style: config.ui.communication_style,
        vim_mode: config.ui.vim_mode,
        busy_enter_behavior: config.ui.busy_enter_behavior,
        readline_mode: false,
        screen_reader_mode: config.ui.screen_reader_mode,
        reduce_motion_mode,
        reduce_motion_keep_progress_animation: config.ui.reduce_motion_keep_progress_animation,
        hide_header: config.ui.hide_header,
        show_transcript_review_hints: config.ui.transcript_review.show_hints,
        show_transcript_review_shortcut_guide: config.ui.transcript_review.show_shortcut_guide,
        show_transcript_review_close_button: config.ui.transcript_review.show_close_button,
        diff_preview_mode: config.ui.diff_preview_mode,
        syntax_highlighting: config.syntax_highlighting.clone(),
        customization: Default::default(),
    }
}
