use crate::env_helpers::default_true;
use serde::{Deserialize, Serialize};
use vtcode_commons::VtCodePaths;

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
    require_write_tool_for_claims: bool,

    /// Automatically apply detected patch blocks in assistant replies
    /// when no write tool was executed. Defaults to false for safety.
    #[serde(default)]
    auto_apply_detected_patches: bool,

    /// Enable zero-trust checks between components.
    #[serde(default = "default_true")]
    pub zero_trust_mode: bool,

    /// Encrypt payloads passed across executors.
    #[serde(default = "default_true")]
    encrypt_payloads: bool,

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
            zero_trust_mode: default_true(),
            encrypt_payloads: default_true(),
            integrity_checks: default_true(),
            hitl_notification_bell: default_true(),
            gatekeeper: GatekeeperConfig::default(),
        }
    }
}

fn default_gatekeeper_auto_clear_paths() -> Vec<String> {
    let mut paths = vec![".vtcode/bin".to_string()];
    if let Ok(resolver) = VtCodePaths::resolve() {
        paths.push(resolver.executable_dir().display().to_string());
        paths.push(resolver.legacy_dir().join("bin").display().to_string());
    } else {
        paths.push(".local/bin".to_string());
    }
    paths.sort();
    paths.dedup();
    paths
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
