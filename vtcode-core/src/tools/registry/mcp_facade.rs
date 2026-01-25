//! MCP client integration for ToolRegistry.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, anyhow};
use serde_json::Value;
use tracing::{debug, warn};

use crate::mcp::{McpClient, McpToolExecutor, McpToolInfo};
use crate::tools::mcp::build_mcp_registration;

use super::ToolRegistry;
use super::mcp_helpers::normalize_mcp_tool_identifier;

impl ToolRegistry {
    /// Set the MCP client for this registry.
    pub async fn with_mcp_client(self, mcp_client: Arc<McpClient>) -> Self {
        *self.mcp_client.write().unwrap() = Some(mcp_client);
        self.mcp_tool_index.write().await.clear();
        self.mcp_tool_presence.write().await.clear();
        if let Ok(mut cache) = self.cached_available_tools.write() {
            *cache = None;
        }
        self.initialized
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self
    }

    /// Attach an MCP client without consuming the registry.
    pub async fn set_mcp_client(&self, mcp_client: Arc<McpClient>) {
        *self.mcp_client.write().unwrap() = Some(mcp_client);
        self.mcp_tool_index.write().await.clear();
        self.mcp_tool_presence.write().await.clear();
        if let Ok(mut cache) = self.cached_available_tools.write() {
            *cache = None;
        }
        self.initialized
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// Get the MCP client if available.
    pub fn mcp_client(&self) -> Option<Arc<McpClient>> {
        self.mcp_client.read().unwrap().clone()
    }

    /// List all MCP tools.
    pub async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>> {
        let client_opt = self.mcp_client.read().unwrap().clone();
        if let Some(mcp_client) = client_opt {
            mcp_client.list_mcp_tools().await
        } else {
            Ok(Vec::new())
        }
    }

    /// Check if an MCP tool exists.
    pub async fn has_mcp_tool(&self, tool_name: &str) -> bool {
        {
            let index = self.mcp_tool_index.read().await;
            if index
                .values()
                .any(|tools| tools.iter().any(|candidate| candidate == tool_name))
            {
                self.mcp_tool_presence
                    .write()
                    .await
                    .insert(tool_name.to_string(), true);
                return true;
            }
        }

        if let Some(cached) = self.mcp_tool_presence.read().await.get(tool_name) {
            return *cached;
        }

        let mcp_client_opt = { self.mcp_client.read().unwrap().clone() };
        let Some(mcp_client) = mcp_client_opt else {
            self.mcp_tool_presence
                .write()
                .await
                .insert(tool_name.to_string(), false);
            return false;
        };

        match mcp_client.has_mcp_tool(tool_name).await {
            Ok(result) => {
                self.mcp_tool_presence
                    .write()
                    .await
                    .insert(tool_name.to_string(), result);
                result
            }
            Err(err) => {
                warn!(
                    tool = tool_name,
                    error = %err,
                    "failed to query MCP tool presence"
                );
                self.mcp_tool_presence
                    .write()
                    .await
                    .insert(tool_name.to_string(), false);
                false
            }
        }
    }

    /// Execute an MCP tool.
    pub async fn execute_mcp_tool(&self, tool_name: &str, args: Value) -> Result<Value> {
        let client_opt = { self.mcp_client.read().unwrap().clone() };
        if let Some(mcp_client) = client_opt {
            mcp_client.execute_mcp_tool(tool_name, &args).await
        } else {
            Err(anyhow!("MCP client not available"))
        }
    }

    pub(super) async fn resolve_mcp_tool_alias(&self, tool_name: &str) -> Option<String> {
        let client_opt = { self.mcp_client.read().unwrap().clone() };
        let Some(mcp_client) = client_opt else {
            return None;
        };

        let normalized = normalize_mcp_tool_identifier(tool_name);
        if normalized.is_empty() {
            return None;
        }

        let tools = match mcp_client.list_mcp_tools().await {
            Ok(list) => list,
            Err(err) => {
                warn!(
                    "Failed to list MCP tools while resolving alias '{}': {}",
                    tool_name, err
                );
                return None;
            }
        };

        for tool in tools {
            if normalize_mcp_tool_identifier(&tool.name) == normalized {
                return Some(tool.name);
            }
        }

        None
    }

    /// Refresh MCP tools (reconnect to providers and update tool lists).
    pub async fn refresh_mcp_tools(&self) -> Result<()> {
        let mcp_client_opt = { self.mcp_client.read().unwrap().clone() };
        if let Some(mcp_client) = mcp_client_opt {
            debug!(
                "Refreshing MCP tools for {} providers",
                mcp_client.get_status().provider_count
            );

            let mut tools: Option<Vec<McpToolInfo>> = None;
            let mut last_err: Option<anyhow::Error> = None;
            for attempt in 0..3 {
                match mcp_client.list_mcp_tools().await {
                    Ok(list) => {
                        tools = Some(list);
                        break;
                    }
                    Err(err) => {
                        last_err = Some(err);
                        let jitter = (attempt as u64 * 37) % 80;
                        let pow = 2_u64.saturating_pow(attempt.min(4) as u32); // cap exponent
                        let backoff =
                            Duration::from_millis(200 * pow + jitter).min(Duration::from_secs(3));
                        warn!(
                            attempt = attempt + 1,
                            delay_ms = %backoff.as_millis(),
                            "Failed to list MCP tools, retrying with backoff"
                        );
                        tokio::time::sleep(backoff).await;
                    }
                }
            }

            let tools = match tools {
                Some(list) => list,
                None => {
                    warn!(
                        error = %last_err.unwrap_or_else(|| anyhow!("unknown MCP error")),
                        "Failed to refresh MCP tools after retries; keeping existing cache"
                    );
                    // MP-3: Record failure in circuit breaker
                    self.mcp_circuit_breaker.record_failure();
                    return Ok(());
                }
            };
            let mut provider_map: HashMap<String, Vec<String>> = HashMap::new();

            for tool in &tools {
                let registration =
                    build_mcp_registration(Arc::clone(&mcp_client), &tool.provider, tool, None);

                if !self.inventory.has_tool(registration.name())
                    && let Err(err) = self.inventory.register_tool(registration)
                {
                    warn!(
                        tool = %tool.name,
                        provider = %tool.provider,
                        error = %err,
                        "failed to register MCP proxy tool"
                    );
                }
            }

            for tool in tools {
                provider_map
                    .entry(tool.provider.clone())
                    .or_default()
                    .push(tool.name.clone());
            }

            for tools in provider_map.values_mut() {
                tools.sort();
                tools.dedup();
            }

            *self.mcp_tool_index.write().await = provider_map;
            {
                let mut presence = self.mcp_tool_presence.write().await;
                presence.clear();
                let index = self.mcp_tool_index.read().await;
                for tools in index.values() {
                    for tool in tools {
                        presence.insert(tool.clone(), true);
                    }
                }
            }

            let mcp_index = self.mcp_tool_index.read().await;
            if let Some(allowlist) = self
                .policy_gateway
                .write()
                .await
                .update_mcp_tools(&mcp_index)
                .await?
            {
                mcp_client.update_allowlist(allowlist);
            }

            self.sync_policy_catalog().await;
            // MP-3: Record success in circuit breaker
            self.mcp_circuit_breaker.record_success();
            Ok(())
        } else {
            debug!("No MCP client configured, nothing to refresh");
            Ok(())
        }
    }
}
