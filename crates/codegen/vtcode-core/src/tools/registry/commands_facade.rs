//! Command configuration accessors for ToolRegistry.

use crate::config::CommandsConfig;

use super::ToolRegistry;

impl ToolRegistry {
    pub fn apply_commands_config(&self, commands_config: &CommandsConfig) {
        self.inventory.update_commands_config(commands_config);
        self.pty_sessions
            .manager()
            .apply_commands_config(commands_config);
        match self.shell_policy.write() {
            Ok(mut shell_policy) => shell_policy.set_commands_config(commands_config),
            Err(poisoned) => poisoned.into_inner().set_commands_config(commands_config),
        }
    }

    pub fn commands_config(&self) -> CommandsConfig {
        match self.shell_policy.read() {
            Ok(shell_policy) => shell_policy.commands_config().cloned().unwrap_or_default(),
            Err(poisoned) => poisoned
                .into_inner()
                .commands_config()
                .cloned()
                .unwrap_or_default(),
        }
    }
}
