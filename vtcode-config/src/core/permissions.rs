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

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            default_mode: PermissionMode::default(),
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
    }
}
