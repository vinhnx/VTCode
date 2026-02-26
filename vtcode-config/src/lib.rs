//! Shared configuration loader utilities for VT Code and downstream integrations.
//!
//! This crate exposes [`VTCodeConfig`] and [`ConfigManager`] for reading and
//! validating `vtcode.toml` files while allowing applications to customize the
//! filesystem layout via [`ConfigDefaultsProvider`]. Consumers can opt into the
//! [`bootstrap`](index.html#features) feature (enabled by default) to scaffold
//! configuration directories with project-specific defaults.
//! Disable default features when you only need parsing/validation to omit the
//! filesystem bootstrap helpers and reduce dependencies.
//!
//! # Examples
//! ```no_run
//! use vtcode_config::ConfigManager;
//!
//! # fn main() -> anyhow::Result<()> {
//! let manager = ConfigManager::load_from_workspace(".")?;
//! println!("Active provider: {}", manager.config().agent.provider);
//! # Ok(())
//! # }
//! ```
//!
//! Install a custom [`ConfigDefaultsProvider`] with
//! [`install_config_defaults_provider`] when you need to override search paths
//! or syntax highlighting defaults exposed by the loader.

pub mod acp;
pub mod api_keys;
pub mod auth;
pub mod constants;
pub mod context;
pub mod core;
pub mod debug;
pub mod defaults;
pub mod hooks;
pub mod loader;
pub mod mcp;
pub mod models;
pub mod optimization;
pub mod output_styles;
pub mod root;
#[cfg(feature = "schema")]
pub mod schema;
pub mod status_line;
pub mod subagent;
pub mod telemetry;
pub mod timeouts;
pub mod types;

pub use acp::{
    AgentClientProtocolConfig, AgentClientProtocolTransport, AgentClientProtocolZedConfig,
    AgentClientProtocolZedToolsConfig, AgentClientProtocolZedWorkspaceTrustMode,
    WorkspaceTrustLevel,
};
pub use agent_teams::{AgentTeamsConfig, TeammateMode};
pub use api_keys::ApiKeySources;
pub use context::{ContextFeaturesConfig, DynamicContextConfig, LedgerConfig};
pub use core::{
    AgentConfig, AgentOnboardingConfig, AuthConfig, AutomationConfig, CommandsConfig,
    DockerSandboxConfig, ExternalSandboxConfig, ExternalSandboxType, FullAutoConfig,
    GatekeeperConfig, MicroVMSandboxConfig, ModelConfig, NetworkAllowlistEntryConfig,
    NetworkConfig, OpenResponsesConfig, OpenRouterAuthConfig, PluginRuntimeConfig,
    PluginTrustLevel, PromptCachingConfig, ProviderPromptCachingConfig, ResourceLimitsConfig,
    ResourceLimitsPreset, SandboxConfig, SandboxMode, SeccompConfig, SeccompProfilePreset,
    SecurityConfig, SensitivePathsConfig, SkillsConfig, SkillsRenderMode, ToolPolicy, ToolsConfig,
    WebFetchConfig,
};
pub use debug::{DebugConfig, TraceLevel};
pub use defaults::{
    ConfigDefaultsProvider, ContextStoreDefaults, PerformanceDefaults, ScenarioDefaults,
    SyntaxHighlightingDefaults, WorkspacePathsDefaults, current_config_defaults, get_config_dir,
    get_data_dir, install_config_defaults_provider, reset_to_default_config_defaults,
    with_config_defaults,
};
pub use hooks::{
    HookCommandConfig, HookCommandKind, HookGroupConfig, HooksConfig, LifecycleHooksConfig,
};
pub use loader::layers::{ConfigLayerEntry, ConfigLayerSource, ConfigLayerStack};
pub use loader::{
    ConfigBuilder, ConfigManager, SyntaxHighlightingConfig, VTCodeConfig, merge_toml_values,
};
pub use mcp::{
    McpAllowListConfig, McpAllowListRules, McpClientConfig, McpHttpServerConfig, McpProviderConfig,
    McpStdioServerConfig, McpTransportConfig, McpUiConfig, McpUiMode,
};
pub use models::{ModelId, OpenRouterMetadata};
pub use optimization::{
    AgentExecutionConfig, AsyncPipelineConfig, CommandCacheConfig, FileReadCacheConfig,
    LLMClientConfig, MemoryPoolConfig, OptimizationConfig, ProfilingConfig, ToolRegistryConfig,
};
pub use output_styles::{OutputStyle, OutputStyleConfig, OutputStyleManager};
pub use root::{
    AskQuestionsConfig, ChatConfig, LayoutModeOverride, NotificationDeliveryMode, PtyConfig,
    ToolOutputMode, UiConfig, UiDisplayMode, UiNotificationsConfig,
};
#[cfg(feature = "schema")]
pub use schema::{vtcode_config_schema, vtcode_config_schema_json, vtcode_config_schema_pretty};
pub use status_line::{StatusLineConfig, StatusLineMode};
pub use subagent::{
    SubagentConfig, SubagentModel, SubagentParseError, SubagentPermissionMode, SubagentSource,
    SubagentsConfig,
};
pub use telemetry::TelemetryConfig;
pub use timeouts::{TimeoutsConfig, resolve_timeout};
pub use types::{
    EditingMode, ReasoningEffortLevel, SystemPromptMode, ToolDocumentationMode,
    UiSurfacePreference, VerbosityLevel,
};

// Re-export auth module types
pub use auth::{
    AuthStatus, OpenRouterOAuthConfig, OpenRouterToken, PkceChallenge, clear_oauth_token,
    generate_pkce_challenge, get_auth_status, get_auth_url, load_oauth_token, save_oauth_token,
};
pub mod agent_teams;
