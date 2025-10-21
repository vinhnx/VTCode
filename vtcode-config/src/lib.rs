//! Shared configuration loader utilities for VTCode and downstream integrations.
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
pub mod constants;
pub mod context;
pub mod core;
pub mod defaults;
pub mod loader;
pub mod mcp;
pub mod models;
pub mod root;
pub mod router;
#[cfg(feature = "schema")]
pub mod schema;
pub mod telemetry;
pub mod types;

pub use acp::{
    AgentClientProtocolConfig, AgentClientProtocolTransport, AgentClientProtocolZedConfig,
    AgentClientProtocolZedToolsConfig, AgentClientProtocolZedWorkspaceTrustMode,
    WorkspaceTrustLevel,
};
pub use api_keys::ApiKeySources;
pub use context::{ContextFeaturesConfig, LedgerConfig};
pub use core::{
    AgentConfig, AgentCustomPromptsConfig, AgentOnboardingConfig, AutomationConfig, CommandsConfig,
    FullAutoConfig, PromptCachingConfig, ProviderPromptCachingConfig, SecurityConfig, ToolPolicy,
    ToolsConfig,
};
pub use defaults::{
    ConfigDefaultsProvider, ContextStoreDefaults, PerformanceDefaults, ScenarioDefaults,
    SyntaxHighlightingDefaults, WorkspacePathsDefaults, current_config_defaults,
    install_config_defaults_provider, reset_to_default_config_defaults, with_config_defaults,
};
pub use loader::{ConfigManager, SyntaxHighlightingConfig, VTCodeConfig};
pub use mcp::{
    McpAllowListConfig, McpAllowListRules, McpClientConfig, McpHttpServerConfig, McpProviderConfig,
    McpStdioServerConfig, McpTransportConfig, McpUiConfig, McpUiMode,
};
pub use models::{ModelId, OpenRouterMetadata};
pub use root::{PtyConfig, StatusLineConfig, StatusLineMode, ToolOutputMode, UiConfig};
pub use router::{ComplexityModelMap, HeuristicSettings, ResourceBudget, RouterConfig};
#[cfg(feature = "schema")]
pub use schema::{vtcode_config_schema, vtcode_config_schema_json, vtcode_config_schema_pretty};
pub use telemetry::TelemetryConfig;
pub use types::{ReasoningEffortLevel, UiSurfacePreference};
