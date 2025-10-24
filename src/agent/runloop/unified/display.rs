use anyhow::Result;

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
    Ok(())
}

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
    renderer.line(MessageStyle::User, message)?;
    renderer.line(MessageStyle::Output, "")?;
    Ok(())
}
