use serde::{Deserialize, Serialize};

/// Native OpenAI service tier selection.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenAIServiceTier {
    Flex,
    Priority,
}

impl OpenAIServiceTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Flex => "flex",
            Self::Priority => "priority",
        }
    }
}

/// How VT Code should provision OpenAI hosted shell environments.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenAIHostedShellEnvironment {
    #[default]
    ContainerAuto,
    ContainerReference,
}

impl OpenAIHostedShellEnvironment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContainerAuto => "container_auto",
            Self::ContainerReference => "container_reference",
        }
    }
}

impl OpenAIHostedShellEnvironment {
    pub const fn uses_container_reference(self) -> bool {
        matches!(self, Self::ContainerReference)
    }
}

/// Hosted shell network access policy.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenAIHostedShellNetworkPolicyType {
    #[default]
    Disabled,
    Allowlist,
}

impl OpenAIHostedShellNetworkPolicyType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Allowlist => "allowlist",
        }
    }
}

/// Per-domain secret injected by the OpenAI hosted shell runtime.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct OpenAIHostedShellDomainSecret {
    pub domain: String,
    pub name: String,
    pub value: String,
}

impl OpenAIHostedShellDomainSecret {
    pub fn validation_error(&self, index: usize) -> Option<String> {
        let base = format!("provider.openai.hosted_shell.network_policy.domain_secrets[{index}]");

        if self.domain.trim().is_empty() {
            return Some(format!("`{base}.domain` must not be empty when set."));
        }
        if self.name.trim().is_empty() {
            return Some(format!("`{base}.name` must not be empty when set."));
        }
        if self.value.trim().is_empty() {
            return Some(format!("`{base}.value` must not be empty when set."));
        }

        None
    }
}

/// Request-scoped network policy for OpenAI hosted shell environments.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct OpenAIHostedShellNetworkPolicy {
    #[serde(rename = "type", default)]
    pub policy_type: OpenAIHostedShellNetworkPolicyType,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_domains: Vec<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domain_secrets: Vec<OpenAIHostedShellDomainSecret>,
}

impl OpenAIHostedShellNetworkPolicy {
    pub const fn is_allowlist(&self) -> bool {
        matches!(
            self.policy_type,
            OpenAIHostedShellNetworkPolicyType::Allowlist
        )
    }

    pub fn first_invalid_message(&self) -> Option<String> {
        match self.policy_type {
            OpenAIHostedShellNetworkPolicyType::Disabled => {
                if !self.allowed_domains.is_empty() || !self.domain_secrets.is_empty() {
                    return Some(
                        "`provider.openai.hosted_shell.network_policy.allowed_domains` and `provider.openai.hosted_shell.network_policy.domain_secrets` require `provider.openai.hosted_shell.network_policy.type = \"allowlist\"`."
                            .to_string(),
                    );
                }
            }
            OpenAIHostedShellNetworkPolicyType::Allowlist => {
                if let Some(index) = self
                    .allowed_domains
                    .iter()
                    .position(|value| value.trim().is_empty())
                {
                    return Some(format!(
                        "`provider.openai.hosted_shell.network_policy.allowed_domains[{index}]` must not be empty when set."
                    ));
                }

                if self.allowed_domains.is_empty() {
                    return Some(
                        "`provider.openai.hosted_shell.network_policy.allowed_domains` must include at least one domain when `provider.openai.hosted_shell.network_policy.type = \"allowlist\"`."
                            .to_string(),
                    );
                }

                for (index, secret) in self.domain_secrets.iter().enumerate() {
                    if let Some(message) = secret.validation_error(index) {
                        return Some(message);
                    }

                    let secret_domain = secret.domain.trim();
                    if !self
                        .allowed_domains
                        .iter()
                        .any(|domain| domain.trim().eq_ignore_ascii_case(secret_domain))
                    {
                        return Some(format!(
                            "`provider.openai.hosted_shell.network_policy.domain_secrets[{index}].domain` must also appear in `provider.openai.hosted_shell.network_policy.allowed_domains`."
                        ));
                    }
                }
            }
        }

        None
    }
}

/// Reserved keyword values for hosted skill version selection.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OpenAIHostedSkillVersionKeyword {
    #[default]
    Latest,
}

/// Hosted skill version selector for OpenAI Responses hosted shell mounts.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum OpenAIHostedSkillVersion {
    Latest(OpenAIHostedSkillVersionKeyword),
    Number(u64),
    String(String),
}

