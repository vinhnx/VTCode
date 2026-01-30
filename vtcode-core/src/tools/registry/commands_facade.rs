//! Command configuration accessors for ToolRegistry.

use crate::config::CommandsConfig;

use super::ToolRegistry;

impl ToolRegistry {
    pub fn apply_commands_config(&self, commands_config: &CommandsConfig) {
        self.inventory
            .command_tool()
            .write()
            .unwrap()
            .update_commands_config(commands_config);
        self.pty_sessions
            .manager()
            .apply_commands_config(commands_config);
        self.shell_policy
            .write()
            .unwrap()
            .set_commands_config(commands_config);
    }

    pub fn commands_config(&self) -> CommandsConfig {
        self.shell_policy
            .read()
            .unwrap()
            .commands_config()
            .cloned()
            .unwrap_or_default()
    }
}
