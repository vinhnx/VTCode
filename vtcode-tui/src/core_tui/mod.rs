use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::config::types::UiSurfacePreference;

pub mod alternate_screen;
pub mod log;
pub mod panic_hook;
pub mod runner;
pub mod session;
pub mod style;
pub mod theme_parser;
pub mod types;
pub mod widgets;

pub use style::{convert_style, theme_from_styles};
pub use theme_parser::ThemeConfigParser;
pub use types::{
    ContentPart, DiffHunk, DiffPreviewState, EditingMode, InlineCommand, InlineEvent,
    InlineEventCallback, InlineHandle, InlineHeaderContext, InlineHeaderHighlight, InlineListItem,
    InlineListSearchConfig, InlineListSelection, InlineMessageKind, InlineSegment, InlineSession,
    InlineTextStyle, InlineTheme, PlanConfirmationResult, PlanContent, PlanPhase, PlanStep,
    SecurePromptConfig, SlashCommandItem, TrustMode, WizardModalMode, WizardStep,
};

use runner::{TuiOptions, run_tui};

pub fn spawn_session(
    theme: InlineTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
    event_callback: Option<InlineEventCallback>,
    active_pty_sessions: Option<Arc<std::sync::atomic::AtomicUsize>>,
    workspace_root: Option<std::path::PathBuf>,
) -> Result<InlineSession> {
    spawn_session_with_prompts(
        theme,
        placeholder,
        surface_preference,
        inline_rows,
        event_callback,
        active_pty_sessions,
        crate::config::KeyboardProtocolConfig::default(),
        workspace_root,
        Vec::new(),
    )
}

/// Spawn session with optional custom prompts pre-loaded
#[allow(clippy::too_many_arguments)]
pub fn spawn_session_with_prompts(
    theme: InlineTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
    event_callback: Option<InlineEventCallback>,
    active_pty_sessions: Option<Arc<std::sync::atomic::AtomicUsize>>,
    keyboard_protocol: crate::config::KeyboardProtocolConfig,
    workspace_root: Option<std::path::PathBuf>,
    slash_commands: Vec<SlashCommandItem>,
) -> Result<InlineSession> {
    spawn_session_with_prompts_and_options(
        theme,
        placeholder,
        surface_preference,
        inline_rows,
        event_callback,
        active_pty_sessions,
        keyboard_protocol,
        workspace_root,
        slash_commands,
        None,
        "Agent TUI".to_string(),
        None,
    )
}

/// Spawn session with host-injected UI options.
#[allow(clippy::too_many_arguments)]
pub fn spawn_session_with_prompts_and_options(
    theme: InlineTheme,
    placeholder: Option<String>,
    surface_preference: UiSurfacePreference,
    inline_rows: u16,
    event_callback: Option<InlineEventCallback>,
    active_pty_sessions: Option<Arc<std::sync::atomic::AtomicUsize>>,
    keyboard_protocol: crate::config::KeyboardProtocolConfig,
    workspace_root: Option<std::path::PathBuf>,
    slash_commands: Vec<SlashCommandItem>,
    appearance: Option<crate::core_tui::session::config::AppearanceConfig>,
    app_name: String,
    non_interactive_hint: Option<String>,
) -> Result<InlineSession> {
    use crossterm::tty::IsTty;

    // Check stdin is a terminal BEFORE spawning the task
    if !std::io::stdin().is_tty() {
        return Err(anyhow::anyhow!(
            "cannot run interactive TUI: stdin is not a terminal (must be run in an interactive terminal)"
        ));
    }

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
                show_logs: crate::ui::tui::panic_hook::is_debug_mode(),
                log_theme: None,
                event_callback,
                active_pty_sessions,
                keyboard_protocol,
                workspace_root,
                slash_commands,
                appearance,
                app_name: app_name.clone(),
            },
        )
        .await
        {
            let error_msg = error.to_string();
            if error_msg.contains("stdin is not a terminal") {
                eprintln!("Error: Interactive TUI requires a proper terminal.");
                if let Some(hint) = non_interactive_hint.as_deref() {
                    eprintln!("{}", hint);
                } else {
                    eprintln!("Use a non-interactive mode in your host app for piped input.");
                }
            } else {
                eprintln!("Error: TUI startup failed: {:#}", error);
            }
            tracing::error!(%error, "inline session terminated unexpectedly");
        }
    });

    Ok(InlineSession {
        handle: InlineHandle { sender: command_tx },
        events: event_rx,
    })
}
