use serde::{Deserialize, Serialize};

/// Gatekeeper mitigation configuration (macOS only)
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatekeeperConfig {
    /// Warn when a quarantined executable is detected
    #[serde(default = "default_true")]
    pub warn_on_quarantine: bool,

    /// Attempt to clear quarantine automatically (opt-in)
    #[serde(default)]
    pub auto_clear_quarantine: bool,

    /// Paths eligible for quarantine auto-clear
    #[serde(default = "default_gatekeeper_auto_clear_paths")]
    pub auto_clear_paths: Vec<String>,
}

/// Security configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SecurityConfig {
    /// Require human confirmation for critical actions
    #[serde(default = "default_true")]
    pub human_in_the_loop: bool,

    /// Require a successful write tool before accepting claims like
    /// "I've updated the file" as applied. When true, such claims are
    /// treated as proposals unless a write tool executed successfully.
    #[serde(default = "default_true")]
    pub require_write_tool_for_claims: bool,

    /// Automatically apply detected patch blocks in assistant replies
    /// when no write tool was executed. Defaults to false for safety.
    #[serde(default)]
    pub auto_apply_detected_patches: bool,

    /// Enable zero-trust checks between components.
    #[serde(default)]
    pub zero_trust_mode: bool,

    /// Encrypt payloads passed across executors.
    #[serde(default)]
    pub encrypt_payloads: bool,

    /// Enable runtime integrity tagging for critical paths.
    #[serde(default = "default_true")]
    pub integrity_checks: bool,

    /// Play terminal bell notification when HITL approval is required.
    #[serde(default = "default_true")]
    pub hitl_notification_bell: bool,

    /// Gatekeeper mitigation options (macOS only)
    #[serde(default)]
    pub gatekeeper: GatekeeperConfig,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            human_in_the_loop: default_true(),
            require_write_tool_for_claims: default_true(),
            auto_apply_detected_patches: false,
            zero_trust_mode: true,
            encrypt_payloads: true,
            integrity_checks: default_true(),
            hitl_notification_bell: default_true(),
            gatekeeper: GatekeeperConfig::default(),
        }
    }
}

#[inline]
const fn default_true() -> bool {
    true
}

fn default_gatekeeper_auto_clear_paths() -> Vec<String> {
    crate::constants::defaults::DEFAULT_GATEKEEPER_AUTO_CLEAR_PATHS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

impl Default for GatekeeperConfig {
    fn default() -> Self {
        Self {
            warn_on_quarantine: default_true(),
            auto_clear_quarantine: false,
            auto_clear_paths: default_gatekeeper_auto_clear_paths(),
        }
    }
}

#[cfg(test)]
#[path = "security_test.rs"]
mod security_test;
