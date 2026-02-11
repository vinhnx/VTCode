use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::RwLock;
use vtcode_core::llm::provider as uni;
use vtcode_core::prompts::sort_tool_definitions;

/// Shared versioned tool-catalog state.
///
/// The turn loop can reuse cached sorted tool definitions when the catalog
/// version is unchanged, while still seeing updates from MCP/skill refreshes.
#[derive(Debug, Default)]
pub(crate) struct ToolCatalogState {
    version: AtomicU64,
    cached_sorted: RwLock<Option<(u64, Arc<Vec<uni::ToolDefinition>>)>>,
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
}
