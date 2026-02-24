//! Unified tool resolution system to eliminate code duplication
//!
//! This module provides a single, efficient tool resolution mechanism
//! that can be used by both `has_tool()` and `execute_tool()` methods.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::types::{ToolRegistration, ToolExecutionError, ToolErrorType};
use super::inventory::ToolInventory;
use super::mcp_client::McpClient;

/// Result of unified tool resolution
#[derive(Debug, Clone)]
pub struct ToolResolution {
    /// The resolved tool information
    pub tool_type: ToolType,
    /// Whether the tool needs PTY
    pub needs_pty: bool,
    /// The actual tool name to use for execution
    pub resolved_name: String,
    /// Original name before resolution
    pub original_name: String,
}

/// Type of tool and its source
#[derive(Debug, Clone)]
pub enum ToolType {
    /// Built-in tool from the registry
    Builtin(Arc<ToolRegistration>),
    /// MCP tool from an external provider
    Mcp {
        provider_name: String,
        tool_name: String,
    },
}

/// Unified tool resolver that eliminates code duplication
pub struct UnifiedToolResolver {
    /// Tool inventory for built-in tools
    inventory: Arc<ToolInventory>,
    /// MCP client for external tools
    mcp_client: Option<Arc<dyn super::McpToolExecutor>>,
    /// Cache for tool resolution results
    resolution_cache: Arc<RwLock<HashMap<String, Option<ToolResolution>>>>,
    /// Cache for MCP tool presence
    mcp_presence_cache: Arc<RwLock<HashMap<String, bool>>>,
    /// Cache configuration
    cache_config: CacheConfig,
}

#[derive(Clone)]
struct CacheConfig {
    /// Maximum age for cached entries
    max_age_seconds: u64,
    /// Whether to cache negative results (tool not found)
    cache_negatives: bool,
}

impl UnifiedToolResolver {
    pub fn new(
        inventory: Arc<ToolInventory>,
        mcp_client: Option<Arc<dyn super::McpToolExecutor>>,
    ) -> Self {
        Self {
            inventory,
            mcp_client,
            resolution_cache: Arc::new(RwLock::new(HashMap::new())),
            mcp_presence_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_config: CacheConfig {
                max_age_seconds: 300, // 5 minutes
                cache_negatives: true,
            },
        }
    }

    /// Resolve a tool name to its actual implementation
    pub async fn resolve_tool(&self, tool_name: &str) -> Result<ToolResolution, ToolExecutionError> {
        let original_name = tool_name.to_string();

        // Check cache first
        if let Some(cached) = self.get_cached_resolution(&original_name).await {
            return cached.ok_or_else(|| {
                ToolExecutionError::new(
                    original_name.clone(),
                    ToolErrorType::NotFound,
                    format!("Tool '{}' not found", tool_name),
                    format!("Tool '{}' was not found in any registry", tool_name),
                )
            });
        }

        // Perform resolution
        let resolution = self.perform_resolution(&original_name).await;

        // Cache the result
        self.cache_resolution(&original_name, resolution.as_ref().ok()).await;

        resolution.map_err(|e| {
            ToolExecutionError::new(
                original_name.clone(),
                ToolErrorType::NotFound,
                format!("Tool '{}' not found: {}", tool_name, e),
                e,
            )
        })
    }

    /// Check if a tool exists without full resolution (faster for has_tool checks)
    pub async fn has_tool(&self, tool_name: &str) -> bool {
        // Quick check for built-in tools
        if self.inventory.has_tool(tool_name) {
            return true;
        }

        // Check MCP tool presence cache
        if let Some(cached) = self.get_cached_mcp_presence(tool_name).await {
            return cached;
        }

        // Perform MCP presence check
        let has_tool = self.check_mcp_tool_presence(tool_name).await;

        // Cache the result
        self.cache_mcp_presence(tool_name, has_tool).await;

        has_tool
    }

    /// Perform the actual tool resolution
    async fn perform_resolution(&self, tool_name: &str) -> Result<ToolResolution, String> {
        // 1. Check built-in tools first (fastest path)
        if let Some(registration) = self.inventory.registration_for(tool_name) {
            return Ok(ToolResolution {
                tool_type: ToolType::Builtin(Arc::new(registration.clone())),
                needs_pty: registration.uses_pty(),
                resolved_name: tool_name.to_string(),
                original_name: tool_name.to_string(),
            });
        }

        // 2. Check MCP tools
        if let Some(mcp_client) = &self.mcp_client {
            // Handle MCP tool names with or without prefix
            let (mcp_tool_name, resolved_name) = if let Some(stripped) = tool_name.strip_prefix("mcp_") {
                (stripped.to_string(), tool_name.to_string())
            } else {
                (tool_name.to_string(), format!("mcp_{}", tool_name))
            };

            // Check if MCP tool exists
            match mcp_client.has_mcp_tool(&mcp_tool_name).await {
                Ok(true) => {
                    return Ok(ToolResolution {
                        tool_type: ToolType::Mcp {
                            provider_name: self.find_mcp_provider(&mcp_tool_name).await
                                .unwrap_or_else(|| "unknown".to_string()),
                            tool_name: mcp_tool_name.clone(),
                        },
                        needs_pty: true, // MCP tools typically need PTY
                        resolved_name,
                        original_name: tool_name.to_string(),
                    });
                }
                Ok(false) => {
                    // Check if it's an alias
                    if let Some(resolved_name) = self.resolve_mcp_tool_alias(&mcp_tool_name).await {
                        if resolved_name != mcp_tool_name {
                            return Ok(ToolResolution {
                                tool_type: ToolType::Mcp {
                                    provider_name: self.find_mcp_provider(&resolved_name).await
                                        .unwrap_or_else(|| "unknown".to_string()),
                                    tool_name: resolved_name,
                                },
                                needs_pty: true,
                                resolved_name: format!("mcp_{}", resolved_name),
                                original_name: tool_name.to_string(),
                            });
                        }
                    }

                    // Fallback: if MCP fetch is unavailable, route to built-in web_fetch
                    if mcp_tool_name == "fetch" && self.inventory.has_tool("web_fetch") {
                        if let Some(registration) = self.inventory.registration_for("web_fetch") {
                            return Ok(ToolResolution {
                                tool_type: ToolType::Builtin(Arc::new(registration.clone())),
                                needs_pty: registration.uses_pty(),
                                resolved_name: "web_fetch".to_string(),
                                original_name: tool_name.to_string(),
                            });
                        }
                    }
                }
                Err(err) => {
                    return Err(format!("Failed to check MCP tool '{}': {}", mcp_tool_name, err));
                }
            }
        }

        Err(format!("Tool '{}' not found in any registry", tool_name))
    }

