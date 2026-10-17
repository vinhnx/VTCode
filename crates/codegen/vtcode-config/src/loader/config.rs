use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr;

use crate::acp::AgentClientProtocolConfig;
use crate::codex::{FileOpener, HistoryConfig, TuiConfig};
use crate::constants::defaults::DEFAULT_PRIMARY_AGENT_NAME;
use crate::context::ContextFeaturesConfig;
use crate::core::{
    AgentConfig, AgentPermissionsConfig, AnthropicConfig, AuthConfig, AutomationConfig, CommandsConfig,
    CustomProviderConfig, DotfileProtectionConfig, ModelConfig, OpenAIConfig, OpenRouterConfig, PermissionsConfig,
    PromptCachingConfig, ProviderOverrideConfig, SandboxConfig, SecurityConfig, SkillsConfig, ToolsConfig,
};
use crate::debug::DebugConfig;
use crate::hooks::HooksConfig;
use crate::mcp::McpClientConfig;
use crate::models::{MiMoAuthMethod, Provider};
use crate::optimization::OptimizationConfig;
use crate::output_styles::OutputStyleConfig;
use crate::root::{ChatConfig, PtyConfig, UiConfig};
use crate::subagents::SubagentRuntimeLimits;
use crate::telemetry::TelemetryConfig;
use crate::timeouts::TimeoutsConfig;
use crate::webmcp::WebmcpConfig;

use crate::loader::syntax_highlighting::SyntaxHighlightingConfig;

/// Provider-specific configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ProviderConfig {
    /// OpenAI provider configuration
    #[serde(default)]
    pub openai: OpenAIConfig,

    /// Anthropic provider configuration
    #[serde(default)]
    pub anthropic: AnthropicConfig,

    /// OpenRouter provider configuration
    #[serde(default)]
    pub openrouter: OpenRouterConfig,

    /// Xiaomi MiMo auth method: "payg" or "token-plan"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mimo_auth_method: Option<MiMoAuthMethod>,
}

/// Codex-compatible top-level feature flags.
///
/// Maps to `[features]` in `vtcode.toml`.
/// When `memories` is true, VT Code can carry useful context from earlier
/// threads into future work via the persistent memory subsystem.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct FeaturesConfig {
    /// Master toggle for the memories subsystem.
    /// When true, VT Code extracts durable context from completed threads
    /// and injects it into future sessions.
    #[serde(default)]
    pub memories: bool,
}

