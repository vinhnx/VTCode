use vtcode_core::config::KeyboardProtocolConfig as CoreKeyboardProtocolConfig;
use vtcode_core::config::loader::VTCodeConfig as CoreVTCodeConfig;
use vtcode_core::config::types::{
    ReasoningEffortLevel as CoreReasoningEffortLevel,
    UiSurfacePreference as CoreUiSurfacePreference,
};
use vtcode_core::ui::slash::SlashCommandInfo as CoreSlashCommandInfo;
use vtcode_core::ui::theme::ThemeStyles as CoreThemeStyles;
use vtcode_tui::KeyboardProtocolSettings;
use vtcode_tui::ReasoningEffortLevel as TuiReasoningEffortLevel;
use vtcode_tui::SessionAppearanceConfig;
use vtcode_tui::SessionSurface;
use vtcode_tui::SlashCommandItem;
use vtcode_tui::ui::theme::ThemeStyles as TuiThemeStyles;

pub(crate) fn to_tui_reasoning(level: CoreReasoningEffortLevel) -> TuiReasoningEffortLevel {
    match level {
        CoreReasoningEffortLevel::None => TuiReasoningEffortLevel::None,
        CoreReasoningEffortLevel::Minimal => TuiReasoningEffortLevel::Minimal,
        CoreReasoningEffortLevel::Low => TuiReasoningEffortLevel::Low,
        CoreReasoningEffortLevel::Medium => TuiReasoningEffortLevel::Medium,
        CoreReasoningEffortLevel::High => TuiReasoningEffortLevel::High,
        CoreReasoningEffortLevel::XHigh => TuiReasoningEffortLevel::XHigh,
    }
}

pub(crate) fn from_tui_reasoning(level: TuiReasoningEffortLevel) -> CoreReasoningEffortLevel {
    match level {
        TuiReasoningEffortLevel::None => CoreReasoningEffortLevel::None,
        TuiReasoningEffortLevel::Minimal => CoreReasoningEffortLevel::Minimal,
        TuiReasoningEffortLevel::Low => CoreReasoningEffortLevel::Low,
        TuiReasoningEffortLevel::Medium => CoreReasoningEffortLevel::Medium,
        TuiReasoningEffortLevel::High => CoreReasoningEffortLevel::High,
        TuiReasoningEffortLevel::XHigh => CoreReasoningEffortLevel::XHigh,
    }
}

pub(crate) fn to_tui_surface(preference: CoreUiSurfacePreference) -> SessionSurface {
    match preference {
        CoreUiSurfacePreference::Auto => SessionSurface::Auto,
        CoreUiSurfacePreference::Alternate => SessionSurface::Alternate,
        CoreUiSurfacePreference::Inline => SessionSurface::Inline,
    }
}

pub(crate) fn to_tui_keyboard_protocol(
    keyboard: CoreKeyboardProtocolConfig,
) -> KeyboardProtocolSettings {
    KeyboardProtocolSettings {
        enabled: keyboard.enabled,
        mode: keyboard.mode,
        disambiguate_escape_codes: keyboard.disambiguate_escape_codes,
        report_event_types: keyboard.report_event_types,
        report_alternate_keys: keyboard.report_alternate_keys,
        report_all_keys: keyboard.report_all_keys,
    }
}

pub(crate) fn tui_theme_styles_from_core(styles: &CoreThemeStyles) -> TuiThemeStyles {
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

pub(crate) fn inline_theme_from_core_styles(styles: &CoreThemeStyles) -> vtcode_tui::InlineTheme {
    let mapped = tui_theme_styles_from_core(styles);
    vtcode_tui::theme_from_styles(&mapped)
}

pub(crate) fn to_tui_slash_commands(commands: &[CoreSlashCommandInfo]) -> Vec<SlashCommandItem> {
    commands
        .iter()
        .map(|cmd| SlashCommandItem::new(cmd.name, cmd.description))
        .collect()
}

pub(crate) fn to_tui_appearance(config: &CoreVTCodeConfig) -> SessionAppearanceConfig {
    SessionAppearanceConfig {
        theme: config.agent.theme.clone(),
        ui_mode: match config.ui.display_mode {
            vtcode_core::config::UiDisplayMode::Full => {
                vtcode_tui::core_tui::session::config::UiMode::Full
            }
            vtcode_core::config::UiDisplayMode::Minimal => {
                vtcode_tui::core_tui::session::config::UiMode::Minimal
            }
            vtcode_core::config::UiDisplayMode::Focused => {
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
        customization: Default::default(),
    }
}
