use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::RwLock;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::llm::provider as uni;
use vtcode_core::prompts::sort_tool_definitions;

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
                Some(Arc::new(sort_tool_definitions(defs_guard.clone())))
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
}

pub(crate) fn should_expose_tool_in_mode(
    tool: &uni::ToolDefinition,
    plan_mode: bool,
    request_user_input_enabled: bool,
) -> bool {
    let Some(name) = tool.function.as_ref().map(|func| func.name.as_str()) else {
        return true;
    };

    if matches!(
        name,
        tool_names::REQUEST_USER_INPUT | tool_names::ASK_QUESTIONS | tool_names::ASK_USER_QUESTION
    ) {
        return plan_mode && request_user_input_enabled;
    }

    if name == tool_names::PLAN_TASK_TRACKER {
        return plan_mode;
    }

    if name == tool_names::TASK_TRACKER {
        return !plan_mode;
    }

    true
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

    fn function_tool(name: &str) -> uni::ToolDefinition {
        uni::ToolDefinition::function(name.to_string(), name.to_string(), json!({}))
    }

    #[test]
    fn filter_tools_for_mode_hides_plan_only_tools_in_edit_mode() {
        let tools = Arc::new(vec![
            function_tool(tool_names::UNIFIED_SEARCH),
            function_tool(tool_names::PLAN_TASK_TRACKER),
            function_tool(tool_names::REQUEST_USER_INPUT),
            function_tool(tool_names::ASK_USER_QUESTION),
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
        assert!(!names.contains(&tool_names::PLAN_TASK_TRACKER));
        assert!(!names.contains(&tool_names::REQUEST_USER_INPUT));
        assert!(!names.contains(&tool_names::ASK_USER_QUESTION));
    }

    #[test]
    fn filter_tools_for_mode_hides_task_tracker_in_plan_mode() {
        let tools = Arc::new(vec![
            function_tool(tool_names::UNIFIED_SEARCH),
            function_tool(tool_names::PLAN_TASK_TRACKER),
            function_tool(tool_names::REQUEST_USER_INPUT),
            function_tool(tool_names::ASK_USER_QUESTION),
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
        assert!(names.contains(&tool_names::ASK_USER_QUESTION));
        assert!(!names.contains(&tool_names::TASK_TRACKER));
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
            .filtered_snapshot_with_stats(&tools, false, true)
            .await
            .snapshot;
        let second = state
            .filtered_snapshot_with_stats(&tools, false, true)
            .await
            .snapshot;
        assert!(first.is_none());
        assert!(second.is_none());

        let cache_guard = state.cached_filtered.read().await;
        assert_eq!(cache_guard.len(), 1);
    }
}
