//! Edited-file monitor runtime wiring for ToolRegistry.

use crate::config::PermissionsConfig;

use super::ToolRegistry;

impl ToolRegistry {
    pub fn apply_permissions_config(&self, permissions: &PermissionsConfig) {
        self.edited_file_monitor
            .apply_permissions_config(permissions);
    }
}
