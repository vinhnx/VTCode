use serde::{Deserialize, Serialize};

/// Unified permission mode for authored policy evaluation.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Standard interactive behavior with prompts when policy requires them.
    #[default]
    #[serde(alias = "ask", alias = "suggest")]
    Default,
    /// Auto-allow built-in file mutations for the active session.
    #[serde(alias = "acceptEdits", alias = "accept-edits", alias = "auto-approved")]
    AcceptEdits,
    /// Classifier-backed autonomous mode.
    #[serde(alias = "trusted_auto", alias = "trusted-auto")]
    Auto,
    /// Read-only planning mode.
    Plan,
    /// Deny any action that is not explicitly allowed.
    #[serde(alias = "dontAsk", alias = "dont-ask")]
    DontAsk,
    /// Skip prompts except protected writes and sandbox escalation prompts.
    #[serde(
        alias = "bypassPermissions",
        alias = "bypass-permissions",
        alias = "full-auto"
    )]
    BypassPermissions,
}

/// Permission system configuration - Controls command resolution, audit logging, and caching
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PermissionsConfig {
    /// Default unified permission mode for the current session.
    #[serde(default)]
    pub default_mode: PermissionMode,

    /// Classifier-backed auto mode policy and environment settings.
    #[serde(default)]
    pub auto_mode: AutoModeConfig,

    /// Rules that allow matching tool calls without prompting.
    #[serde(default)]
    pub allow: Vec<String>,

    /// Rules that require an interactive prompt when they match.
    #[serde(default)]
    pub ask: Vec<String>,

    /// Rules that deny matching tool calls.
    #[serde(default)]
    pub deny: Vec<String>,

    /// Enable the enhanced permission system (resolver + audit logger + cache)
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Enable command resolution to actual paths (helps identify suspicious commands)
    #[serde(default = "default_resolve_commands")]
    pub resolve_commands: bool,

    /// Enable audit logging of all permission decisions
    #[serde(default = "default_audit_enabled")]
    pub audit_enabled: bool,

    /// Directory for audit logs (created if not exists)
    /// Defaults to ~/.vtcode/audit
    #[serde(default = "default_audit_directory")]
    pub audit_directory: String,

    /// Log allowed commands to audit trail
    #[serde(default = "default_log_allowed_commands")]
    pub log_allowed_commands: bool,

    /// Log denied commands to audit trail
    #[serde(default = "default_log_denied_commands")]
    pub log_denied_commands: bool,

    /// Log permission prompts (when user is asked for confirmation)
    #[serde(default = "default_log_permission_prompts")]
    pub log_permission_prompts: bool,

    /// Enable permission decision caching to avoid redundant evaluations
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,

    /// Cache time-to-live in seconds (how long to cache decisions)
    /// Default: 300 seconds (5 minutes)
    #[serde(default = "default_cache_ttl_seconds")]
    pub cache_ttl_seconds: u64,
}

/// Classifier-backed auto mode configuration.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AutoModeConfig {
    /// Optional model override for the transcript reviewer.
    #[serde(default)]
    pub model: String,

    /// Optional model override for the prompt-injection probe.
    #[serde(default)]
    pub probe_model: String,

    /// Maximum consecutive denials before auto mode falls back.
    #[serde(default = "default_auto_mode_max_consecutive_denials")]
    pub max_consecutive_denials: u32,

    /// Maximum total denials before auto mode falls back.
    #[serde(default = "default_auto_mode_max_total_denials")]
    pub max_total_denials: u32,

    /// Drop broad code-execution allow rules while auto mode is active.
    #[serde(default = "default_auto_mode_drop_broad_allow_rules")]
    pub drop_broad_allow_rules: bool,

    /// Classifier block rules applied in stage 2 reasoning.
    #[serde(default = "default_auto_mode_block_rules")]
    pub block_rules: Vec<String>,

    /// Narrow allow exceptions applied after block rules.
    #[serde(default = "default_auto_mode_allow_exceptions")]
    pub allow_exceptions: Vec<String>,

    /// Trusted environment boundaries for the classifier.
    #[serde(default)]
    pub environment: AutoModeEnvironmentConfig,
}

/// Trust-boundary configuration for auto mode.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AutoModeEnvironmentConfig {
    #[serde(default)]
    pub trusted_paths: Vec<String>,

    #[serde(default)]
    pub trusted_domains: Vec<String>,

    #[serde(default)]
    pub trusted_git_hosts: Vec<String>,

    #[serde(default)]
    pub trusted_git_orgs: Vec<String>,

    #[serde(default)]
    pub trusted_services: Vec<String>,
}

impl Default for AutoModeConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            probe_model: String::new(),
            max_consecutive_denials: default_auto_mode_max_consecutive_denials(),
            max_total_denials: default_auto_mode_max_total_denials(),
            drop_broad_allow_rules: default_auto_mode_drop_broad_allow_rules(),
            block_rules: default_auto_mode_block_rules(),
            allow_exceptions: default_auto_mode_allow_exceptions(),
            environment: AutoModeEnvironmentConfig::default(),
        }
    }
}

#[inline]
const fn default_enabled() -> bool {
    true
}

#[inline]
const fn default_resolve_commands() -> bool {
    true
}

#[inline]
const fn default_audit_enabled() -> bool {
    true
}

const DEFAULT_AUDIT_DIR: &str = "~/.vtcode/audit";

#[inline]
fn default_audit_directory() -> String {
    DEFAULT_AUDIT_DIR.into()
}

#[inline]
const fn default_log_allowed_commands() -> bool {
    true
}

