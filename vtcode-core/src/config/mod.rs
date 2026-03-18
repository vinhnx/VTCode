//! Configuration facade for vtcode-core.
//!
//! This module re-exports the extracted `vtcode-config` crate so existing
//! call sites continue to access configuration types and helpers through
//! `vtcode_core::config`.

pub mod acp;
pub mod api;
pub mod api_keys;
pub mod constants;
pub mod context;
pub mod core;
pub mod defaults;
pub mod hooks;
pub mod ide_context;
pub mod loader;
pub mod mcp;
pub mod models;
pub mod output_styles;
pub mod telemetry;
pub mod types;
pub mod validation;
pub mod validator;

pub use acp::{
    AgentClientProtocolConfig, AgentClientProtocolTransport, AgentClientProtocolZedConfig,
    AgentClientProtocolZedToolsConfig, AgentClientProtocolZedWorkspaceTrustMode,
    WorkspaceTrustLevel,
};
pub use api::{
    ConfigLayerView, ConfigReadRequest, ConfigReadResponse, ConfigService, ConfigWriteRequest,
    ConfigWriteResponse, ConfigWriteStrategy, ConfigWriteTarget, OverrideMetadata,
};
pub use api_keys::ApiKeySources;
pub use context::{ContextFeaturesConfig, LedgerConfig};
pub use core::{
    AgentConfig, AgentOnboardingConfig, AutomationConfig, CommandsConfig, EditorToolConfig,
    FullAutoConfig, GatekeeperConfig, ModelConfig, OpenAIPromptCacheKeyMode, PermissionsConfig,
    PromptCachingConfig, ProviderPromptCachingConfig, SecurityConfig, ToolPolicy, ToolsConfig,
};
pub use core::{PluginRuntimeConfig, PluginTrustLevel};
pub use defaults::{
    ConfigDefaultsProvider, ContextStoreDefaults, PerformanceDefaults, ScenarioDefaults,
    SyntaxHighlightingDefaults, WorkspacePathsDefaults, current_config_defaults, get_config_dir,
    get_data_dir, install_config_defaults_provider, reset_to_default_config_defaults,
    with_config_defaults,
};
pub use hooks::{
    HookCommandConfig, HookCommandKind, HookGroupConfig, HooksConfig, LifecycleHooksConfig,
};
pub use ide_context::{
    IdeContextConfig, IdeContextProviderConfig, IdeContextProviderFamily, IdeContextProviderMode,
    IdeContextProvidersConfig,
};
pub use loader::{ConfigManager, SyntaxHighlightingConfig, VTCodeConfig};
pub use mcp::{
    McpAllowListConfig, McpAllowListRules, McpClientConfig, McpHttpServerConfig, McpProviderConfig,
    McpStdioServerConfig, McpTransportConfig, McpUiConfig, McpUiMode,
};
pub use models::{ModelId, OpenRouterMetadata};
pub use telemetry::TelemetryConfig;
pub use types::{
    ReasoningEffortLevel, SystemPromptMode, ToolDocumentationMode, UiSurfacePreference,
    VerbosityLevel,
};
pub use validation::{ValidationResult, validate_config, validate_model_exists};
pub use validator::ConfigValidator;
pub use vtcode_config::root::{
    KeyboardProtocolConfig, LayoutModeOverride, PtyConfig, ReasoningDisplayMode, ToolOutputMode,
    UiConfig, UiDisplayMode,
};
pub use vtcode_config::status_line::{StatusLineConfig, StatusLineMode};
pub use vtcode_config::{
    FileOpener, HistoryConfig, HistoryPersistence, TerminalNotificationMethod, TuiAlternateScreen,
    TuiConfig, TuiNotificationEvent, TuiNotificationsConfig,
};
pub use vtcode_config::{TimeoutsConfig, resolve_timeout};

/// Convert KeyboardProtocolConfig to KeyboardEnhancementFlags
pub fn keyboard_protocol_to_flags(
    config: &KeyboardProtocolConfig,
) -> crossterm::event::KeyboardEnhancementFlags {
    keyboard_protocol_to_flags_for_terminal(
        config,
        cfg!(target_os = "macos"),
        std::env::var("TERM_PROGRAM").ok().as_deref(),
        std::env::var("TERM").ok().as_deref(),
    )
}

