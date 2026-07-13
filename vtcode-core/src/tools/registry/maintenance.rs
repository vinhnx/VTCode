//! Internal maintenance helpers for ToolRegistry.

use std::time::Duration;

use crate::tool_policy::ToolPolicy;
use anyhow::Result;

use super::{ToolLatencyStats, ToolRegistry, ToolTimeoutCategory};

impl ToolRegistry {
    pub(super) async fn sync_policy_catalog(&self) {
        let lifecycle = {
            let policy_gateway = self.policy_gateway.lock().await;
            policy_gateway.full_auto_catalogue_lifecycle()
        };
        let _lifecycle_guard = lifecycle.lock().await;
        self.sync_policy_catalog_serialized().await;
    }

    async fn sync_policy_catalog_serialized(&self) {
        // Include aliases so policy prompts stay in sync with exposed names
        let available = self.available_tools().await;
        let mcp_keys = self.mcp_policy_keys().await;
        let full_auto_catalogue_config = self
            .policy_gateway
            .lock()
            .await
            .full_auto_catalogue_config();
        let full_auto_visible_policy_names = if let Some(config) = &full_auto_catalogue_config {
            Some(self.visible_policy_names(config.clone()).await)
        } else {
            None
        };
        #[cfg(test)]
        {
            let test_hooks = self
                .policy_gateway
                .lock()
                .await
                .full_auto_catalogue_test_hooks();
            test_hooks.pause_after_refresh_snapshot().await;
        }
        {
            let mut policy_gateway = self.policy_gateway.lock().await;
            policy_gateway
                .sync_available_tools(available, &mcp_keys)
                .await;
            if let (Some(config), Some(visible_policy_names)) = (
                full_auto_catalogue_config.as_ref(),
                full_auto_visible_policy_names.as_ref(),
            ) {
                policy_gateway.refresh_full_auto_catalogue(config, visible_policy_names);
            }
        }

        // Seed default permissions from tool metadata when policy manager is present
        let policy_seeds = {
            let assembly = self
                .tool_assembly
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            assembly
                .policy_seed_metadata()
                .iter()
                .map(|(name, metadata)| (name.clone(), metadata.clone()))
                .collect::<Vec<_>>()
        };
        let mut policy_gateway = self.policy_gateway.lock().await;
        if let Ok(policy) = policy_gateway.policy_manager_mut() {
            let mut seeded = 0usize;
            for (name, metadata) in policy_seeds {
                if let Some(default_policy) = metadata.default_permission() {
                    let current = policy.get_policy(&name);
                    if matches!(current, ToolPolicy::Prompt) {
                        if let Err(err) = policy
                            .seed_default_policy(&name, default_policy.clone())
                            .await
                        {
                            tracing::warn!(
                                tool = %name,
                                error = %err,
                                "Failed to seed default policy from tool metadata"
                            );
                        } else {
                            seeded += 1;
                        }
                    }
                }
            }

            if seeded > 0 {
                tracing::trace!(seeded, "Seeded default tool policies from tool assembly");
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
        // Acquire the MCP client lock once and derive both flags from it,
        // avoiding a second read-lock acquisition below.
        let mcp_client_held = self.mcp_client.read().is_some();
        if self.initialized.load(std::sync::atomic::Ordering::Relaxed)
            && (!mcp_client_held || !self.mcp_tool_index.read().await.is_empty())
        {
            return Ok(());
        }

        if mcp_client_held
            && self.mcp_tool_index.read().await.is_empty()
            && let Err(err) = self.refresh_mcp_tools().await
        {
            tracing::warn!(
                error = %err,
                "Failed to refresh MCP tools during registry initialization"
            );
        }

        self.prewarm_search_runtime();
        self.sync_policy_catalog().await;
        self.initialized
            .store(true, std::sync::atomic::Ordering::Relaxed);

        Ok(())
    }
}
