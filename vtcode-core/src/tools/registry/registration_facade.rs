//! Registration-related ToolRegistry helpers.

use anyhow::Result;

use super::{ToolRegistration, ToolRegistry};

impl ToolRegistry {
    /// Register a new tool with the registry.
    ///
    /// # Arguments
    /// * `registration` - The tool registration to add
    ///
    /// # Returns
    /// `Result<()>` indicating success or an error if the tool is already registered
    pub async fn register_tool(&self, registration: ToolRegistration) -> Result<()> {
        self.inventory.register_tool(registration)?;
        // Invalidate cache
        if let Ok(mut cache) = self.cached_available_tools.write() {
            *cache = None;
        }
        self.rebuild_tool_assembly().await;
        self.sync_policy_catalog().await;
        Ok(())
    }

    /// Unregister a tool from the registry.
    pub async fn unregister_tool(&self, name: &str) -> Result<bool> {
        let removed = self.inventory.remove_tool(name)?.is_some();
        if removed {
            if let Ok(mut cache) = self.cached_available_tools.write() {
                *cache = None;
            }
            self.rebuild_tool_assembly().await;
            self.sync_policy_catalog().await;
        }
        Ok(removed)
    }
}
