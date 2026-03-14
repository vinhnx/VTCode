use crate::config::KeyboardProtocolConfig;
use crate::config::loader::VTCodeConfig;
use crate::config::types::{ReasoningEffortLevel, UiSurfacePreference};
use crate::ui::slash::SlashCommandInfo;
use crate::ui::theme::ThemeStyles;
use vtcode_tui::KeyboardProtocolSettings;
use vtcode_tui::ReasoningEffortLevel as TuiReasoningEffortLevel;
use vtcode_tui::SessionAppearanceConfig;
use vtcode_tui::SessionSurface;
use vtcode_tui::SlashCommandItem;
use vtcode_tui::ui::theme::ThemeStyles as TuiThemeStyles;

pub fn to_tui_reasoning(level: ReasoningEffortLevel) -> TuiReasoningEffortLevel {
    match level {
        ReasoningEffortLevel::None => TuiReasoningEffortLevel::None,
        ReasoningEffortLevel::Minimal => TuiReasoningEffortLevel::Minimal,
        ReasoningEffortLevel::Low => TuiReasoningEffortLevel::Low,
        ReasoningEffortLevel::Medium => TuiReasoningEffortLevel::Medium,
        ReasoningEffortLevel::High => TuiReasoningEffortLevel::High,
        ReasoningEffortLevel::XHigh => TuiReasoningEffortLevel::XHigh,
    }
}

pub fn from_tui_reasoning(level: TuiReasoningEffortLevel) -> ReasoningEffortLevel {
    match level {
        TuiReasoningEffortLevel::None => ReasoningEffortLevel::None,
        TuiReasoningEffortLevel::Minimal => ReasoningEffortLevel::Minimal,
        TuiReasoningEffortLevel::Low => ReasoningEffortLevel::Low,
        TuiReasoningEffortLevel::Medium => ReasoningEffortLevel::Medium,
        TuiReasoningEffortLevel::High => ReasoningEffortLevel::High,
        TuiReasoningEffortLevel::XHigh => ReasoningEffortLevel::XHigh,
    }
}

pub fn to_tui_surface(preference: UiSurfacePreference) -> SessionSurface {
    match preference {
        UiSurfacePreference::Auto => SessionSurface::Auto,
        UiSurfacePreference::Alternate => SessionSurface::Alternate,
        UiSurfacePreference::Inline => SessionSurface::Inline,
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

pub fn inline_theme_from_core_styles(styles: &ThemeStyles) -> vtcode_tui::InlineTheme {
    let mapped = tui_theme_styles_from_core(styles);
    vtcode_tui::theme_from_styles(&mapped)
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
            crate::config::UiDisplayMode::Full => {
                vtcode_tui::core_tui::session::config::UiMode::Full
            }
            crate::config::UiDisplayMode::Minimal => {
                vtcode_tui::core_tui::session::config::UiMode::Minimal
            }
            crate::config::UiDisplayMode::Focused => {
                vtcode_tui::core_tui::session::config::UiMode::Focused
            }
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
            crate::config::LayoutModeOverride::Auto => {
                vtcode_tui::core_tui::session::config::LayoutModeOverride::Auto
            }
            crate::config::LayoutModeOverride::Compact => {
                vtcode_tui::core_tui::session::config::LayoutModeOverride::Compact
            }
            crate::config::LayoutModeOverride::Standard => {
                vtcode_tui::core_tui::session::config::LayoutModeOverride::Standard
            }
            crate::config::LayoutModeOverride::Wide => {
                vtcode_tui::core_tui::session::config::LayoutModeOverride::Wide
            }
        },
        reasoning_display_mode: match config.ui.reasoning_display_mode {
            crate::config::ReasoningDisplayMode::Always => {
                vtcode_tui::core_tui::session::config::ReasoningDisplayMode::Always
            }
            crate::config::ReasoningDisplayMode::Toggle => {
                vtcode_tui::core_tui::session::config::ReasoningDisplayMode::Toggle
            }
            crate::config::ReasoningDisplayMode::Hidden => {
                vtcode_tui::core_tui::session::config::ReasoningDisplayMode::Hidden
            }
        },
        reasoning_visible_default: config.ui.reasoning_visible_default,
        screen_reader_mode: config.ui.screen_reader_mode,
        reduce_motion_mode,
        reduce_motion_keep_progress_animation: config.ui.reduce_motion_keep_progress_animation,
        customization: Default::default(),
    }
}
