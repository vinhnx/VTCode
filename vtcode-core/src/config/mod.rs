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
pub mod router;
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
    AgentConfig, AgentCustomPromptsConfig, AgentOnboardingConfig, AutomationConfig, CommandsConfig,
    FullAutoConfig, PermissionsConfig, PromptCachingConfig, ProviderPromptCachingConfig,
    SecurityConfig, ToolPolicy, ToolsConfig,
};
pub use defaults::{
    ConfigDefaultsProvider, ContextStoreDefaults, PerformanceDefaults, ScenarioDefaults,
    SyntaxHighlightingDefaults, WorkspacePathsDefaults, current_config_defaults,
    install_config_defaults_provider, reset_to_default_config_defaults, with_config_defaults,
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
pub use router::{ComplexityModelMap, HeuristicSettings, ResourceBudget, RouterConfig};
pub use telemetry::TelemetryConfig;
pub use types::{ReasoningEffortLevel, UiSurfacePreference};
pub use validation::{ValidationResult, validate_config, validate_model_exists};
pub use validator::{ConfigValidator, ModelsDatabase, ValidationResult as ConfigValidationResult};
pub use vtcode_config::TimeoutsConfig;
pub use vtcode_config::root::{
    PtyConfig, StatusLineConfig, StatusLineMode, ToolOutputMode, UiConfig,
};