impl Default for OpenAIHostedSkillVersion {
    fn default() -> Self {
        Self::Latest(OpenAIHostedSkillVersionKeyword::Latest)
    }
}

impl OpenAIHostedSkillVersion {
    pub fn validation_error(&self, field_path: &str) -> Option<String> {
        match self {
            Self::String(value) if value.trim().is_empty() => {
                Some(format!("`{field_path}` must not be empty when set."))
            }
            _ => None,
        }
    }
}

/// Hosted skill reference mounted into an OpenAI hosted shell environment.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenAIHostedSkill {
    /// Reference to a pre-registered hosted skill.
    SkillReference {
        skill_id: String,
        #[serde(default)]
        version: OpenAIHostedSkillVersion,
    },
    /// Inline base64 zip bundle.
    Inline {
        bundle_b64: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
    },
}

impl OpenAIHostedSkill {
    pub fn validation_error(&self, index: usize) -> Option<String> {
        match self {
            Self::SkillReference { skill_id, version } => {
                let skill_id_path =
                    format!("provider.openai.hosted_shell.skills[{index}].skill_id");
                if skill_id.trim().is_empty() {
                    return Some(format!(
                        "`{skill_id_path}` must not be empty when `type = \"skill_reference\"`."
                    ));
                }

                let version_path = format!("provider.openai.hosted_shell.skills[{index}].version");
                version.validation_error(&version_path)
            }
            Self::Inline { bundle_b64, .. } => {
                let bundle_path =
                    format!("provider.openai.hosted_shell.skills[{index}].bundle_b64");
                if bundle_b64.trim().is_empty() {
                    return Some(format!(
                        "`{bundle_path}` must not be empty when `type = \"inline\"`."
                    ));
                }
                None
            }
        }
    }
}

/// OpenAI hosted shell configuration.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct OpenAIHostedShellConfig {
    /// Enable OpenAI hosted shell instead of VT Code's local shell tool.
    #[serde(default)]
    pub enabled: bool,

    /// Environment provisioning mode for hosted shell.
    #[serde(default)]
    pub environment: OpenAIHostedShellEnvironment,

    /// Existing OpenAI container ID to reuse when `environment = "container_reference"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,

    /// File IDs to mount when using `container_auto`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_ids: Vec<String>,

    /// Hosted skills to mount when using `container_auto`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<OpenAIHostedSkill>,

    /// Request-scoped network policy for `container_auto` hosted shells.
    #[serde(default)]
    pub network_policy: OpenAIHostedShellNetworkPolicy,
}

impl OpenAIHostedShellConfig {
    pub fn container_id_ref(&self) -> Option<&str> {
        self.container_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub const fn uses_container_reference(&self) -> bool {
        self.environment.uses_container_reference()
    }

    pub fn first_invalid_skill_message(&self) -> Option<String> {
        if self.uses_container_reference() {
            return None;
        }

        self.skills
            .iter()
            .enumerate()
            .find_map(|(index, skill)| skill.validation_error(index))
    }

    pub fn has_valid_skill_mounts(&self) -> bool {
        self.first_invalid_skill_message().is_none()
    }

    pub fn first_invalid_network_policy_message(&self) -> Option<String> {
        if self.uses_container_reference() {
            return None;
        }

        self.network_policy.first_invalid_message()
    }

    pub fn has_valid_network_policy(&self) -> bool {
        self.first_invalid_network_policy_message().is_none()
    }

    pub fn has_valid_reference_target(&self) -> bool {
        !self.uses_container_reference() || self.container_id_ref().is_some()
    }

    pub fn is_valid_for_runtime(&self) -> bool {
        self.has_valid_reference_target()
            && self.has_valid_skill_mounts()
            && self.has_valid_network_policy()
    }
}

/// OpenAI hosted tool search configuration.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct OpenAIToolSearchConfig {
    /// Enable hosted tool search for OpenAI Responses-compatible models.
    #[serde(default = "default_tool_search_enabled")]
    pub enabled: bool,

    /// Automatically defer loading of all tools except the core always-on set.
    #[serde(default = "default_defer_by_default")]
    pub defer_by_default: bool,

    /// Tool names that should never be deferred (always available).
    #[serde(default)]
    pub always_available_tools: Vec<String>,
}

