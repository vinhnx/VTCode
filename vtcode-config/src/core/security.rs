use serde::{Deserialize, Serialize};

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
        }
    }
}

#[inline]
const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_security_config() {
        let config = SecurityConfig::default();
        assert_eq!(config.human_in_the_loop, true);
        assert_eq!(config.require_write_tool_for_claims, true);
        assert_eq!(config.hitl_notification_bell, true);
    }

    #[test]
    fn test_security_config_with_custom_bell_setting() {
        let config = SecurityConfig {
            hitl_notification_bell: false,
            ..Default::default()
        };
        assert_eq!(config.hitl_notification_bell, false);
    }

    #[test]
    fn test_serialize_deserialize_security_config() {
        let original = SecurityConfig {
            human_in_the_loop: true,
            require_write_tool_for_claims: true,
            auto_apply_detected_patches: false,
            zero_trust_mode: false,
            encrypt_payloads: false,
            integrity_checks: true,
            hitl_notification_bell: true,
        };
        
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: SecurityConfig = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(original.hitl_notification_bell, deserialized.hitl_notification_bell);
        assert_eq!(original.human_in_the_loop, deserialized.human_in_the_loop);
    }
}