/// Workspace-level configuration controls.
///
/// Maps to `[workspace]` in `vtcode.toml`.
/// When `use_root_config` is true, only the workspace root `vtcode.toml`
/// is used as the active config layer (system, user, project, and
/// dot-dir layers are discarded).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WorkspaceConfig {
    /// When true, force the workspace root `vtcode.toml` as the sole
    /// active config layer, discarding system, user, project, and
    /// dot-dir layers.
    #[serde(default)]
    pub(crate) use_root_config: bool,

    /// Include workspace context in messages.
    #[serde(default = "default_true")]
    pub(crate) include_context: bool,

    /// Maximum size of workspace context to include (in bytes).
    #[serde(default)]
    pub(crate) max_context_size: Option<usize>,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            use_root_config: false,
            include_context: true,
            max_context_size: None,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Main configuration structure for VT Code
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VTCodeConfig {
    /// Primary agent selected at startup when no session override is active.
    #[serde(default = "default_primary_agent")]
    pub default_primary_agent: String,

    /// Codex-compatible top-level feature flags (`[features]` table).
    #[serde(default)]
    pub features: FeaturesConfig,

    /// Codex-compatible clickable citation URI scheme.
    #[serde(default)]
    pub file_opener: FileOpener,

    /// External notification command invoked for supported events.
    #[serde(default)]
    pub notify: Vec<String>,

    /// Codex-compatible local history persistence controls.
    #[serde(default)]
    pub history: HistoryConfig,

    /// Codex-compatible TUI settings.
    #[serde(default)]
    pub tui: TuiConfig,

    /// Agent-wide settings
    #[serde(default)]
    pub agent: AgentConfig,

    /// Authentication configuration for OAuth flows
    #[serde(default)]
    pub auth: AuthConfig,

    /// Tool execution policies
    #[serde(default)]
    pub tools: ToolsConfig,

    /// Unix command permissions
    #[serde(default)]
    pub commands: CommandsConfig,

    /// Permission system settings (resolution, audit logging, caching)
    #[serde(default)]
    pub permissions: PermissionsConfig,

    /// Runtime-only agent permission policy supplied by derived child agents.
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub runtime_agent_permissions: Option<AgentPermissionsConfig>,

    /// Runtime-only snapshot of lifecycle hook commands sourced from
    /// workspace-controlled configuration layers, populated by `ConfigManager`
    /// at load time. Workspace-controlled hooks require explicit user approval
    /// before execution; see [`WorkspaceLifecycleHooks`](crate::hooks::WorkspaceLifecycleHooks).
    #[serde(skip)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    pub workspace_lifecycle_hooks: Option<crate::hooks::WorkspaceLifecycleHooks>,

    /// Security settings
    #[serde(default)]
    pub security: SecurityConfig,

    /// Sandbox settings for command execution isolation
    #[serde(default)]
    pub sandbox: SandboxConfig,

    /// UI settings
    #[serde(default)]
    pub ui: UiConfig,

    /// Chat settings
    #[serde(default)]
    pub chat: ChatConfig,

    /// PTY settings
    #[serde(default)]
    pub pty: PtyConfig,

    /// Debug and tracing settings
    #[serde(default)]
    pub debug: DebugConfig,

    /// Context features (e.g., Decision Ledger)
    #[serde(default)]
    pub context: ContextFeaturesConfig,

    /// Telemetry configuration (logging, trajectory)
    #[serde(default)]
    pub telemetry: TelemetryConfig,

    /// Performance optimization settings
    #[serde(default)]
    pub optimization: OptimizationConfig,

    /// Syntax highlighting configuration
    #[serde(default)]
    pub syntax_highlighting: SyntaxHighlightingConfig,

    /// Timeout ceilings and UI warning thresholds
    #[serde(default)]
    pub timeouts: TimeoutsConfig,

    /// Automation configuration
    #[serde(default)]
    pub automation: AutomationConfig,

    /// Subagent runtime configuration
    #[serde(default)]
    pub subagents: SubagentRuntimeLimits,

    /// Prompt cache configuration (local + provider integration)
    #[serde(default)]
    pub prompt_cache: PromptCachingConfig,

    /// Model Context Protocol configuration
    #[serde(default)]
    pub mcp: McpClientConfig,

    /// Authenticated browser editor bridge configuration
    #[serde(default)]
    pub webmcp: WebmcpConfig,

    /// Agent Client Protocol configuration
    #[serde(default)]
    pub acp: AgentClientProtocolConfig,

    /// Lifecycle hooks configuration
    #[serde(default)]
    pub hooks: HooksConfig,

    /// Model-specific behavior configuration
    #[serde(default)]
    pub model: ModelConfig,

    /// Provider-specific configuration
    #[serde(default)]
    pub provider: ProviderConfig,

    /// Skills system configuration (Agent Skills spec)
    #[serde(default)]
    pub skills: SkillsConfig,

    /// Extra OpenAI-compatible endpoints for the model picker.
    /// Define in user or system config only.
    #[serde(default)]
    pub custom_providers: Vec<CustomProviderConfig>,

    /// Built-in provider overrides for model lists and endpoint configuration.
    ///
    /// Maps provider key (e.g., "opencode-zen", "opencode-go") to override
    /// config that extends the provider's hardcoded model list with custom
    /// models, and optionally overrides the base URL or API key env var.
    #[serde(default)]
    pub provider_overrides: BTreeMap<String, ProviderOverrideConfig>,

    /// Restrict which providers may be used.
    ///
    /// When non-empty, only providers listed here are visible in the model
    /// picker, selectable at first-run, and instantiable at runtime.  Empty
    /// (the default) means all built-in and custom providers are available.
    #[serde(default)]
    pub providers_whitelist: Vec<String>,

    /// Output style configuration
    #[serde(default)]
    pub output_style: OutputStyleConfig,

    /// Dotfile protection configuration
    #[serde(default)]
    pub dotfile_protection: DotfileProtectionConfig,

    /// Workspace-level configuration controls
    #[serde(default)]
    pub workspace: WorkspaceConfig,
}

impl Default for VTCodeConfig {
    fn default() -> Self {
        Self {
            default_primary_agent: default_primary_agent(),
            features: FeaturesConfig::default(),
            file_opener: FileOpener::default(),
            notify: Vec::new(),
            history: HistoryConfig::default(),
            tui: TuiConfig::default(),
            agent: AgentConfig::default(),
            auth: AuthConfig::default(),
            tools: ToolsConfig::default(),
            commands: CommandsConfig::default(),
            permissions: PermissionsConfig::default(),
            runtime_agent_permissions: None,
            workspace_lifecycle_hooks: None,
            security: SecurityConfig::default(),
            sandbox: SandboxConfig::default(),
            ui: UiConfig::default(),
            chat: ChatConfig::default(),
            pty: PtyConfig::default(),
            debug: DebugConfig::default(),
            context: ContextFeaturesConfig::default(),
            telemetry: TelemetryConfig::default(),
            optimization: OptimizationConfig::default(),
            syntax_highlighting: SyntaxHighlightingConfig::default(),
            timeouts: TimeoutsConfig::default(),
            automation: AutomationConfig::default(),
            subagents: SubagentRuntimeLimits::default(),
            prompt_cache: PromptCachingConfig::default(),
            mcp: McpClientConfig::default(),
            webmcp: WebmcpConfig::default(),
            acp: AgentClientProtocolConfig::default(),
            hooks: HooksConfig::default(),
            model: ModelConfig::default(),
            provider: ProviderConfig::default(),
            skills: SkillsConfig::default(),
            custom_providers: Vec::new(),
            provider_overrides: BTreeMap::new(),
            providers_whitelist: Vec::new(),
            output_style: OutputStyleConfig::default(),
            dotfile_protection: DotfileProtectionConfig::default(),
            workspace: WorkspaceConfig::default(),
        }
    }
}