impl Default for OpenAIToolSearchConfig {
    fn default() -> Self {
        Self {
            enabled: default_tool_search_enabled(),
            defer_by_default: default_defer_by_default(),
            always_available_tools: Vec::new(),
        }
    }
}

/// OpenAI-specific provider configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OpenAIConfig {
    /// Enable Responses API WebSocket transport for non-streaming requests.
    /// This is an opt-in path designed for long-running, tool-heavy workflows.
    #[serde(default)]
    pub websocket_mode: bool,

    /// Optional Responses API `store` flag.
    /// Set to `false` to avoid server-side storage when using Responses-compatible models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responses_store: Option<bool>,

    /// Optional Responses API `include` selectors.
    /// Example: `["reasoning.encrypted_content"]` for encrypted reasoning continuity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub responses_include: Vec<String>,

    /// Optional native OpenAI `service_tier` request parameter.
    /// Leave unset to inherit the Project-level default service tier.
    /// Options: "flex", "priority"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<OpenAIServiceTier>,

    /// Optional hosted shell configuration for OpenAI native Responses models.
    #[serde(default)]
    pub hosted_shell: OpenAIHostedShellConfig,

    /// Hosted tool search configuration for OpenAI Responses-compatible models.
    #[serde(default)]
    pub tool_search: OpenAIToolSearchConfig,
}

/// Anthropic-specific provider configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnthropicConfig {
    /// DEPRECATED: Model name validation has been removed. The Anthropic API validates
    /// model names directly, avoiding maintenance burden and allowing flexibility.
    /// This field is kept for backward compatibility but has no effect.
    #[deprecated(
        since = "0.75.0",
        note = "Model validation removed. API validates model names directly."
    )]
    #[serde(default)]
    pub skip_model_validation: bool,

    /// Enable extended thinking feature for Anthropic models
    /// When enabled, Claude uses internal reasoning before responding, providing
    /// enhanced reasoning capabilities for complex tasks.
    /// Only supported by Claude 4, Claude 4.5, and Claude 3.7 Sonnet models.
    /// Claude 4.6 uses adaptive thinking instead of extended thinking.
    /// Note: Extended thinking is now auto-enabled by default (31,999 tokens).
    /// Set MAX_THINKING_TOKENS=63999 environment variable for 2x budget on 64K models.
    /// See: https://docs.anthropic.com/en/docs/build-with-claude/extended-thinking
    #[serde(default = "default_extended_thinking_enabled")]
    pub extended_thinking_enabled: bool,

    /// Beta header for interleaved thinking feature
    #[serde(default = "default_interleaved_thinking_beta")]
    pub interleaved_thinking_beta: String,

    /// Budget tokens for extended thinking (minimum: 1024, default: 31999)
    /// On 64K output models (Opus 4.5, Sonnet 4.5, Haiku 4.5): default 31,999, max 63,999
    /// On 32K output models (Opus 4): max 31,999
    /// Use MAX_THINKING_TOKENS environment variable to override.
    #[serde(default = "default_interleaved_thinking_budget_tokens")]
    pub interleaved_thinking_budget_tokens: u32,

    /// Type value for enabling interleaved thinking
    #[serde(default = "default_interleaved_thinking_type")]
    pub interleaved_thinking_type_enabled: String,

    /// Tool search configuration for dynamic tool discovery (advanced-tool-use beta)
    #[serde(default)]
    pub tool_search: ToolSearchConfig,

    /// Effort level for token usage (high, medium, low)
    /// Controls how many tokens Claude uses when responding, trading off between
    /// response thoroughness and token efficiency.
    /// Supported by Claude Opus 4.5/4.6 (4.5 requires effort beta header)
    #[serde(default = "default_effort")]
    pub effort: String,

    /// Enable token counting via the count_tokens endpoint
    /// When enabled, the agent can estimate input token counts before making API calls
    /// Useful for proactive management of rate limits and costs
    #[serde(default = "default_count_tokens_enabled")]
    pub count_tokens_enabled: bool,
}

#[allow(deprecated)]
impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            skip_model_validation: false,
            extended_thinking_enabled: default_extended_thinking_enabled(),
            interleaved_thinking_beta: default_interleaved_thinking_beta(),
            interleaved_thinking_budget_tokens: default_interleaved_thinking_budget_tokens(),
            interleaved_thinking_type_enabled: default_interleaved_thinking_type(),
            tool_search: ToolSearchConfig::default(),
            effort: default_effort(),
            count_tokens_enabled: default_count_tokens_enabled(),
        }
    }
}

