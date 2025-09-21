use anyhow::Result;
use anstyle;

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
        renderer.line_if_not_empty(MessageStyle::Output)?;
        *applied = true;
    }
    Ok(())
}

/// Display a user message with soft-gray ANSI styling
pub(crate) fn display_user_message(renderer: &mut AnsiRenderer, message: &str) -> Result<()> {
    // Display the user message with soft-gray styling
    for line in message.lines() {
        renderer.line_with_style(
            anstyle::Style::new().fg_color(Some(anstyle::Color::Rgb(anstyle::RgbColor(128, 128, 128)))),
            line,
        )?;
    }
    // Add a small gap after the user message
    renderer.line(MessageStyle::Output, "")?;
    Ok(())
}
