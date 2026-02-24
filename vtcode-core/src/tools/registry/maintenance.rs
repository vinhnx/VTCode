//! Internal maintenance helpers for ToolRegistry.

use std::time::Duration;

use crate::tool_policy::ToolPolicy;
use anyhow::Result;

use super::{ToolLatencyStats, ToolRegistry, ToolTimeoutCategory};

impl ToolRegistry {
    pub(super) async fn sync_policy_catalog(&self) {
        // Include aliases so policy prompts stay in sync with exposed names
        let available = self.available_tools().await;
        let mcp_keys = self.mcp_policy_keys().await;
        self.policy_gateway
            .write()
            .await
            .sync_available_tools(available, &mcp_keys)
            .await;

        // Seed default permissions from tool metadata when policy manager is present
        let registrations = self.inventory.registration_metadata();
        let mut policy_gateway = self.policy_gateway.write().await;
        if let Ok(policy) = policy_gateway.policy_manager_mut() {
            let mut seeded = 0usize;
            for (name, metadata) in registrations {
                if let Some(default_policy) = metadata.default_permission() {
                    let current = policy.get_policy(&name);
                    if matches!(current, ToolPolicy::Prompt) {
                        if let Err(err) = policy.set_policy(&name, default_policy.clone()).await {
                            tracing::warn!(
                                tool = %name,
                                error = %err,
                                "Failed to seed default policy from tool metadata"
                            );
                        } else {
                            seeded += 1;
                            // Apply same default to aliases so they behave consistently
                            for alias in metadata.aliases() {
                                if let Err(err) =
                                    policy.set_policy(alias, default_policy.clone()).await
                                {
                                    tracing::warn!(
                                        tool = %name,
                                        alias = %alias,
                                        error = %err,
                                        "Failed to seed default policy for alias"
                                    );
                                }
                            }
                        }
                    }
                }
            }

            if seeded > 0 {
                tracing::debug!(seeded, "Seeded default tool policies from registrations");
            }
        }
    }

    pub(super) fn initialize_resiliency_trackers(&self) {
        let categories = [
            ToolTimeoutCategory::Default,
            ToolTimeoutCategory::Pty,
            ToolTimeoutCategory::Mcp,
        ];
        let mut state = self.resiliency.lock();
        for category in categories {
            state.failure_trackers.entry(category).or_default();
            state.success_trackers.entry(category).or_insert(0);
            state
                .latency_stats
                .entry(category)
                .or_insert_with(|| ToolLatencyStats::new(50));
            state
                .adaptive_timeout_ceiling
                .entry(category)
                .or_insert_with(|| Duration::from_secs(0));
        }
    }

    pub async fn initialize_async(&self) -> Result<()> {
        let mcp_client_is_none = {
            self.mcp_client
                .read()
                .ok()
                .map(|g| g.is_none())
                .unwrap_or(true)
        };
        if self.initialized.load(std::sync::atomic::Ordering::Relaxed)
            && (mcp_client_is_none || !self.mcp_tool_index.read().await.is_empty())
        {
            return Ok(());
        }

        let mcp_client_is_some = {
            self.mcp_client
                .read()
                .ok()
                .map(|g| g.is_some())
                .unwrap_or(false)
        };
        if mcp_client_is_some
            && self.mcp_tool_index.read().await.is_empty()
            && let Err(err) = self.refresh_mcp_tools().await
        {
            tracing::warn!(
                error = %err,
                "Failed to refresh MCP tools during registry initialization"
            );
        }

        self.sync_policy_catalog().await;
        self.initialized
            .store(true, std::sync::atomic::Ordering::Relaxed);

        Ok(())
    }
}
