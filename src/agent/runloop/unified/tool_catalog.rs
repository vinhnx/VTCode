use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::RwLock;
use tracing::{debug, info};
use vtcode_core::core::agent::features::FeatureSet;
use vtcode_core::llm::provider as uni;

/// Shared versioned tool-catalog state.
///
/// The turn loop can reuse cached sorted tool definitions when the catalog
/// version is unchanged, while still seeing updates from MCP/skill refreshes.
#[derive(Debug, Clone)]
struct FilteredCacheEntry {
    version: u64,
    plan_mode: bool,
    request_user_input_enabled: bool,
    snapshot: Option<Arc<Vec<uni::ToolDefinition>>>,
}

#[derive(Debug, Clone)]
pub(crate) struct FilteredSnapshotResult {
    pub snapshot: Option<Arc<Vec<uni::ToolDefinition>>>,
    pub cache_hit: bool,
}

#[derive(Debug, Default)]
pub(crate) struct ToolCatalogState {
    version: AtomicU64,
    cache_epoch: AtomicU64,
    pending_refresh_reasons: Mutex<BTreeSet<String>>,
    expanded_tool_names: Mutex<BTreeSet<String>>,
    cached_sorted: RwLock<Option<(u64, Arc<Vec<uni::ToolDefinition>>)>>,
    cached_filtered: RwLock<Vec<FilteredCacheEntry>>,
}

