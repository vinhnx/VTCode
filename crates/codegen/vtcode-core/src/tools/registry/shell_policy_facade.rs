//! Shell policy and agent identity accessors for ToolRegistry.

use anyhow::Result;

use super::ToolRegistry;

impl ToolRegistry {
    pub fn set_agent_type(&self, agent_type: impl Into<String>) {
        if let Ok(mut guard) = self.agent_type.write() {
            *guard = agent_type.into();
        }
    }

    pub fn check_shell_policy(
        &self,
        command: &str,
        deny_regex_patterns: &[String],
        deny_glob_patterns: &[String],
    ) -> Result<()> {
        let agent_type = self.agent_type.read().unwrap_or_else(|e| e.into_inner());
        let mut checker = self
            .shell_policy
            .write()
            .map_err(|e| anyhow::anyhow!("Shell policy lock poisoned: {e}"))?;
        checker.check_command(command, agent_type.as_str(), deny_regex_patterns, deny_glob_patterns)
    }
}
