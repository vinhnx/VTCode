use anyhow::Result;
use tokio::sync::mpsc;

use crate::config::types::UiSurfacePreference;

pub mod alternate_screen;
mod runner;
mod session;
mod style;
mod theme_parser;
mod types;

pub use style::{convert_style, theme_from_styles};
pub use theme_parser::ThemeConfigParser;
pub use types::{
    InlineCommand, InlineEvent, InlineEventCallback, InlineHandle, InlineHeaderContext,
    InlineHeaderHighlight, InlineListItem, InlineListSearchConfig, InlineListSelection,
    InlineMessageKind, InlineSegment, InlineSession, InlineTextStyle, InlineTheme,
    SecurePromptConfig,
};

use runner::{TuiOptions, run_tui};

pub fn spawn_session(
    theme: InlineTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
    show_timeline_pane: bool,
    event_callback: Option<InlineEventCallback>,
) -> Result<InlineSession> {
    spawn_session_with_prompts(
        theme,
        placeholder,
        surface_preference,
        inline_rows,
        show_timeline_pane,
        event_callback,
        None,
    )
}

/// Spawn session with optional custom prompts pre-loaded
pub fn spawn_session_with_prompts(
    theme: InlineTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
    show_timeline_pane: bool,
    event_callback: Option<InlineEventCallback>,
    custom_prompts: Option<crate::prompts::CustomPromptRegistry>,
) -> Result<InlineSession> {
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        if let Err(error) = run_tui(
            command_rx,
            event_tx,
            TuiOptions {
                theme,
                placeholder,
                surface_preference,
                inline_rows,
                show_timeline_pane,
                event_callback,
                custom_prompts,
            },
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
