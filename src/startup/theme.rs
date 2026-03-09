use anyhow::{Context, Result};
use vtcode_core::cli::args::Cli;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::load_user_config;
use vtcode_core::ui::theme::{self as ui_theme, DEFAULT_THEME_ID};

pub(super) async fn determine_theme(args: &Cli, config: &VTCodeConfig) -> Result<String> {
    let color_config = ui_theme::ColorAccessibilityConfig {
        minimum_contrast: config.ui.minimum_contrast,
        bold_is_bright: config.ui.bold_is_bright,
        safe_colors_only: config.ui.safe_colors_only,
    };
    ui_theme::set_color_accessibility_config(color_config);

    let user_theme_pref = load_user_config().await.ok().and_then(|dot| {
        let trimmed = dot.preferences.theme.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    });

    let config_theme = config.agent.theme.trim();
    let auto_theme = match config.ui.color_scheme_mode {
        vtcode_config::root::ColorSchemeMode::Auto => {
            Some(ui_theme::suggest_theme_for_terminal().to_owned())
        }
        vtcode_config::root::ColorSchemeMode::Light => Some("vitesse-light".to_owned()),
        vtcode_config::root::ColorSchemeMode::Dark => None,
    };

    let mut theme_selection = args
        .theme
        .clone()
        .or_else(|| (!config_theme.is_empty()).then(|| config_theme.to_string()))
        .or(user_theme_pref)
        .or(auto_theme)
        .unwrap_or_else(|| DEFAULT_THEME_ID.to_owned());

    if let Err(err) = ui_theme::set_active_theme(&theme_selection) {
        if args.theme.is_some() {
            return Err(err.context(format!("Failed to activate theme '{}'", theme_selection)));
        }

        theme_selection = DEFAULT_THEME_ID.to_owned();
        ui_theme::set_active_theme(&theme_selection)
            .with_context(|| format!("Failed to activate theme '{}'", theme_selection))?;
    }

    let validation = ui_theme::validate_theme_contrast(&theme_selection);
    for warning in &validation.warnings {
        tracing::debug!(theme = %theme_selection, warning = %warning, "Theme contrast warning");
    }

    if !ui_theme::theme_matches_terminal_scheme(&theme_selection) {
        let scheme_kind = if ui_theme::is_light_theme(&theme_selection) {
            "light"
        } else {
            "dark"
        };
        tracing::warn!(
            theme = %theme_selection,
            "Theme '{}' is {} but your terminal appears {}. \
             The theme background is painted automatically for readability. \
             Set ui.color_scheme_mode = \"auto\" in vtcode.toml or pick a matching theme.",
            theme_selection,
            scheme_kind,
            if scheme_kind == "light" { "dark" } else { "light" },
        );
    }

    Ok(theme_selection)
}
