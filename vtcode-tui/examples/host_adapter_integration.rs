use std::path::PathBuf;

use vtcode_tui::host::{
    HostAdapter, HostSessionDefaults, NotificationProvider, ThemeProvider, WorkspaceInfoProvider,
};
use vtcode_tui::{InlineTheme, SessionSurface, spawn_session_with_host};

struct DemoHost;

impl WorkspaceInfoProvider for DemoHost {
    fn workspace_name(&self) -> String {
        "demo-workspace".to_string()
    }

    fn workspace_root(&self) -> Option<PathBuf> {
        std::env::current_dir().ok()
    }
}

impl NotificationProvider for DemoHost {
    fn set_terminal_focused(&self, _focused: bool) {}
}

impl ThemeProvider for DemoHost {
    fn available_themes(&self) -> Vec<String> {
        vec!["default".to_string(), "catppuccin".to_string()]
    }

    fn active_theme_name(&self) -> Option<String> {
        Some("default".to_string())
    }
}

impl HostAdapter for DemoHost {
    fn session_defaults(&self) -> HostSessionDefaults {
        HostSessionDefaults {
            surface_preference: SessionSurface::Auto,
            inline_rows: 20,
            keyboard_protocol: Default::default(),
        }
    }
}

fn main() {
    let host = DemoHost;
    let _ = host.workspace_name();
    let _ = host.workspace_root();
    let _ = host.available_themes();
    host.set_terminal_focused(true);

    // Keep this example non-interactive in CI.
    if std::env::var("VTCODE_TUI_RUN_EXAMPLES").is_ok() {
        let _ = spawn_session_with_host(InlineTheme::default(), &host);
    }
}
