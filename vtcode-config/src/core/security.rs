use serde::{Deserialize, Serialize};

/// Security configuration
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
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            human_in_the_loop: default_true(),
            require_write_tool_for_claims: default_true(),
            auto_apply_detected_patches: false,
        }
    }
}

fn default_true() -> bool {
    true
}