fn keyboard_protocol_to_flags_for_terminal(
    config: &KeyboardProtocolConfig,
    is_macos: bool,
    term_program: Option<&str>,
    term: Option<&str>,
) -> crossterm::event::KeyboardEnhancementFlags {
    use ratatui::crossterm::event::KeyboardEnhancementFlags;

    if !config.enabled {
        return KeyboardEnhancementFlags::empty();
    }

    let mut flags = match config.mode.as_str() {
        "default" => {
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
        }
        "full" => {
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
        }
        "minimal" => KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
        "custom" => {
            let mut flags = KeyboardEnhancementFlags::empty();
            if config.disambiguate_escape_codes {
                flags |= KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
            }
            if config.report_event_types {
                flags |= KeyboardEnhancementFlags::REPORT_EVENT_TYPES;
            }
            if config.report_alternate_keys {
                flags |= KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS;
            }
            if config.report_all_keys {
                flags |= KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
            }
            flags
        }
        _ => {
            tracing::warn!(
                "Invalid keyboard protocol mode '{}', using default",
                config.mode
            );
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
        }
    };

    if should_force_report_all_keys(config.mode.as_str(), is_macos, term_program, term) {
        flags |= KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
    }

    flags
}

fn should_force_report_all_keys(
    mode: &str,
    is_macos: bool,
    term_program: Option<&str>,
    term: Option<&str>,
) -> bool {
    if !is_macos || !matches!(mode, "default") {
        return false;
    }

    // Ghostty on macOS needs "report all keys" enabled so bare Command presses
    // surface as modifier-key events that transcript link clicks can merge in.
    terminal_name_contains(term_program, "ghostty") || terminal_name_contains(term, "ghostty")
}

fn terminal_name_contains(value: Option<&str>, needle: &str) -> bool {
    value
        .map(|value| value.to_ascii_lowercase().contains(needle))
        .unwrap_or(false)
}

#[cfg(test)]
mod keyboard_protocol_tests {
    use super::*;
    use ratatui::crossterm::event::KeyboardEnhancementFlags;

    fn default_keyboard_protocol_config() -> KeyboardProtocolConfig {
        KeyboardProtocolConfig {
            enabled: true,
            mode: "default".to_string(),
            disambiguate_escape_codes: true,
            report_event_types: true,
            report_alternate_keys: true,
            report_all_keys: false,
        }
    }

    #[test]
    fn test_keyboard_protocol_default_mode() {
        let flags = keyboard_protocol_to_flags_for_terminal(
            &default_keyboard_protocol_config(),
            false,
            Some("Ghostty"),
            Some("xterm-ghostty"),
        );

        assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
    }

    #[test]
    fn test_keyboard_protocol_default_mode_enables_all_keys_for_ghostty_on_macos() {
        let flags = keyboard_protocol_to_flags_for_terminal(
            &default_keyboard_protocol_config(),
            true,
            Some("Ghostty"),
            Some("xterm-ghostty"),
        );

        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
    }

    #[test]
    fn test_keyboard_protocol_minimal_mode() {
        let config = KeyboardProtocolConfig {
            enabled: true,
            mode: "minimal".to_string(),
            disambiguate_escape_codes: true,
            report_event_types: true,
            report_alternate_keys: true,
            report_all_keys: false,
        };

        let flags = keyboard_protocol_to_flags_for_terminal(
            &config,
            true,
            Some("Ghostty"),
            Some("xterm-ghostty"),
        );

        assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
    }

    #[test]
    fn test_keyboard_protocol_disabled() {
        let config = KeyboardProtocolConfig {
            enabled: false,
            mode: "default".to_string(),
            disambiguate_escape_codes: true,
            report_event_types: true,
            report_alternate_keys: true,
            report_all_keys: false,
        };

        let flags = keyboard_protocol_to_flags_for_terminal(
            &config,
            true,
            Some("Ghostty"),
            Some("xterm-ghostty"),
        );
        assert!(flags.is_empty());
    }

    #[test]
    fn test_keyboard_protocol_custom_mode() {
        let config = KeyboardProtocolConfig {
            enabled: true,
            mode: "custom".to_string(),
            disambiguate_escape_codes: true,
            report_event_types: false,
            report_alternate_keys: true,
            report_all_keys: false,
        };

        let flags = keyboard_protocol_to_flags_for_terminal(
            &config,
            true,
            Some("Ghostty"),
            Some("xterm-ghostty"),
        );

        assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
    }

    #[test]
    fn test_keyboard_protocol_validation() {
        let mut config = KeyboardProtocolConfig {
            enabled: true,
            mode: "invalid".to_string(),
            disambiguate_escape_codes: true,
            report_event_types: true,
            report_alternate_keys: true,
            report_all_keys: false,
        };

        assert!(config.validate().is_err());

        config.mode = "default".to_string();
        assert!(config.validate().is_ok());
    }
}
