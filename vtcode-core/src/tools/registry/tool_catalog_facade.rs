use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::RwLock;

use crate::core::agent::harness_kernel::{
    SessionToolCatalogSnapshot, filter_tool_definitions_for_mode,
};
use crate::llm::provider::ToolDefinition;
use crate::prompts::sort_tool_definitions;

#[derive(Debug, Clone)]
struct FilteredCacheEntry {
    version: u64,
    plan_mode: bool,
    request_user_input_enabled: bool,
    snapshot: SessionToolCatalogSnapshot,
}

#[derive(Debug, Default)]
pub struct SessionToolCatalogState {
    version: AtomicU64,
    cache_epoch: AtomicU64,
    pending_refresh_reasons: Mutex<BTreeSet<String>>,
    expanded_tool_names: Mutex<BTreeSet<String>>,
    cached_sorted: RwLock<Option<(u64, Arc<Vec<ToolDefinition>>)>>,
    cached_filtered: RwLock<Vec<FilteredCacheEntry>>,
}

impl SessionToolCatalogState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn current_version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub fn current_epoch(&self) -> u64 {
        self.cache_epoch.load(Ordering::Acquire)
    }

    pub fn mark_pending_refresh(&self, reason: &str) {
        let mut pending = self
            .pending_refresh_reasons
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        pending.insert(reason.to_string());
    }

    pub fn note_explicit_refresh(&self, reason: &str) -> u64 {
        let cache_epoch = self.cache_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        let version = self.version.fetch_add(1, Ordering::AcqRel) + 1;
        let pending_refreshes = std::mem::take(
            &mut *self
                .pending_refresh_reasons
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        tracing::info!(
            cache_epoch,
            version,
            reason,
            pending_refreshes = ?pending_refreshes,
            "tool catalog cache epoch bumped"
        );
        cache_epoch
    }

    pub fn change_notifier(self: &Arc<Self>) -> Arc<dyn Fn(&'static str) + Send + Sync> {
        let state = Arc::clone(self);
        Arc::new(move |reason| {
            state.note_explicit_refresh(reason);
        })
    }

    pub async fn note_tool_references(
        &self,
        tools: &Arc<RwLock<Vec<ToolDefinition>>>,
        tool_references: &[String],
    ) -> Option<u64> {
        if tool_references.is_empty() {
            return None;
        }

        let discoverable: BTreeSet<String> = {
            let defs = tools.read().await;
            defs.iter()
                .filter(|tool| tool.defer_loading == Some(true))
                .map(|tool| tool.function_name().to_string())
                .collect()
        };
        if discoverable.is_empty() {
            return None;
        }

        let mut newly_expanded = Vec::new();
        {
            let mut expanded = self
                .expanded_tool_names
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for tool_name in tool_references {
                if discoverable.contains(tool_name) && expanded.insert(tool_name.clone()) {
                    newly_expanded.push(tool_name.clone());
                }
            }
        }

        if newly_expanded.is_empty() {
            return None;
        }

        tracing::info!(
            expanded_tools = ?newly_expanded,
            "tool references expanded deferred tool definitions"
        );
        Some(self.note_explicit_refresh("tool_search_expansion"))
    }

    pub async fn filtered_snapshot_with_stats(
        &self,
        tools: &Arc<RwLock<Vec<ToolDefinition>>>,
        plan_mode: bool,
        request_user_input_enabled: bool,
    ) -> SessionToolCatalogSnapshot {
        let version = self.current_version();

        if let Some(entry) = {
            let cache_guard = self.cached_filtered.read().await;
            cache_guard
                .iter()
                .find(|entry| {
                    entry.version == version
                        && entry.plan_mode == plan_mode
                        && entry.request_user_input_enabled == request_user_input_enabled
                })
                .cloned()
        } {
            return entry.snapshot.with_cache_hit(true);
        }

        let filtered = filter_tool_definitions_for_mode(
            self.sorted_snapshot(tools).await,
            plan_mode,
            request_user_input_enabled,
        );
        let snapshot = SessionToolCatalogSnapshot::new(
            version,
            self.current_epoch(),
            plan_mode,
            request_user_input_enabled,
            filtered,
            false,
        );

        let mut cache_guard = self.cached_filtered.write().await;
        cache_guard.retain(|entry| entry.version == version);
        cache_guard.push(FilteredCacheEntry {
            version,
            plan_mode,
            request_user_input_enabled,
            snapshot: snapshot.clone(),
        });
        snapshot
    }

    pub fn snapshot_for_defs(
        &self,
        defs: Vec<ToolDefinition>,
        plan_mode: bool,
        request_user_input_enabled: bool,
    ) -> SessionToolCatalogSnapshot {
        let defs = sort_snapshot_definitions(defs);
        let filtered = filter_tool_definitions_for_mode(
            (!defs.is_empty()).then(|| Arc::new(defs)),
            plan_mode,
            request_user_input_enabled,
        );
        SessionToolCatalogSnapshot::new(
            self.current_version(),
            self.current_epoch(),
            plan_mode,
            request_user_input_enabled,
            filtered,
            false,
        )
    }

    async fn sorted_snapshot(
        &self,
        tools: &Arc<RwLock<Vec<ToolDefinition>>>,
    ) -> Option<Arc<Vec<ToolDefinition>>> {
        let version = self.current_version();
        if let Some(snapshot) = {
            let cache_guard = self.cached_sorted.read().await;
            cache_guard
                .as_ref()
                .and_then(|(cached_version, defs)| (*cached_version == version).then_some(defs))
                .map(Arc::clone)
        } {
            return Some(snapshot);
        }

        let expanded_tool_names = self
            .expanded_tool_names
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let next_snapshot = {
            let defs_guard = tools.read().await;
            if defs_guard.is_empty() {
                None
            } else {
                let mut snapshot = defs_guard.clone();
                if !expanded_tool_names.is_empty() {
                    for tool in &mut snapshot {
                        if expanded_tool_names.contains(tool.function_name()) {
                            tool.defer_loading = None;
                        }
                    }
                }
                Some(Arc::new(sort_snapshot_definitions(snapshot)))
            }
        };

        let mut cache_guard = self.cached_sorted.write().await;
        *cache_guard = next_snapshot
            .as_ref()
            .map(|snapshot| (version, Arc::clone(snapshot)));
        next_snapshot
    }
}

