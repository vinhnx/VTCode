//! Inventory-related accessors for ToolRegistry.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use crate::tools::file_ops::FileOpsTool;
use crate::tools::grep_file::GrepSearchManager;

use super::{Tool, ToolHandler, ToolRegistry};

impl ToolRegistry {
    /// Get a tool by name from the inventory (with hot cache optimization).
    pub fn get_tool(&self, name: &str) -> Option<Arc<dyn Tool>> {
        // Check hot cache first if optimizations are enabled
        if self
            .optimization_config
            .tool_registry
            .use_optimized_registry
        {
            // Use a separate read and write operation to avoid borrow checker issues
            {
                let cache = self.hot_tool_cache.read();
                if let Some(cached_tool) = cache.peek(name) {
                    return Some(cached_tool.clone());
                }
            }
        }

        // Fallback to inventory lookup
        let tool = self
            .inventory
            .get_registration(name)
            .and_then(|reg| match reg.handler() {
                ToolHandler::TraitObject(tool) => Some(tool.clone()),
                _ => None,
            });

        // Cache the result if optimizations are enabled and tool was found
        if let Some(ref tool_arc) = tool
            && self
                .optimization_config
                .tool_registry
                .use_optimized_registry
        {
            self.hot_tool_cache
                .write()
                .put(name.to_string(), tool_arc.clone());
        }

        tool
    }
    pub fn workspace_root(&self) -> &std::path::PathBuf {
        self.inventory.workspace_root()
    }

    /// Get the workspace root as an owned PathBuf.
    pub fn workspace_root_owned(&self) -> PathBuf {
        self.inventory.workspace_root().clone()
    }

    /// Get workspace root as Cow<str> to avoid allocations when possible
    pub(crate) fn workspace_root_str(&self) -> Cow<'_, str> {
        self.workspace_root().to_string_lossy()
    }

    pub fn file_ops_tool(&self) -> &FileOpsTool {
        self.inventory.file_ops_tool()
    }

    pub fn grep_file_manager(&self) -> Arc<GrepSearchManager> {
        self.inventory.grep_file_manager()
    }
}