#[inline]
fn default_count_tokens_enabled() -> bool {
    false
}

/// Configuration for Anthropic's tool search feature (advanced-tool-use beta)
/// Enables dynamic tool discovery for large tool catalogs (up to 10k tools)
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolSearchConfig {
    /// Enable tool search feature (requires advanced-tool-use-2025-11-20 beta)
    #[serde(default = "default_tool_search_enabled")]
    pub enabled: bool,

    /// Search algorithm: "regex" (Python regex patterns) or "bm25" (natural language)
    #[serde(default = "default_tool_search_algorithm")]
    pub algorithm: String,

    /// Automatically defer loading of all tools except core tools
    #[serde(default = "default_defer_by_default")]
    pub defer_by_default: bool,

    /// Maximum number of tool search results to return
    #[serde(default = "default_max_results")]
    pub max_results: u32,

    /// Tool names that should never be deferred (always available)
    #[serde(default)]
    pub always_available_tools: Vec<String>,
}

impl Default for ToolSearchConfig {
    fn default() -> Self {
        Self {
            enabled: default_tool_search_enabled(),
            algorithm: default_tool_search_algorithm(),
            defer_by_default: default_defer_by_default(),
            max_results: default_max_results(),
            always_available_tools: vec![],
        }
    }
}

#[inline]
fn default_tool_search_enabled() -> bool {
    true
}

#[inline]
fn default_tool_search_algorithm() -> String {
    "regex".to_string()
}

#[inline]
fn default_defer_by_default() -> bool {
    true
}

#[inline]
fn default_max_results() -> u32 {
    5
}

#[inline]
fn default_extended_thinking_enabled() -> bool {
    true
}

#[inline]
fn default_interleaved_thinking_beta() -> String {
    "interleaved-thinking-2025-05-14".to_string()
}

#[inline]
fn default_interleaved_thinking_budget_tokens() -> u32 {
    31999
}

#[inline]
fn default_interleaved_thinking_type() -> String {
    "enabled".to_string()
}

