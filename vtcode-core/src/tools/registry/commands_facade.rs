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
    }
}
