use anyhow::Result;
use tokio::sync::mpsc;

use crate::config::types::UiSurfacePreference;

mod session;
mod style;
mod tui;
mod types;

pub use style::{convert_style, theme_from_styles};
pub use types::{
    InlineCommand, InlineEvent, InlineHandle, InlineHeaderContext, InlineHeaderHighlight,
    InlineListItem, InlineListSelection, InlineMessageKind, InlineSegment, InlineSession,
    InlineTextStyle, InlineTheme, SecurePromptConfig,
};

use tui::run_tui;

pub fn spawn_session(
    theme: InlineTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
    show_timeline_pane: bool,
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
            show_timeline_pane,
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
