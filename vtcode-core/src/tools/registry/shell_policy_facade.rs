//! Shell policy and agent identity accessors for ToolRegistry.

use std::borrow::Cow;

use anyhow::Result;

use super::ToolRegistry;

impl ToolRegistry {
    pub fn set_agent_type(&self, agent_type: impl Into<Cow<'static, str>>) {
        *self.agent_type.write().unwrap() = agent_type.into();
    }

    pub fn check_shell_policy(
        &self,
        command: &str,
        deny_regex_patterns: &[String],
        deny_glob_patterns: &[String],
    ) -> Result<()> {
        let agent_type = self.agent_type.read().unwrap().clone();
        let mut checker = self.shell_policy.write().unwrap();
        checker.check_command(
            command,
            &agent_type,
            deny_regex_patterns,
            deny_glob_patterns,
        )
    }
}
