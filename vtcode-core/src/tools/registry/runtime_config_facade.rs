//! Shared runtime configuration application for ToolRegistry.

use anyhow::Result;

use crate::config::{CommandsConfig, TimeoutsConfig, ToolsConfig};

use super::ToolRegistry;

impl ToolRegistry {
    pub async fn apply_tool_runtime_config(
        &self,
        commands_config: &CommandsConfig,
        tools_config: &ToolsConfig,
    ) -> Result<()> {
        self.apply_commands_config(commands_config);
        self.apply_config_policies(tools_config).await
    }

    pub async fn apply_session_runtime_config(
        &self,
        commands_config: &CommandsConfig,
        sandbox_config: &vtcode_config::SandboxConfig,
        timeouts: &TimeoutsConfig,
        tools_config: &ToolsConfig,
    ) -> Result<()> {
        self.apply_commands_config(commands_config);
        self.apply_sandbox_config(sandbox_config);
        self.apply_timeout_policy(timeouts);
        self.apply_config_policies(tools_config).await
    }
}
