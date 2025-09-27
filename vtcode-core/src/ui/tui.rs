use anyhow::Result;
use tokio::sync::mpsc;

use crate::config::types::UiSurfacePreference;

mod session;
mod style;
mod tui;
mod types;

pub use style::{convert_style, theme_from_styles};
pub use types::{
    InlineCommand, InlineEvent, InlineHandle, InlineMessageKind, InlineSegment, InlineSession,
    InlineTextStyle, InlineTheme,
};

use tui::run_tui;

pub fn spawn_session(
    theme: InlineTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
) -> Result<InlineSession> {
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
            tracing::error!(%error, "inline session terminated unexpectedly");
        }
    });

    Ok(InlineSession {
        handle: InlineHandle { sender: command_tx },
        events: event_rx,
    })
}
