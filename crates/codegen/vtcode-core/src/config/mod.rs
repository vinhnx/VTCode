//! Configuration facade for vtcode-core.
//!
//! This module re-exports the extracted `vtcode-config` crate so existing
//! call sites continue to access configuration types and helpers through
//! `vtcode_core::config`.

pub mod acp;
pub mod api;
pub mod api_keys;
pub mod consent;
pub mod constants;
pub mod context;
pub mod core;
pub mod defaults;
pub mod hooks;
pub mod loader;
pub mod mcp;
pub mod models;
pub mod telemetry;
pub mod types;
pub mod validation;
pub mod validator;

pub use acp::{
    AgentClientProtocolConfig, AgentClientProtocolTransport, AgentClientProtocolZedConfig,
    AgentClientProtocolZedToolsConfig, AgentClientProtocolZedWorkspaceTrustMode, WorkspaceTrustLevel,
};
pub use api::{
    ConfigLayerView, ConfigReadRequest, ConfigReadResponse, ConfigResetRequest, ConfigResetResponse, ConfigService,
    ConfigWriteRequest, ConfigWriteResponse, ConfigWriteStrategy, ConfigWriteTarget, OverrideMetadata,
};
pub use api_keys::ApiKeySources;
pub use consent::{
    BUNDLED_SKILLS_SETTING_PATH, ConsentAnswer, bundled_skill_missing_hint, bundled_skills_answer,
    bundled_skills_install_permitted, install_location_description,
};
pub use context::{ContextFeaturesConfig, LedgerConfig};
pub use core::{
    AgentConfig, AgentOnboardingConfig, AgentPromptSuggestionsConfig, AutoPermissionConfig,
    AutoPermissionEnvironmentConfig, AutomationConfig, CommandsConfig, EditorToolConfig, FullAutoConfig,
    GatekeeperConfig, ModelConfig, OpenAIPromptCacheKeyMode, PermissionsConfig, PersistentMemoryConfig,
    PromptCachingConfig, ProviderPromptCachingConfig, ScheduledTasksConfig, SecurityConfig, ToolPolicy, ToolProfile,
    ToolsConfig, build_openai_prompt_cache_key, build_session_affinity_prompt_cache_key,
    map_prompt_cache_key_for_provider, session_affinity_key_enabled, session_affinity_provider,
    tool_call_delay_for_rate,
};
pub use core::{PluginRuntimeConfig, PluginTrustLevel};
pub use defaults::{
    ConfigDefaultsProvider, ContextStoreDefaults, PerformanceDefaults, ScenarioDefaults, SyntaxHighlightingDefaults,
    WorkspacePathsDefaults, current_config_defaults, get_config_dir, get_data_dir, install_config_defaults_provider,
    reset_to_default_config_defaults, with_config_defaults,
};
pub use hooks::{
    HookCommandConfig, HookCommandKind, HookGroupConfig, HooksConfig, LifecycleHooksConfig, WorkspaceHookCommand,
    WorkspaceLifecycleHooks,
};
pub use loader::{ConfigManager, SyntaxHighlightingConfig, VTCodeConfig};
pub use mcp::{
    McpAllowListConfig, McpAllowListRules, McpClientConfig, McpHttpServerConfig, McpProviderConfig,
    McpStdioServerConfig, McpTransportConfig, McpUiConfig, McpUiMode,
};
pub use models::{ModelId, OpenRouterMetadata};
pub use telemetry::TelemetryConfig;
pub use types::{ReasoningEffortLevel, SystemPromptMode, ToolDocumentationMode, UiSurfacePreference, VerbosityLevel};
pub use validation::{ValidationResult, validate_config, validate_model_exists};
pub use validator::ConfigValidator;
pub use vtcode_config::WebmcpConfig;
pub use vtcode_config::root::{
    KeyboardProtocolConfig, KeyboardProtocolMode, LayoutModeOverride, PtyConfig, ReasoningDisplayMode, ToolDisplayMode,
    ToolOutputMode, UiConfig, UiDisplayMode, UiTranscriptReviewConfig,
};
pub use vtcode_config::status_line::{StatusLineConfig, StatusLineMode};
pub use vtcode_config::terminal_title::effective_title_items;
pub use vtcode_config::terminal_title::{DEFAULT_TERMINAL_TITLE_ITEMS, TerminalTitleConfig};
pub use vtcode_config::{
    FileOpener, HistoryConfig, HistoryPersistence, NotificationCondition, TerminalNotificationMethod,
    TuiAlternateScreen, TuiConfig, TuiNotificationEvent, TuiNotificationsConfig,
};
pub use vtcode_config::{TimeoutsConfig, resolve_timeout};
