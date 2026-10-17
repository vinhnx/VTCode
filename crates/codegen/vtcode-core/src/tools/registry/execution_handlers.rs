//! Registered handler dispatch after facade-owned admission and routing.

use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tracing::warn;

use crate::tools::traits::Tool;

use super::{ToolHandler, ToolRegistration, ToolRegistry};

impl ToolRegistry {
    /// Dispatch an admitted registration without processing or recording its result.
    pub(super) async fn execute_registered_handler(
        &self,
        tool_name: &str,
        registration: &ToolRegistration,
        args: Value,
        cached_tool: Option<&Arc<dyn Tool>>,
    ) -> Result<Value> {
        // Log deprecation warning if tool is deprecated
        if registration.is_deprecated() {
            if let Some(msg) = registration.deprecation_message() {
                warn!("Tool '{}' is deprecated: {}", tool_name, msg);
            } else {
                warn!("Tool '{}' is deprecated and may be removed in a future version", tool_name);
            }
        }

        let handler = registration.handler();
        match handler {
            ToolHandler::RegistryFn(executor) => {
                // PERFORMANCE OPTIMIZATION: Use memory pool for tool execution if enabled
                if self.optimization_config.memory_pool.enabled {
                    let _execution_guard = self.memory_pool.get_value();
                    let _string_guard = self.memory_pool.get_string();
                    let _vec_guard = self.memory_pool.get_vec();
                    executor(self, args).await
                } else {
                    executor(self, args).await
                }
            }
            ToolHandler::TraitObject(tool) => {
                // PERFORMANCE OPTIMIZATION: Use cached tool if available and optimizations enabled
                if self.optimization_config.tool_registry.use_optimized_registry {
                    if let Some(cached_tool) = cached_tool {
                        // Use cached tool instance to avoid registry lookup overhead
                        cached_tool.execute(args).await
                    } else {
                        // Cache the tool for future use
                        self.hot_tool_cache.write().put(tool_name.to_string(), tool.clone());
                        tool.execute(args).await
                    }
                } else {
                    tool.execute(args).await
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