#[inline]
fn default_effort() -> String {
    "low".to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        AnthropicConfig, OpenAIConfig, OpenAIHostedShellConfig, OpenAIHostedShellDomainSecret,
        OpenAIHostedShellEnvironment, OpenAIHostedShellNetworkPolicy,
        OpenAIHostedShellNetworkPolicyType, OpenAIHostedSkill, OpenAIHostedSkillVersion,
        OpenAIServiceTier,
    };

    #[test]
    fn openai_config_defaults_to_websocket_mode_disabled() {
        let config = OpenAIConfig::default();
        assert!(!config.websocket_mode);
        assert_eq!(config.responses_store, None);
        assert!(config.responses_include.is_empty());
        assert_eq!(config.service_tier, None);
        assert_eq!(config.hosted_shell, OpenAIHostedShellConfig::default());
        assert!(config.tool_search.enabled);
        assert!(config.tool_search.defer_by_default);
        assert!(config.tool_search.always_available_tools.is_empty());
    }

    #[test]
    fn openai_config_parses_websocket_mode_opt_in() {
        let parsed: OpenAIConfig =
            toml::from_str("websocket_mode = true").expect("config should parse");
        assert!(parsed.websocket_mode);
        assert_eq!(parsed.responses_store, None);
        assert!(parsed.responses_include.is_empty());
        assert_eq!(parsed.service_tier, None);
        assert_eq!(parsed.hosted_shell, OpenAIHostedShellConfig::default());
        assert_eq!(parsed.tool_search, super::OpenAIToolSearchConfig::default());
    }

    #[test]
    fn openai_config_parses_responses_options() {
        let parsed: OpenAIConfig = toml::from_str(
            r#"
responses_store = false
responses_include = ["reasoning.encrypted_content", "output_text.annotations"]
"#,
        )
        .expect("config should parse");
        assert_eq!(parsed.responses_store, Some(false));
        assert_eq!(
            parsed.responses_include,
            vec![
                "reasoning.encrypted_content".to_string(),
                "output_text.annotations".to_string()
            ]
        );
        assert_eq!(parsed.service_tier, None);
        assert_eq!(parsed.hosted_shell, OpenAIHostedShellConfig::default());
    }

    #[test]
    fn openai_config_parses_service_tier() {
        let parsed: OpenAIConfig =
            toml::from_str(r#"service_tier = "priority""#).expect("config should parse");
        assert_eq!(parsed.service_tier, Some(OpenAIServiceTier::Priority));
    }

    #[test]
    fn openai_config_parses_flex_service_tier() {
        let parsed: OpenAIConfig =
            toml::from_str(r#"service_tier = "flex""#).expect("config should parse");
        assert_eq!(parsed.service_tier, Some(OpenAIServiceTier::Flex));
    }

    #[test]
    fn openai_config_parses_hosted_shell() {
        let parsed: OpenAIConfig = toml::from_str(
            r#"
[hosted_shell]
enabled = true
environment = "container_auto"
file_ids = ["file_123"]

[[hosted_shell.skills]]
type = "skill_reference"
skill_id = "skill_123"
"#,
        )
        .expect("config should parse");

        assert!(parsed.hosted_shell.enabled);
        assert_eq!(
            parsed.hosted_shell.environment,
            OpenAIHostedShellEnvironment::ContainerAuto
        );
        assert_eq!(parsed.hosted_shell.file_ids, vec!["file_123".to_string()]);
        assert_eq!(
            parsed.hosted_shell.skills,
            vec![OpenAIHostedSkill::SkillReference {
                skill_id: "skill_123".to_string(),
                version: OpenAIHostedSkillVersion::default(),
            }]
        );
    }

    #[test]
    fn openai_config_parses_hosted_shell_pinned_version_and_inline_bundle() {
        let parsed: OpenAIConfig = toml::from_str(
            r#"
[hosted_shell]
enabled = true

[[hosted_shell.skills]]
type = "skill_reference"
skill_id = "skill_123"
version = 2

[[hosted_shell.skills]]
type = "inline"
bundle_b64 = "UEsFBgAAAAAAAA=="
sha256 = "deadbeef"
"#,
        )
        .expect("config should parse");

        assert_eq!(
            parsed.hosted_shell.skills,
            vec![
                OpenAIHostedSkill::SkillReference {
                    skill_id: "skill_123".to_string(),
                    version: OpenAIHostedSkillVersion::Number(2),
                },
                OpenAIHostedSkill::Inline {
                    bundle_b64: "UEsFBgAAAAAAAA==".to_string(),
                    sha256: Some("deadbeef".to_string()),
                },
            ]
        );
    }

    #[test]
    fn openai_config_parses_hosted_shell_network_policy() {
        let parsed: OpenAIConfig = toml::from_str(
            r#"
[hosted_shell]
enabled = true

[hosted_shell.network_policy]
type = "allowlist"
allowed_domains = ["httpbin.org"]

[[hosted_shell.network_policy.domain_secrets]]
domain = "httpbin.org"
name = "API_KEY"
value = "debug-secret-123"
"#,
        )
        .expect("config should parse");

        assert_eq!(
            parsed.hosted_shell.network_policy,
            OpenAIHostedShellNetworkPolicy {
                policy_type: OpenAIHostedShellNetworkPolicyType::Allowlist,
                allowed_domains: vec!["httpbin.org".to_string()],
                domain_secrets: vec![OpenAIHostedShellDomainSecret {
                    domain: "httpbin.org".to_string(),
                    name: "API_KEY".to_string(),
                    value: "debug-secret-123".to_string(),
                }],
            }
        );
    }

    #[test]
    fn openai_config_parses_tool_search() {
        let parsed: OpenAIConfig = toml::from_str(
            r#"
[tool_search]
enabled = false
defer_by_default = false
always_available_tools = ["unified_search", "custom_tool"]
"#,
        )
        .expect("config should parse");

        assert!(!parsed.tool_search.enabled);
        assert!(!parsed.tool_search.defer_by_default);
        assert_eq!(
            parsed.tool_search.always_available_tools,
            vec!["unified_search".to_string(), "custom_tool".to_string()]
        );
    }

    #[test]
    fn anthropic_tool_search_defaults_to_enabled() {
        let config = AnthropicConfig::default();

        assert!(config.tool_search.enabled);
        assert!(config.tool_search.defer_by_default);
        assert_eq!(config.tool_search.algorithm, "regex");
        assert!(config.tool_search.always_available_tools.is_empty());
    }

    #[test]
    fn hosted_shell_container_reference_requires_non_empty_container_id() {
        let config = OpenAIHostedShellConfig {
            enabled: true,
            environment: OpenAIHostedShellEnvironment::ContainerReference,
            container_id: Some("   ".to_string()),
            file_ids: Vec::new(),
            skills: Vec::new(),
            network_policy: OpenAIHostedShellNetworkPolicy::default(),
        };

        assert!(!config.has_valid_reference_target());
        assert!(config.container_id_ref().is_none());
    }

    #[test]
    fn hosted_shell_reports_invalid_skill_reference_mounts() {
        let config = OpenAIHostedShellConfig {
            enabled: true,
            environment: OpenAIHostedShellEnvironment::ContainerAuto,
            container_id: None,
            file_ids: Vec::new(),
            skills: vec![OpenAIHostedSkill::SkillReference {
                skill_id: "   ".to_string(),
                version: OpenAIHostedSkillVersion::default(),
            }],
            network_policy: OpenAIHostedShellNetworkPolicy::default(),
        };

        let message = config
            .first_invalid_skill_message()
            .expect("invalid mount should be reported");

        assert!(message.contains("provider.openai.hosted_shell.skills[0].skill_id"));
        assert!(!config.has_valid_skill_mounts());
        assert!(!config.is_valid_for_runtime());
    }

    #[test]
    fn hosted_shell_ignores_skill_validation_for_container_reference() {
        let config = OpenAIHostedShellConfig {
            enabled: true,
            environment: OpenAIHostedShellEnvironment::ContainerReference,
            container_id: Some("cntr_123".to_string()),
            file_ids: Vec::new(),
            skills: vec![OpenAIHostedSkill::Inline {
                bundle_b64: "   ".to_string(),
                sha256: None,
            }],
            network_policy: OpenAIHostedShellNetworkPolicy::default(),
        };

        assert!(config.first_invalid_skill_message().is_none());
        assert!(config.has_valid_skill_mounts());
        assert!(config.is_valid_for_runtime());
    }

    #[test]
    fn hosted_shell_reports_invalid_allowlist_without_domains() {
        let config = OpenAIHostedShellConfig {
            enabled: true,
            environment: OpenAIHostedShellEnvironment::ContainerAuto,
            container_id: None,
            file_ids: Vec::new(),
            skills: Vec::new(),
            network_policy: OpenAIHostedShellNetworkPolicy {
                policy_type: OpenAIHostedShellNetworkPolicyType::Allowlist,
                allowed_domains: Vec::new(),
                domain_secrets: Vec::new(),
            },
        };

        let message = config
            .first_invalid_network_policy_message()
            .expect("invalid network policy should be reported");

        assert!(message.contains("network_policy.allowed_domains"));
        assert!(!config.has_valid_network_policy());
        assert!(!config.is_valid_for_runtime());
    }

    #[test]
    fn hosted_shell_reports_domain_secret_outside_allowlist() {
        let config = OpenAIHostedShellConfig {
            enabled: true,
            environment: OpenAIHostedShellEnvironment::ContainerAuto,
            container_id: None,
            file_ids: Vec::new(),
            skills: Vec::new(),
            network_policy: OpenAIHostedShellNetworkPolicy {
                policy_type: OpenAIHostedShellNetworkPolicyType::Allowlist,
                allowed_domains: vec!["pypi.org".to_string()],
                domain_secrets: vec![OpenAIHostedShellDomainSecret {
                    domain: "httpbin.org".to_string(),
                    name: "API_KEY".to_string(),
                    value: "secret".to_string(),
                }],
            },
        };

        let message = config
            .first_invalid_network_policy_message()
            .expect("invalid domain secret should be reported");

        assert!(message.contains("domain_secrets[0].domain"));
        assert!(!config.has_valid_network_policy());
    }

    #[test]
    fn hosted_shell_ignores_network_policy_validation_for_container_reference() {
        let config = OpenAIHostedShellConfig {
            enabled: true,
            environment: OpenAIHostedShellEnvironment::ContainerReference,
            container_id: Some("cntr_123".to_string()),
            file_ids: Vec::new(),
            skills: Vec::new(),
            network_policy: OpenAIHostedShellNetworkPolicy {
                policy_type: OpenAIHostedShellNetworkPolicyType::Allowlist,
                allowed_domains: Vec::new(),
                domain_secrets: Vec::new(),
            },
        };

        assert!(config.first_invalid_network_policy_message().is_none());
        assert!(config.has_valid_network_policy());
    }
}
