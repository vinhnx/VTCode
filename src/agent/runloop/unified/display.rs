use anyhow::Result;

use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::update_theme_preference;

pub(crate) fn persist_theme_preference(renderer: &mut AnsiRenderer, theme_id: &str) -> Result<()> {
    if let Err(err) = update_theme_preference(theme_id) {
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
        renderer.line(MessageStyle::Output, "")?;
        *applied = true;
    }
    Ok(())
}
