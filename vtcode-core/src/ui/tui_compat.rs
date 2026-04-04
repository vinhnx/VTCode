//! Compatibility layer between vtcode-core config types and TUI types.
//!
//! These conversions bridge the core configuration layer with the UI surface
//! types shared via `vtcode-commons::ui_protocol`.

use crate::config::KeyboardProtocolConfig;
use crate::config::loader::VTCodeConfig;
use crate::config::types::ReasoningEffortLevel;
use crate::ui::slash::SlashCommandInfo;
use crate::ui::theme::ThemeStyles;
use crate::ui::tui::{
    InlineTheme, KeyboardProtocolSettings, LayoutModeOverride, ReasoningDisplayMode,
    SessionSurface, SlashCommandItem, UiMode,
};
#[cfg(feature = "tui")]
use vtcode_tui::ui::theme::ThemeStyles as TuiThemeStyles;

#[cfg(feature = "tui")]
use crate::ui::tui::{FullscreenInteractionSettings, SessionAppearanceConfig};

#[cfg(not(feature = "tui"))]
use crate::ui::tui::SessionAppearanceConfig;

/// Convert a config `ReasoningEffortLevel` to the string form used by
/// `InlineListSelection::Reasoning`.
pub fn reasoning_to_selection_string(level: ReasoningEffortLevel) -> String {
    level.as_str().to_owned()
}

/// Convert a reasoning selection string back to a `ReasoningEffortLevel`.
pub fn reasoning_from_selection_string(s: &str) -> ReasoningEffortLevel {
    ReasoningEffortLevel::parse(s).unwrap_or_default()
}

pub fn to_tui_surface(preference: crate::config::types::UiSurfacePreference) -> SessionSurface {
    match preference {
        crate::config::types::UiSurfacePreference::Auto => SessionSurface::Auto,
        crate::config::types::UiSurfacePreference::Alternate => SessionSurface::Alternate,
        crate::config::types::UiSurfacePreference::Inline => SessionSurface::Inline,
    }
}

pub fn to_tui_keyboard_protocol(keyboard: KeyboardProtocolConfig) -> KeyboardProtocolSettings {
    KeyboardProtocolSettings {
        enabled: keyboard.enabled,
        mode: keyboard.mode,
        disambiguate_escape_codes: keyboard.disambiguate_escape_codes,
        report_event_types: keyboard.report_event_types,
        report_alternate_keys: keyboard.report_alternate_keys,
        report_all_keys: keyboard.report_all_keys,
    }
}

#[cfg(feature = "tui")]
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
    )
}

#[cfg(feature = "tui")]
pub fn tui_theme_styles_from_core(styles: &ThemeStyles) -> TuiThemeStyles {
    TuiThemeStyles {
        info: styles.info,
        error: styles.error,
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

#[cfg(not(feature = "tui"))]
pub fn tui_theme_styles_from_core(styles: &ThemeStyles) -> ThemeStyles {
    styles.clone()
}

pub fn to_tui_slash_commands(commands: &[SlashCommandInfo]) -> Vec<SlashCommandItem> {
    commands
        .iter()
        .map(|cmd| SlashCommandItem::new(cmd.name, cmd.description))
        .collect()
}

pub fn to_tui_appearance(config: &VTCodeConfig) -> SessionAppearanceConfig {
    let reduce_motion_mode =
        config.ui.reduce_motion_mode || matches!(config.tui.animations, Some(false));
    SessionAppearanceConfig {
        theme: config.agent.theme.clone(),
        ui_mode: match config.ui.display_mode {
            crate::config::UiDisplayMode::Full => UiMode::Full,
            crate::config::UiDisplayMode::Minimal => UiMode::Minimal,
            crate::config::UiDisplayMode::Focused => UiMode::Focused,
        },
        show_sidebar: config.ui.show_sidebar,
        min_content_width: 40,
        min_navigation_width: 20,
        navigation_width_percent: 25,
        transcript_bottom_padding: 0,
        dim_completed_todos: config.ui.dim_completed_todos,
        message_block_spacing: if config.ui.message_block_spacing {
            1
        } else {
            0
        },
        layout_mode: match config.ui.layout_mode {
            crate::config::LayoutModeOverride::Auto => LayoutModeOverride::Auto,
            crate::config::LayoutModeOverride::Compact => LayoutModeOverride::Compact,
            crate::config::LayoutModeOverride::Standard => LayoutModeOverride::Standard,
            crate::config::LayoutModeOverride::Wide => LayoutModeOverride::Wide,
        },
        reasoning_display_mode: match config.ui.reasoning_display_mode {
            crate::config::ReasoningDisplayMode::Always => ReasoningDisplayMode::Always,
            crate::config::ReasoningDisplayMode::Toggle => ReasoningDisplayMode::Toggle,
            crate::config::ReasoningDisplayMode::Hidden => ReasoningDisplayMode::Hidden,
        },
        reasoning_visible_default: config.ui.reasoning_visible_default,
        vim_mode: config.ui.vim_mode,
        screen_reader_mode: config.ui.screen_reader_mode,
        reduce_motion_mode,
        reduce_motion_keep_progress_animation: config.ui.reduce_motion_keep_progress_animation,
        customization: Default::default(),
    }
}
