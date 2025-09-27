use anyhow::Result;
use tokio::sync::mpsc;

use crate::config::types::UiSurfacePreference;

mod action;
mod prompt;
mod session;
mod style;
mod transcript;
mod tui;
mod types;

pub use style::{convert_style, parse_tui_color, theme_from_styles};
pub use types::{
    RatatuiCommand, RatatuiEvent, RatatuiHandle, RatatuiMessageKind, RatatuiSegment,
    RatatuiSession, RatatuiTextStyle, RatatuiTheme,
};

use tui::run_tui;

pub fn spawn_session(
    theme: RatatuiTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
) -> Result<RatatuiSession> {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        if let Err(error) = run_tui(
            command_rx,
            event_tx,
            theme,
            placeholder,
            surface_preference,
            inline_rows,
        )
        .await
        {
            tracing::error!(%error, "ratatui session terminated unexpectedly");
        }
    });

    Ok(RatatuiSession {
        handle: RatatuiHandle { sender: command_tx },
        events: event_rx,
    })
}
