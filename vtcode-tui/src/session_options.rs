use std::path::PathBuf;
use std::sync::Arc;

use crate::config::KeyboardProtocolConfig;
use crate::core_tui::session::config::AppearanceConfig;

use crate::{InlineEventCallback, InlineSession, InlineTheme, SlashCommandItem};

/// Standalone surface preference for selecting inline vs alternate rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionSurface {
    #[default]
    Auto,
    Alternate,
    Inline,
}

impl From<SessionSurface> for crate::config::UiSurfacePreference {
    fn from(value: SessionSurface) -> Self {
        match value {
            SessionSurface::Auto => Self::Auto,
            SessionSurface::Alternate => Self::Alternate,
            SessionSurface::Inline => Self::Inline,
        }
    }
}

impl From<crate::config::UiSurfacePreference> for SessionSurface {
    fn from(value: crate::config::UiSurfacePreference) -> Self {
        match value {
            crate::config::UiSurfacePreference::Auto => Self::Auto,
            crate::config::UiSurfacePreference::Alternate => Self::Alternate,
            crate::config::UiSurfacePreference::Inline => Self::Inline,
        }
    }
}

/// Standalone keyboard protocol settings for terminal key event enhancements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardProtocolSettings {
    pub enabled: bool,
    pub mode: String,
    pub disambiguate_escape_codes: bool,
    pub report_event_types: bool,
    pub report_alternate_keys: bool,
    pub report_all_keys: bool,
}

impl Default for KeyboardProtocolSettings {
    fn default() -> Self {
        Self::from(KeyboardProtocolConfig::default())
    }
}

impl From<KeyboardProtocolConfig> for KeyboardProtocolSettings {
    fn from(value: KeyboardProtocolConfig) -> Self {
        Self {
            enabled: value.enabled,
            mode: value.mode,
            disambiguate_escape_codes: value.disambiguate_escape_codes,
            report_event_types: value.report_event_types,
            report_alternate_keys: value.report_alternate_keys,
            report_all_keys: value.report_all_keys,
        }
    }
}

impl From<KeyboardProtocolSettings> for KeyboardProtocolConfig {
    fn from(value: KeyboardProtocolSettings) -> Self {
        Self {
            enabled: value.enabled,
            mode: value.mode,
            disambiguate_escape_codes: value.disambiguate_escape_codes,
            report_event_types: value.report_event_types,
            report_alternate_keys: value.report_alternate_keys,
            report_all_keys: value.report_all_keys,
        }
    }
}

/// Standalone session launch options for reusable integrations.
#[derive(Clone)]
pub struct SessionOptions {
    pub placeholder: Option<String>,
    pub surface_preference: SessionSurface,
    pub inline_rows: u16,
    pub event_callback: Option<InlineEventCallback>,
    pub active_pty_sessions: Option<Arc<std::sync::atomic::AtomicUsize>>,
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
            active_pty_sessions: None,
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
    crate::core_tui::spawn_session_with_prompts_and_options(
        theme,
        options.placeholder,
        options.surface_preference.into(),
        options.inline_rows,
        options.event_callback,
        options.active_pty_sessions,
        options.keyboard_protocol.into(),
        options.workspace_root,
        options.slash_commands,
        options.appearance,
        options.app_name,
        options.non_interactive_hint,
    )
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

    #[test]
    fn session_surface_conversion_roundtrip() {
        let variants = [
            SessionSurface::Auto,
            SessionSurface::Alternate,
            SessionSurface::Inline,
        ];

        for variant in variants {
            let converted: crate::config::UiSurfacePreference = variant.into();
            let roundtrip = SessionSurface::from(converted);
            assert_eq!(variant, roundtrip);
        }
    }

    #[test]
    fn keyboard_protocol_conversion_roundtrip() {
        let settings = KeyboardProtocolSettings {
            enabled: true,
            mode: "custom".to_string(),
            disambiguate_escape_codes: true,
            report_event_types: false,
            report_alternate_keys: true,
            report_all_keys: false,
        };

        let config: KeyboardProtocolConfig = settings.clone().into();
        let restored = KeyboardProtocolSettings::from(config);

        assert_eq!(settings, restored);
    }

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