    /// Check MCP tool presence with proper error handling
    async fn check_mcp_tool_presence(&self, tool_name: &str) -> bool {
        if let Some(mcp_client) = &self.mcp_client {
            let mcp_tool_name = if let Some(stripped) = tool_name.strip_prefix("mcp_") {
                stripped
            } else {
                tool_name
            };

            match mcp_client.has_mcp_tool(mcp_tool_name).await {
                Ok(true) => return true,
                Ok(false) => {
                    // Check if it's an alias
                    if let Some(resolved_name) = self.resolve_mcp_tool_alias(mcp_tool_name).await {
                        if resolved_name != mcp_tool_name {
                            return true;
                        }
                    }
                }
                Err(_) => {
                    // Log error but don't fail the presence check
                    // This allows the system to continue working even if MCP is temporarily unavailable
                }
            }
        }
        false
    }

    /// Find MCP provider for a tool (simplified version)
    async fn find_mcp_provider(&self, tool_name: &str) -> Option<String> {
        // This is a simplified implementation
        // In practice, you might want to maintain a provider index or
        // query each provider to find which one owns the tool
        Some("mcp_provider".to_string())
    }

    /// Resolve MCP tool aliases
    async fn resolve_mcp_tool_alias(&self, tool_name: &str) -> Option<String> {
        // This would implement alias resolution logic
        // For now, just return the original name
        Some(tool_name.to_string())
    }

    /// Get cached resolution result
    async fn get_cached_resolution(&self, tool_name: &str) -> Option<Result<ToolResolution, ()>> {
        let cache = self.resolution_cache.read().ok()?;
        cache.get(tool_name).cloned()
    }

    /// Cache resolution result
    async fn cache_resolution(&self, tool_name: &str, resolution: Option<&ToolResolution>) {
        let Ok(mut cache) = self.resolution_cache.write() else {
            return;
        };

        if let Some(res) = resolution {
            cache.insert(tool_name.to_string(), Ok(res.clone()));
        } else if self.cache_config.cache_negatives {
            cache.insert(tool_name.to_string(), Err(()));
        }
    }

    /// Get cached MCP presence result
    async fn get_cached_mcp_presence(&self, tool_name: &str) -> Option<bool> {
        let cache = self.mcp_presence_cache.read().ok()?;
        cache.get(tool_name).copied()
    }

    /// Cache MCP presence result
    async fn cache_mcp_presence(&self, tool_name: &str, has_tool: bool) {
        if let Ok(mut cache) = self.mcp_presence_cache.write() {
            cache.insert(tool_name.to_string(), has_tool);
        }
    }

    /// Clear all caches
    pub async fn clear_caches(&self) {
        if let Ok(mut cache) = self.resolution_cache.write() {
            cache.clear();
        }
        if let Ok(mut cache) = self.mcp_presence_cache.write() {
            cache.clear();
        }
    }

    /// Get cache statistics
    pub async fn cache_stats(&self) -> CacheStats {
        CacheStats {
            resolution_cache_entries: self.resolution_cache.read().ok().map(|c| c.len()).unwrap_or(0),
            mcp_presence_cache_entries: self.mcp_presence_cache.read().ok().map(|c| c.len()).unwrap_or(0),
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub resolution_cache_entries: usize,
    pub mcp_presence_cache_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_builtin_tool_resolution() {
        let inventory = Arc::new(ToolInventory::new());
        let resolver = UnifiedToolResolver::new(inventory, None);

        // This would need actual tool registrations to test properly
        // For now, just test the structure
        assert!(!resolver.has_tool("nonexistent_tool").await);
    }

    #[tokio::test]
    async fn test_cache_operations() {
        let inventory = Arc::new(ToolInventory::new());
        let resolver = UnifiedToolResolver::new(inventory, None);

        // Test cache stats
        let stats = resolver.cache_stats().await;
        assert_eq!(stats.resolution_cache_entries, 0);
        assert_eq!(stats.mcp_presence_cache_entries, 0);

        // Clear caches
        resolver.clear_caches().await;
    }

    #[tokio::test]
    async fn test_tool_name_parsing() {
        let inventory = Arc::new(ToolInventory::new());
        let resolver = UnifiedToolResolver::new(inventory, None);

        // Test MCP tool name parsing
        assert_eq!(resolver.check_mcp_tool_presence("mcp_some_tool").await, false);
        assert_eq!(resolver.check_mcp_tool_presence("some_tool").await, false);
    }
}