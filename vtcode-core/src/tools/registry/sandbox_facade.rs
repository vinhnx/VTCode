//! Sandbox configuration accessors for ToolRegistry.

use super::ToolRegistry;

pub(super) fn runtime_sandbox_config_default() -> vtcode_config::SandboxConfig {
    let mut config = vtcode_config::SandboxConfig::default();
    // Keep legacy behavior for registry instances that never receive workspace config.
    config.enabled = false;
    config
}

impl ToolRegistry {
    pub fn apply_sandbox_config(&self, sandbox_config: &vtcode_config::SandboxConfig) {
        if let Ok(mut guard) = self.runtime_sandbox_config.write() {
            *guard = sandbox_config.clone();
        }
    }

    pub fn sandbox_config(&self) -> vtcode_config::SandboxConfig {
        self.runtime_sandbox_config
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| runtime_sandbox_config_default())
    }
}
