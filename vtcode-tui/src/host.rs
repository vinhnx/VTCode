use crate::{KeyboardProtocolSettings, SessionSurface};

/// Provides high-level workspace metadata for header rendering.
pub trait WorkspaceInfoProvider {
    fn workspace_name(&self) -> String;
    fn workspace_root(&self) -> Option<std::path::PathBuf>;
}

/// Provides notification hooks for terminal focus changes.
pub trait NotificationProvider {
    fn set_terminal_focused(&self, focused: bool);
}

/// Provides theme lookup/synchronization for dynamic UI styling.
pub trait ThemeProvider {
    fn available_themes(&self) -> Vec<String>;
    fn active_theme_name(&self) -> Option<String>;
}

/// Host-level defaults for launching a TUI session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSessionDefaults {
    pub surface_preference: SessionSurface,
    pub inline_rows: u16,
    pub keyboard_protocol: KeyboardProtocolSettings,
}

impl Default for HostSessionDefaults {
    fn default() -> Self {
        Self {
            surface_preference: SessionSurface::default(),
            inline_rows: vtcode_config::constants::ui::DEFAULT_INLINE_VIEWPORT_ROWS,
            keyboard_protocol: KeyboardProtocolSettings::default(),
        }
    }
}

/// Full host adapter contract for embedding `vtcode-tui` in other apps.
pub trait HostAdapter: WorkspaceInfoProvider + NotificationProvider + ThemeProvider {
    /// Provide host-specific defaults for TUI session startup.
    fn session_defaults(&self) -> HostSessionDefaults {
        HostSessionDefaults::default()
    }
}
