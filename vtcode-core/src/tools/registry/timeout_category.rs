//! Timeout category helpers for ToolRegistry.

use super::{ToolRegistry, ToolTimeoutCategory};

impl ToolRegistry {
    pub async fn timeout_category_for(&self, name: &str) -> ToolTimeoutCategory {
        // Resolve alias through registration lookup
        let registration_opt = self.inventory.registration_for(name);
        if let Some(registration) = registration_opt {
            return if registration.uses_pty() {
                ToolTimeoutCategory::Pty
            } else {
                ToolTimeoutCategory::Default
            };
        }

        if let Some(stripped) = name.strip_prefix("mcp_") {
            if self.has_mcp_tool(stripped).await {
                return ToolTimeoutCategory::Mcp;
            }
        } else if self.find_mcp_provider(name).await.is_some() || self.has_mcp_tool(name).await {
            return ToolTimeoutCategory::Mcp;
        }

        ToolTimeoutCategory::Default
    }
}
