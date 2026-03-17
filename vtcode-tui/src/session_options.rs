use std::path::PathBuf;
use std::sync::Arc;

use crate::UiSurfacePreference;
use crate::config::KeyboardProtocolConfig;
use crate::core_tui::app::session::AppSession;
use crate::core_tui::app::types::{
    FocusChangeCallback, InlineEventCallback, InlineSession, InlineTheme, SlashCommandItem,
};
use crate::core_tui::log;
use crate::core_tui::runner::{TuiOptions, run_tui};
use crate::core_tui::session::config::AppearanceConfig;
use crate::options::{KeyboardProtocolSettings, SessionSurface};

/// Standalone session launch options for reusable integrations.
#[derive(Clone)]
pub struct SessionOptions {
    pub placeholder: Option<String>,
    pub surface_preference: SessionSurface,
    pub inline_rows: u16,
    pub event_callback: Option<InlineEventCallback>,
    pub focus_callback: Option<FocusChangeCallback>,
    pub active_pty_sessions: Option<Arc<std::sync::atomic::AtomicUsize>>,
    pub input_activity_counter: Option<Arc<std::sync::atomic::AtomicU64>>,
    pub keyboard_protocol: KeyboardProtocolSettings,
    pub workspace_root: Option<PathBuf>,
    pub slash_commands: Vec<SlashCommandItem>,
    pub appearance: Option<AppearanceConfig>,
    pub app_name: String,
    pub non_interactive_hint: Option<String>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            placeholder: None,
            surface_preference: SessionSurface::Auto,
            inline_rows: crate::config::constants::ui::DEFAULT_INLINE_VIEWPORT_ROWS,
            event_callback: None,
            focus_callback: None,
            active_pty_sessions: None,
            input_activity_counter: None,
            keyboard_protocol: KeyboardProtocolSettings::default(),
            workspace_root: None,
            slash_commands: Vec::new(),
            appearance: None,
            app_name: "Agent TUI".to_string(),
            non_interactive_hint: None,
        }
    }
}

impl SessionOptions {
    /// Build options from a host adapter's defaults.
    pub fn from_host(host: &impl crate::host::HostAdapter) -> Self {
        let defaults = host.session_defaults();
        Self {
            surface_preference: defaults.surface_preference,
            inline_rows: defaults.inline_rows,
            keyboard_protocol: defaults.keyboard_protocol,
            workspace_root: host.workspace_root(),
            slash_commands: host.slash_commands(),
            app_name: host.app_name(),
            non_interactive_hint: host.non_interactive_hint(),
            ..Self::default()
        }
    }
}

/// Spawn a session using standalone options and local config types.
pub fn spawn_session_with_options(
    theme: InlineTheme,
    options: SessionOptions,
) -> anyhow::Result<InlineSession> {
    use crossterm::tty::IsTty;

    // Check stdin is a terminal BEFORE spawning the task
    if !std::io::stdin().is_tty() {
        return Err(anyhow::anyhow!(
            "cannot run interactive TUI: stdin is not a terminal (must be run in an interactive terminal)"
        ));
    }

    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let show_logs = log::is_tui_log_capture_enabled();

    tokio::spawn(async move {
        if let Err(error) = run_tui(
            command_rx,
            event_tx,
            TuiOptions {
                surface_preference: UiSurfacePreference::from(options.surface_preference),
                inline_rows: options.inline_rows,
                show_logs,
                log_theme: None,
                event_callback: options.event_callback,
                focus_callback: options.focus_callback,
                active_pty_sessions: options.active_pty_sessions,
                input_activity_counter: options.input_activity_counter,
                keyboard_protocol: KeyboardProtocolConfig::from(options.keyboard_protocol),
                workspace_root: options.workspace_root,
            },
            move |rows| {
                AppSession::new_with_logs(
                    theme,
                    options.placeholder,
                    rows,
                    show_logs,
                    options.appearance,
                    options.slash_commands,
                    options.app_name,
                )
            },
        )
        .await
        {
            let error_msg = error.to_string();
            if error_msg.contains("stdin is not a terminal") {
                eprintln!("Error: Interactive TUI requires a proper terminal.");
                if let Some(hint) = options.non_interactive_hint.as_deref() {
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
        handle: crate::core_tui::app::types::InlineHandle { sender: command_tx },
        events: event_rx,
    })
}

/// Spawn a session using defaults from a host adapter.
pub fn spawn_session_with_host(
    theme: InlineTheme,
    host: &impl crate::host::HostAdapter,
) -> anyhow::Result<InlineSession> {
    spawn_session_with_options(theme, SessionOptions::from_host(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DemoHost;

    impl crate::host::WorkspaceInfoProvider for DemoHost {
        fn workspace_name(&self) -> String {
            "demo".to_string()
        }

        fn workspace_root(&self) -> Option<PathBuf> {
            Some(PathBuf::from("/workspace/demo"))
        }
    }

    impl crate::host::NotificationProvider for DemoHost {
        fn set_terminal_focused(&self, _focused: bool) {}
    }

    impl crate::host::ThemeProvider for DemoHost {
        fn available_themes(&self) -> Vec<String> {
            vec!["default".to_string()]
        }

        fn active_theme_name(&self) -> Option<String> {
            Some("default".to_string())
        }
    }

    impl crate::host::HostAdapter for DemoHost {
        fn session_defaults(&self) -> crate::host::HostSessionDefaults {
            crate::host::HostSessionDefaults {
                surface_preference: SessionSurface::Inline,
                inline_rows: 24,
                keyboard_protocol: KeyboardProtocolSettings::default(),
            }
        }
    }

    // SessionOptions behavior tests.

    #[test]
    fn session_options_from_host_uses_defaults() {
        let options = SessionOptions::from_host(&DemoHost);

        assert_eq!(options.surface_preference, SessionSurface::Inline);
        assert_eq!(options.inline_rows, 24);
        assert_eq!(
            options.workspace_root,
            Some(PathBuf::from("/workspace/demo"))
        );
    }
}