#[inline]
const fn default_log_denied_commands() -> bool {
    true
}

#[inline]
const fn default_log_permission_prompts() -> bool {
    true
}

#[inline]
const fn default_cache_enabled() -> bool {
    true
}

#[inline]
const fn default_cache_ttl_seconds() -> u64 {
    300 // 5 minutes
}

#[inline]
const fn default_auto_mode_max_consecutive_denials() -> u32 {
    3
}

#[inline]
const fn default_auto_mode_max_total_denials() -> u32 {
    20
}

#[inline]
const fn default_auto_mode_drop_broad_allow_rules() -> bool {
    true
}

fn default_auto_mode_block_rules() -> Vec<String> {
    vec![
        "Block destructive source-control actions such as force-pushes, direct pushes to protected branches, or remote branch deletion unless the user explicitly authorized that exact blast radius.".to_string(),
        "Block remote code download-and-execute flows, including curl-or-wget pipes into interpreters or shells, and running code from freshly cloned external repositories.".to_string(),
        "Block sending workspace contents, credentials, environment variables, or other sensitive data to external endpoints unless the user explicitly named both the destination and the data.".to_string(),
        "Block credential hunting, secret discovery, or scanning config and environment stores for alternative tokens unless the user explicitly asked for that credential workflow.".to_string(),
        "Block commands that bypass safety checks, disable verification, install persistence, or weaken the host security posture.".to_string(),
        "Block destructive or production/shared-infrastructure actions unless the user explicitly authorized the target and the action.".to_string(),
        "Block destructive actions against inferred, fuzzy-matched, or agent-selected targets when the user did not name the exact target.".to_string(),
    ]
}

fn default_auto_mode_allow_exceptions() -> Vec<String> {
    vec![
        "Allow read-only tools and read-only browsing/search actions.".to_string(),
        "Allow file edits and writes inside the current workspace when the path is not protected.".to_string(),
        "Allow pushes only to the current session branch or configured git remotes inside the trusted environment.".to_string(),
    ]
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            default_mode: PermissionMode::default(),
            auto_mode: AutoModeConfig::default(),
            allow: Vec::new(),
            ask: Vec::new(),
            deny: Vec::new(),
            enabled: default_enabled(),
            resolve_commands: default_resolve_commands(),
            audit_enabled: default_audit_enabled(),
            audit_directory: default_audit_directory(),
            log_allowed_commands: default_log_allowed_commands(),
            log_denied_commands: default_log_denied_commands(),
            log_permission_prompts: default_log_permission_prompts(),
            cache_enabled: default_cache_enabled(),
            cache_ttl_seconds: default_cache_ttl_seconds(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PermissionMode, PermissionsConfig};

    #[test]
    fn parses_claude_style_mode_aliases() {
        let config: PermissionsConfig = toml::from_str(
            r#"
            default_mode = "acceptEdits"
            "#,
        )
        .expect("permissions config");
        assert_eq!(config.default_mode, PermissionMode::AcceptEdits);

        let config: PermissionsConfig = toml::from_str(
            r#"
            default_mode = "dontAsk"
            "#,
        )
        .expect("permissions config");
        assert_eq!(config.default_mode, PermissionMode::DontAsk);

        let config: PermissionsConfig = toml::from_str(
            r#"
            default_mode = "bypassPermissions"
            "#,
        )
        .expect("permissions config");
        assert_eq!(config.default_mode, PermissionMode::BypassPermissions);

        let config: PermissionsConfig = toml::from_str(
            r#"
            default_mode = "auto"
            "#,
        )
        .expect("permissions config");
        assert_eq!(config.default_mode, PermissionMode::Auto);
    }

    #[test]
    fn parses_legacy_mode_aliases() {
        let config: PermissionsConfig = toml::from_str(
            r#"
            default_mode = "ask"
            "#,
        )
        .expect("permissions config");
        assert_eq!(config.default_mode, PermissionMode::Default);

        let config: PermissionsConfig = toml::from_str(
            r#"
            default_mode = "auto-approved"
            "#,
        )
        .expect("permissions config");
        assert_eq!(config.default_mode, PermissionMode::AcceptEdits);

        let config: PermissionsConfig = toml::from_str(
            r#"
            default_mode = "full-auto"
            "#,
        )
        .expect("permissions config");
        assert_eq!(config.default_mode, PermissionMode::BypassPermissions);

        let config: PermissionsConfig = toml::from_str(
            r#"
            default_mode = "trusted_auto"
            "#,
        )
        .expect("permissions config");
        assert_eq!(config.default_mode, PermissionMode::Auto);
    }

    #[test]
    fn parses_exact_tool_rules() {
        let config: PermissionsConfig = toml::from_str(
            r#"
            allow = ["read_file", "unified_search"]
            deny = ["unified_exec"]
            "#,
        )
        .expect("permissions config");

        assert_eq!(
            config.allow,
            vec!["read_file".to_string(), "unified_search".to_string()]
        );
        assert_eq!(config.deny, vec!["unified_exec".to_string()]);
    }

    #[test]
    fn auto_mode_defaults_are_conservative() {
        let config = PermissionsConfig::default();

        assert_eq!(config.auto_mode.max_consecutive_denials, 3);
        assert_eq!(config.auto_mode.max_total_denials, 20);
        assert!(config.auto_mode.drop_broad_allow_rules);
        assert!(!config.auto_mode.block_rules.is_empty());
        assert!(!config.auto_mode.allow_exceptions.is_empty());
        assert!(config.auto_mode.environment.trusted_paths.is_empty());
    }
}
