use serde::{Deserialize, Serialize};

/// Permission system configuration - Controls command resolution, audit logging, and caching
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PermissionsConfig {
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

    /// Log sandbox events
    #[serde(default = "default_log_sandbox_events")]
    pub log_sandbox_events: bool,

    /// Enable permission decision caching to avoid redundant evaluations
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,

    /// Cache time-to-live in seconds (how long to cache decisions)
    /// Default: 300 seconds (5 minutes)
    #[serde(default = "default_cache_ttl_seconds")]
    pub cache_ttl_seconds: u64,
}

fn default_enabled() -> bool {
    true
}

fn default_resolve_commands() -> bool {
    true
}

fn default_audit_enabled() -> bool {
    true
}

fn default_audit_directory() -> String {
    "~/.vtcode/audit".to_string()
}

fn default_log_allowed_commands() -> bool {
    true
}

fn default_log_denied_commands() -> bool {
    true
}

fn default_log_permission_prompts() -> bool {
    true
}

fn default_log_sandbox_events() -> bool {
    true
}

fn default_cache_enabled() -> bool {
    true
}

fn default_cache_ttl_seconds() -> u64 {
    300 // 5 minutes
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            resolve_commands: default_resolve_commands(),
            audit_enabled: default_audit_enabled(),
            audit_directory: default_audit_directory(),
            log_allowed_commands: default_log_allowed_commands(),
            log_denied_commands: default_log_denied_commands(),
            log_permission_prompts: default_log_permission_prompts(),
            log_sandbox_events: default_log_sandbox_events(),
            cache_enabled: default_cache_enabled(),
            cache_ttl_seconds: default_cache_ttl_seconds(),
        }
    }
}