impl VTCodeConfig {
    pub fn validate(&self) -> Result<()> {
        self.syntax_highlighting
            .validate()
            .context("Invalid syntax_highlighting configuration")?;

        self.context.validate().context("Invalid context configuration")?;

        self.hooks.validate().context("Invalid hooks configuration")?;

        self.timeouts.validate().context("Invalid timeouts configuration")?;

        self.prompt_cache.validate().context("Invalid prompt_cache configuration")?;

        self.webmcp.validate().context("Invalid webmcp configuration")?;

        self.agent
            .validate_llm_params()
            .map_err(anyhow::Error::msg)
            .context("Invalid agent configuration")?;

        self.ui
            .keyboard_protocol
            .validate()
            .context("Invalid keyboard_protocol configuration")?;

        self.pty.validate().context("Invalid pty configuration")?;

        // Validate custom providers
        let mut seen_names = std::collections::HashSet::new();
        for cp in &self.custom_providers {
            cp.validate()
                .map_err(|msg| anyhow::anyhow!(msg))
                .context("Invalid custom_providers configuration")?;
            if !seen_names.insert(cp.name.to_lowercase()) {
                anyhow::bail!("custom_providers: duplicate name `{}`", cp.name);
            }
        }

        // Validate provider overrides
        for (provider_name, override_config) in &self.provider_overrides {
            override_config
                .validate(provider_name)
                .map_err(|msg| anyhow::anyhow!(msg))
                .context("Invalid provider_overrides configuration")?;
            // Validate that the provider key matches a known provider
            if Provider::from_str(provider_name).is_err() {
                anyhow::bail!(
                    "provider_overrides: unknown provider `{provider_name}`; \
                     must be one of: gemini, openai, anthropic, copilot, deepseek, meta, \
                     openrouter, ollama, lmstudio, llamacpp, moonshot, zai, minimax, \
                     mimo, mistral, huggingface, opencodezen, opencodego, qwen, \
                     stepfun, evolink, poolside, nvidia, merge-gateway, vercel"
                );
            }
        }

        // Validate providers_whitelist entries
        for entry in &self.providers_whitelist {
            let known = Provider::from_str(entry).is_ok() || self.custom_providers.iter().any(|cp| cp.name == *entry);
            if !known {
                anyhow::bail!(
                    "providers_whitelist: unknown provider `{entry}`; \
                     must be a built-in provider or a name from custom_providers"
                );
            }
        }

        Ok(())
    }

    /// Returns true when the persistent memory subsystem is enabled.
    ///
    /// The top-level `features.memories` flag is the global master switch.
    /// `agent.persistent_memory.enabled` then enables repository-scoped storage.
    #[must_use]
    pub fn persistent_memory_enabled(&self) -> bool {
        self.features.memories && self.agent.persistent_memory.enabled
    }

    /// Returns true when the memories subsystem is enabled for injection.
    ///
    /// Memories are active when the subsystem is enabled and `memories.use_memories`
    /// is true.
    #[must_use]
    pub fn memories_enabled(&self) -> bool {
        self.persistent_memory_enabled() && self.agent.persistent_memory.memories.use_memories
    }

    /// Returns true when completed threads should be stored as memory inputs.
    #[must_use]
    pub fn should_generate_memories(&self) -> bool {
        self.persistent_memory_enabled() && self.agent.persistent_memory.memories.generate_memories
    }

    /// Look up a custom provider by its stable key.
    pub fn custom_provider(&self, name: &str) -> Option<&CustomProviderConfig> {
        let lower = name.to_lowercase();
        self.custom_providers.iter().find(|cp| cp.name.to_lowercase() == lower)
    }

    /// Return the explicitly configured credential key for a provider.
    ///
    /// Custom-provider identities take precedence over built-in provider
    /// overrides. A missing result means callers should use the provider's
    /// built-in default or the agent-wide fallback.
    pub fn configured_api_key_env(&self, name: &str) -> Option<String> {
        if let Some(custom_provider) = self.custom_provider(name) {
            return Some(custom_provider.resolved_api_key_env());
        }

        self.provider_overrides
            .iter()
            .find(|(provider, _)| provider.eq_ignore_ascii_case(name))
            .and_then(|(_, provider_override)| provider_override.api_key_env.clone())
            .filter(|api_key_env| !api_key_env.trim().is_empty())
    }

    /// Get the display name for any provider key, falling back to the raw key
    /// if no custom provider matches.
    pub fn provider_display_name(&self, provider_key: &str) -> String {
        if let Some(cp) = self.custom_provider(provider_key) {
            cp.display_name.clone()
        } else if provider_key.eq_ignore_ascii_case("codex") {
            "Codex".to_string()
        } else if let Ok(p) = FromStr::from_str(provider_key) {
            let p: Provider = p;
            p.label().to_string()
        } else {
            provider_key.to_string()
        }
    }
}

fn default_primary_agent() -> String {
    DEFAULT_PRIMARY_AGENT_NAME.to_string()
}