fn sort_snapshot_definitions(defs: Vec<ToolDefinition>) -> Vec<ToolDefinition> {
    sort_tool_definitions(defs)
}

impl super::ToolRegistry {
    pub fn tool_catalog_state(&self) -> Arc<SessionToolCatalogState> {
        Arc::clone(&self.tool_catalog_state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::tools;

    fn function_tool(name: &str) -> ToolDefinition {
        ToolDefinition::function(name.to_string(), name.to_string(), serde_json::json!({}))
    }

    #[tokio::test]
    async fn filtered_snapshot_reuses_cached_projection_until_refresh() {
        let state = SessionToolCatalogState::new();
        let tools = Arc::new(RwLock::new(vec![
            function_tool(tools::UNIFIED_SEARCH),
            function_tool(tools::REQUEST_USER_INPUT),
        ]));

        let first = state.filtered_snapshot_with_stats(&tools, true, true).await;
        let second = state.filtered_snapshot_with_stats(&tools, true, true).await;

        assert!(!first.cache_hit);
        assert!(second.cache_hit);
        assert_eq!(first.tool_catalog_hash, second.tool_catalog_hash);
        assert_eq!(first.available_tools(), second.available_tools());

        state.note_explicit_refresh("test");
        let third = state.filtered_snapshot_with_stats(&tools, true, true).await;
        assert_eq!(third.version, 1);
    }

    #[test]
    fn snapshot_for_defs_sorts_tool_order_for_stable_projections() {
        let state = SessionToolCatalogState::new();
        let snapshot = state.snapshot_for_defs(
            vec![
                function_tool("z_tool"),
                function_tool(tools::UNIFIED_FILE),
                function_tool("a_tool"),
            ],
            false,
            false,
        );

        let names: Vec<&str> = snapshot
            .snapshot
            .as_ref()
            .expect("sorted snapshot")
            .iter()
            .map(|tool| tool.function_name())
            .collect();

        assert_eq!(names, vec![tools::UNIFIED_FILE, "a_tool", "z_tool"]);
    }
}
