use anyhow::{Context, Result};
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::update_theme_preference;

pub(crate) async fn persist_theme_preference(
    renderer: &mut AnsiRenderer,
    theme_id: &str,
) -> Result<()> {
    if let Err(err) = update_theme_preference(theme_id).await {
        renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist theme preference: {}", err),
        )?;
    }
    if let Err(err) = persist_theme_config(theme_id) {
        renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist theme in vtcode.toml: {}", err),
        )?;
    }
    Ok(())
}

fn persist_theme_config(theme_id: &str) -> Result<()> {
    let mut manager =
        ConfigManager::load().context("Failed to load configuration for theme update")?;
    let mut config = manager.config().clone();
    if config.agent.theme != theme_id {
        config.agent.theme = theme_id.to_string();
        manager
            .save_config(&config)
            .context("Failed to save theme to configuration")?;
    }
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn ensure_turn_bottom_gap(
    renderer: &mut AnsiRenderer,
    applied: &mut bool,
) -> Result<()> {
    if !*applied {
        renderer.line_if_not_empty(MessageStyle::Output)?;
        *applied = true;
    }
    Ok(())
}

/// Display a user message using the active user styling
pub(crate) fn display_user_message(renderer: &mut AnsiRenderer, message: &str) -> Result<()> {
    renderer.line(MessageStyle::User, message)
}