impl ToolCatalogState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bump_version(&self) -> u64 {
        self.version.fetch_add(1, Ordering::AcqRel) + 1
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
        if pending.insert(reason.to_string()) {
            debug!(
                reason,
                pending_refreshes = pending.len(),
                "tool catalog refresh queued for next cache epoch"
            );
        }
    }

    pub fn note_explicit_refresh(&self, reason: &str) -> u64 {
        self.bump_cache_epoch(reason, None)
    }

    pub async fn note_tool_references(
        &self,
        tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
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
            None
        } else {
            Some(self.bump_cache_epoch("tool_search_expansion", Some(newly_expanded)))
        }
    }

    pub async fn sorted_snapshot(
        &self,
        tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
    ) -> Option<Arc<Vec<uni::ToolDefinition>>> {
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

        let next_snapshot = {
            let defs_guard = tools.read().await;
            if defs_guard.is_empty() {
                None
            } else {
                let expanded_tool_names = self
                    .expanded_tool_names
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                let mut snapshot = defs_guard.clone();
                if !expanded_tool_names.is_empty() {
                    for tool in &mut snapshot {
                        if expanded_tool_names.contains(tool.function_name()) {
                            tool.defer_loading = None;
                        }
                    }
                }
                Some(Arc::new(snapshot))
            }
        };

        let mut cache_guard = self.cached_sorted.write().await;
        *cache_guard = next_snapshot
            .as_ref()
            .map(|snapshot| (version, Arc::clone(snapshot)));

        next_snapshot
    }

    /// Return a mode-filtered tool snapshot cached by tool catalog version + mode flags.
    pub async fn filtered_snapshot_with_stats(
        &self,
        tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
        plan_mode: bool,
        request_user_input_enabled: bool,
    ) -> FilteredSnapshotResult {
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
            return FilteredSnapshotResult {
                snapshot: entry.snapshot,
                cache_hit: true,
            };
        }

        let filtered = filter_tools_for_mode(
            self.sorted_snapshot(tools).await,
            plan_mode,
            request_user_input_enabled,
        );

        let mut cache_guard = self.cached_filtered.write().await;
        cache_guard.retain(|entry| entry.version == version);
        if let Some(existing) = cache_guard.iter_mut().find(|entry| {
            entry.plan_mode == plan_mode
                && entry.request_user_input_enabled == request_user_input_enabled
        }) {
            existing.snapshot = filtered.as_ref().map(Arc::clone);
        } else {
            cache_guard.push(FilteredCacheEntry {
                version,
                plan_mode,
                request_user_input_enabled,
                snapshot: filtered.as_ref().map(Arc::clone),
            });
        }

        FilteredSnapshotResult {
            snapshot: filtered,
            cache_hit: false,
        }
    }

    #[cfg(test)]
    fn pending_refresh_reasons(&self) -> Vec<String> {
        self.pending_refresh_reasons
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    fn bump_cache_epoch(&self, reason: &str, expanded_tools: Option<Vec<String>>) -> u64 {
        let cache_epoch = self.cache_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        let version = self.bump_version();
        let pending_refresh_reasons = std::mem::take(
            &mut *self
                .pending_refresh_reasons
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        info!(
            cache_epoch,
            version,
            reason,
            pending_refreshes = pending_refresh_reasons.len(),
            pending_refresh_reasons = ?pending_refresh_reasons,
            expanded_tools = ?expanded_tools,
            "tool catalog cache epoch bumped"
        );
        cache_epoch
    }
}

pub(crate) fn tool_catalog_change_notifier(
    tool_catalog: &Arc<ToolCatalogState>,
) -> Arc<dyn Fn(&'static str) + Send + Sync> {
    let tool_catalog = Arc::clone(tool_catalog);
    Arc::new(move |reason| {
        tool_catalog.note_explicit_refresh(reason);
    })
}

pub(crate) fn should_expose_tool_in_mode(
    tool: &uni::ToolDefinition,
    plan_mode: bool,
    request_user_input_enabled: bool,
) -> bool {
    let Some(name) = tool.function.as_ref().map(|func| func.name.as_str()) else {
        return true;
    };

    FeatureSet::tool_enabled_for_mode(name, plan_mode, request_user_input_enabled)
}

pub(crate) fn filter_tools_for_mode(
    tools: Option<Arc<Vec<uni::ToolDefinition>>>,
    plan_mode: bool,
    request_user_input_enabled: bool,
) -> Option<Arc<Vec<uni::ToolDefinition>>> {
    let tools = tools?;
    if tools
        .iter()
        .all(|tool| should_expose_tool_in_mode(tool, plan_mode, request_user_input_enabled))
    {
        return Some(tools);
    }

    let filtered: Vec<uni::ToolDefinition> = tools
        .iter()
        .filter(|tool| should_expose_tool_in_mode(tool, plan_mode, request_user_input_enabled))
        .cloned()
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(Arc::new(filtered))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::sync::RwLock;
    use vtcode_core::config::constants::tools as tool_names;

    fn function_tool(name: &str) -> uni::ToolDefinition {
        uni::ToolDefinition::function(name.to_string(), name.to_string(), json!({}))
    }

    #[test]
    fn change_notifier_bumps_version() {
        let tool_catalog = Arc::new(ToolCatalogState::new());
        let notifier = tool_catalog_change_notifier(&tool_catalog);

        assert_eq!(tool_catalog.current_version(), 0);
        assert_eq!(tool_catalog.current_epoch(), 0);
        notifier("load_skill");
        assert_eq!(tool_catalog.current_version(), 1);
        assert_eq!(tool_catalog.current_epoch(), 1);
    }

    #[test]
    fn filter_tools_for_mode_hides_only_plan_specific_tools_in_edit_mode() {
        let tools = Arc::new(vec![
            function_tool(tool_names::UNIFIED_SEARCH),
            function_tool(tool_names::PLAN_TASK_TRACKER),
            function_tool(tool_names::REQUEST_USER_INPUT),
            function_tool(tool_names::TASK_TRACKER),
        ]);

        let filtered = filter_tools_for_mode(Some(Arc::clone(&tools)), false, true)
            .expect("filtered tool list");
        let names: Vec<&str> = filtered
            .iter()
            .filter_map(|tool| tool.function.as_ref().map(|func| func.name.as_str()))
            .collect();

        assert!(names.contains(&tool_names::UNIFIED_SEARCH));
        assert!(names.contains(&tool_names::TASK_TRACKER));
        assert!(names.contains(&tool_names::REQUEST_USER_INPUT));
        assert!(!names.contains(&tool_names::PLAN_TASK_TRACKER));
    }

    #[test]
    fn filter_tools_for_mode_keeps_task_tracker_in_plan_mode() {
        let tools = Arc::new(vec![
            function_tool(tool_names::UNIFIED_SEARCH),
            function_tool(tool_names::PLAN_TASK_TRACKER),
            function_tool(tool_names::REQUEST_USER_INPUT),
            function_tool(tool_names::TASK_TRACKER),
        ]);

        let filtered = filter_tools_for_mode(Some(Arc::clone(&tools)), true, true)
            .expect("filtered tool list");
        let names: Vec<&str> = filtered
            .iter()
            .filter_map(|tool| tool.function.as_ref().map(|func| func.name.as_str()))
            .collect();

        assert!(names.contains(&tool_names::UNIFIED_SEARCH));
        assert!(names.contains(&tool_names::PLAN_TASK_TRACKER));
        assert!(names.contains(&tool_names::REQUEST_USER_INPUT));
        assert!(names.contains(&tool_names::TASK_TRACKER));
    }

    #[test]
    fn filter_tools_for_mode_respects_request_user_input_toggle() {
        let tools = Arc::new(vec![
            function_tool(tool_names::UNIFIED_SEARCH),
            function_tool(tool_names::REQUEST_USER_INPUT),
        ]);

        let filtered = filter_tools_for_mode(Some(tools), true, false).expect("filtered tool list");
        let names: Vec<&str> = filtered
            .iter()
            .filter_map(|tool| tool.function.as_ref().map(|func| func.name.as_str()))
            .collect();

        assert!(names.contains(&tool_names::UNIFIED_SEARCH));
        assert!(!names.contains(&tool_names::REQUEST_USER_INPUT));
    }

    #[tokio::test]
    async fn filtered_snapshot_reuses_cached_mode_projection_until_version_changes() {
        let state = ToolCatalogState::new();
        let tools = Arc::new(RwLock::new(vec![
            function_tool(tool_names::UNIFIED_SEARCH),
            function_tool(tool_names::REQUEST_USER_INPUT),
            function_tool(tool_names::TASK_TRACKER),
        ]));

        let first = state
            .filtered_snapshot_with_stats(&tools, true, true)
            .await
            .snapshot
            .expect("first filtered snapshot");
        let second = state
            .filtered_snapshot_with_stats(&tools, true, true)
            .await
            .snapshot
            .expect("second filtered snapshot");
        assert!(Arc::ptr_eq(&first, &second));

        state.bump_version();
        let third = state
            .filtered_snapshot_with_stats(&tools, true, true)
            .await
            .snapshot
            .expect("third filtered snapshot");
        assert!(!Arc::ptr_eq(&second, &third));
    }

    #[tokio::test]
    async fn filtered_snapshot_caches_none_without_duplicate_entries() {
        let state = ToolCatalogState::new();
        let tools = Arc::new(RwLock::new(vec![function_tool(
            tool_names::REQUEST_USER_INPUT,
        )]));

        let first = state
            .filtered_snapshot_with_stats(&tools, false, false)
            .await
            .snapshot;
        let second = state
            .filtered_snapshot_with_stats(&tools, false, false)
            .await
            .snapshot;
        assert!(first.is_none());
        assert!(second.is_none());

        let cache_guard = state.cached_filtered.read().await;
        assert_eq!(cache_guard.len(), 1);
    }

    #[tokio::test]
    async fn background_refresh_keeps_cached_snapshot_until_epoch_bump() {
        let state = ToolCatalogState::new();
        let tools = Arc::new(RwLock::new(vec![function_tool(tool_names::UNIFIED_SEARCH)]));

        let first = state.sorted_snapshot(&tools).await.expect("first snapshot");
        assert_eq!(first.len(), 1);

        {
            let mut defs = tools.write().await;
            defs.push(function_tool("mcp_deferred_tool"));
        }
        state.mark_pending_refresh("mcp_background_refresh");

        let second = state
            .sorted_snapshot(&tools)
            .await
            .expect("second snapshot");
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(second.len(), 1);

        state.note_explicit_refresh("mcp_manual_refresh");

        let third = state.sorted_snapshot(&tools).await.expect("third snapshot");
        assert!(!Arc::ptr_eq(&second, &third));
        assert_eq!(third.len(), 2);
    }

    #[test]
    fn pending_refreshes_wait_for_explicit_epoch() {
        let state = ToolCatalogState::new();

        state.mark_pending_refresh("mcp_background_refresh");
        state.mark_pending_refresh("mcp_background_refresh");

        assert_eq!(
            state.pending_refresh_reasons(),
            vec!["mcp_background_refresh".to_string()]
        );
        assert_eq!(state.current_epoch(), 0);

        state.note_explicit_refresh("mcp_manual_refresh");

        assert!(state.pending_refresh_reasons().is_empty());
        assert_eq!(state.current_epoch(), 1);
        assert_eq!(state.current_version(), 1);
    }

    #[tokio::test]
    async fn tool_references_expand_deferred_tools_on_next_epoch() {
        let state = ToolCatalogState::new();
        let tools = Arc::new(RwLock::new(vec![
            function_tool(tool_names::UNIFIED_SEARCH),
            function_tool("deferred_lookup").with_defer_loading(true),
        ]));

        let before = state.sorted_snapshot(&tools).await.expect("snapshot");
        let deferred = before
            .iter()
            .find(|tool| tool.function_name() == "deferred_lookup")
            .expect("deferred tool");
        assert_eq!(deferred.defer_loading, Some(true));

        let epoch = state
            .note_tool_references(&tools, &["deferred_lookup".to_string()])
            .await
            .expect("tool search expansion");
        assert_eq!(epoch, 1);

        let after = state.sorted_snapshot(&tools).await.expect("snapshot");
        let expanded = after
            .iter()
            .find(|tool| tool.function_name() == "deferred_lookup")
            .expect("expanded tool");
        assert!(expanded.defer_loading.is_none());
    }
}
