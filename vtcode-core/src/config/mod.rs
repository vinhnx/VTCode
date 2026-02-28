//! Configuration facade for vtcode-core.
//!
//! This module re-exports the extracted `vtcode-config` crate so existing
//! call sites continue to access configuration types and helpers through
//! `vtcode_core::config`.

pub mod acp;
pub mod api_keys;
pub mod constants;
pub mod context;
pub mod core;
pub mod defaults;
pub mod hooks;
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
pub use validator::{ConfigValidator, ModelsDatabase, ValidationResult as ConfigValidationResult};
pub use vtcode_config::root::{
    KeyboardProtocolConfig, LayoutModeOverride, PtyConfig, ReasoningDisplayMode, ToolOutputMode,
    UiConfig, UiDisplayMode,
};
pub use vtcode_config::status_line::{StatusLineConfig, StatusLineMode};
pub use vtcode_config::{TimeoutsConfig, resolve_timeout};

/// Convert KeyboardProtocolConfig to KeyboardEnhancementFlags
pub fn keyboard_protocol_to_flags(
    config: &KeyboardProtocolConfig,
) -> ratatui::crossterm::event::KeyboardEnhancementFlags {
    use ratatui::crossterm::event::KeyboardEnhancementFlags;

    if !config.enabled {
        return KeyboardEnhancementFlags::empty();
    }

    match config.mode.as_str() {
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
    }
}

#[cfg(test)]
mod keyboard_protocol_tests {
    use super::*;

    #[test]
    fn test_keyboard_protocol_default_mode() {
        let config = KeyboardProtocolConfig {
            enabled: true,
            mode: "default".to_string(),
            disambiguate_escape_codes: true,
            report_event_types: true,
            report_alternate_keys: true,
            report_all_keys: false,
        };

        let flags = keyboard_protocol_to_flags(&config);
        use ratatui::crossterm::event::KeyboardEnhancementFlags;

        assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
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

        let flags = keyboard_protocol_to_flags(&config);
        use ratatui::crossterm::event::KeyboardEnhancementFlags;

        assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
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

        let flags = keyboard_protocol_to_flags(&config);
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

        let flags = keyboard_protocol_to_flags(&config);
        use ratatui::crossterm::event::KeyboardEnhancementFlags;

        assert!(flags.contains(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(!flags.contains(KeyboardEnhancementFlags::REPORT_EVENT_TYPES));
        assert!(flags.contains(KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS));
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
