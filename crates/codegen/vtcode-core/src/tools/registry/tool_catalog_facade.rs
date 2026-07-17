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

// ─── Client-Side Tool Search Index ──────────────────────────────────────────
//
// The search engine now lives in `tool_search_index`, isolated behind the
// `ToolSearchEngine` trait. This facade only depends on that interface so the
// scoring implementation can be swapped without changes here.
pub use super::tool_search_index::{ToolEmbeddingIndex, ToolSearchEngine, ToolSearchResult};

// ─── Derived Tool Groups ─────────────────────────────────────────────────────

/// A namespace-derived grouping of tools, computed from
/// [`ToolDefinition::namespace`]. Purely a client-side view for search
/// results and diagnostics.
///
/// IMPORTANT DESIGN CONSTRAINT: groups are never sent to the provider as
/// wire-level "stub" tools. Provider-hosted tool search (Anthropic's
/// tool_search_tool_* beta, OpenAI's hosted tool_search) requires the full
/// deferred tool definitions to already be present in the request payload --
/// there is no server-side concept of a group stub the model can expand
/// later. Sending a fake stub entry (e.g. a placeholder tool named after the
/// group) would invite the model to hallucinate a call to a tool that does
/// not exist. Namespace/group metadata therefore stays entirely client-side:
/// it powers local BM25 ranking (see `ToolEmbeddingIndex::build`) and the
/// `by_group` field in local tool search results, but is never itself placed
/// on the wire.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolGroup {
    /// The namespace name (e.g. an MCP server name).
    pub name: String,
    /// The namespace description. Always `Some` for namespaced groups --
    /// `tool_groups` only creates a group when a tool carries a namespace,
    /// and it takes the description from that namespace, not per-tool.
    pub description: Option<String>,
    /// Total number of tools in this group.
    pub tool_count: usize,
    /// Number of tools in this group that are currently deferred
    /// (`defer_loading == Some(true)`).
    pub deferred_count: usize,
}

/// Fold tool definitions into namespace-derived groups.
///
/// Tools without a namespace are skipped -- they are core tools and do not
/// belong to any group. Groups are returned sorted by name for a
/// deterministic, stable ordering across calls.
#[must_use]
pub fn tool_groups(tools: &[ToolDefinition]) -> Vec<ToolGroup> {
    let mut groups: std::collections::BTreeMap<String, ToolGroup> =
        std::collections::BTreeMap::new();

    for tool in tools {
        let Some(namespace) = tool.namespace.as_ref() else {
            continue;
        };

        let group = groups.entry(namespace.name.clone()).or_insert_with(|| ToolGroup {
            name: namespace.name.clone(),
            description: Some(namespace.description.clone()),
            tool_count: 0,
            deferred_count: 0,
        });
        group.tool_count += 1;
        if tool.defer_loading == Some(true) {
            group.deferred_count += 1;
        }
    }

    groups.into_values().collect()
}

#[derive(Debug, Clone)]
struct FilteredCacheEntry {
    version: u64,
    planning_active: bool,
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
    /// Client-side embedding index for tool search. Rebuilt when the tool
    /// catalog epoch changes (tools added/removed, deferred tools expanded).
    embedding_index: RwLock<Option<ToolEmbeddingIndex>>,
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

    /// Get or build the client-side tool embedding index.
    ///
    /// The index is rebuilt when the catalog epoch changes (tools added/removed,
    /// deferred tools expanded). This is the infrastructure for client-side
    /// embedding-guided tool search described in the Microsoft article.
    pub async fn embedding_index(
        &self,
        tools: &Arc<RwLock<Vec<ToolDefinition>>>,
    ) -> Arc<ToolEmbeddingIndex> {
        let current_epoch = self.current_epoch();

        // Check if the cached index is still valid
        {
            let index_guard = self.embedding_index.read().await;
            if let Some(ref index) = *index_guard {
                if index.epoch() == current_epoch {
                    return Arc::new(index.clone());
                }
            }
        }

        // Build a new index
        let defs = tools.read().await;
        let index = ToolEmbeddingIndex::build(&defs, current_epoch);
        drop(defs);

        let arc_index = Arc::new(index.clone());
        {
            let mut index_guard = self.embedding_index.write().await;
            *index_guard = Some(index);
        }

        arc_index
    }

    /// Search tools by query using the client-side embedding index.
    ///
    /// Returns tools ranked by BM25-inspired relevance scoring. This is the
    /// client-side alternative to provider-side tool search, matching the
    /// article's description: "Rather than lexical matching over tool names and
    /// descriptions, we use our embedding model to compare the query against
    /// vector representations of every available tool."
    pub async fn search_tools(
        &self,
        tools: &Arc<RwLock<Vec<ToolDefinition>>>,
        query: &str,
        max_results: usize,
    ) -> Vec<ToolSearchResult> {
        let index = self.embedding_index(tools).await;
        index.search(query, max_results)
    }

    pub async fn filtered_snapshot_with_stats(
        &self,
        tools: &Arc<RwLock<Vec<ToolDefinition>>>,
        planning_active: bool,
        request_user_input_enabled: bool,
    ) -> SessionToolCatalogSnapshot {
        let version = self.current_version();

        if let Some(entry) = {
            let cache_guard = self.cached_filtered.read().await;
            cache_guard
                .iter()
                .find(|entry| {
                    entry.version == version
                        && entry.planning_active == planning_active
                        && entry.request_user_input_enabled == request_user_input_enabled
                })
                .cloned()
        } {
            return entry.snapshot.with_cache_hit(true);
        }

        let filtered = filter_tool_definitions_for_mode(
            self.sorted_snapshot(tools).await,
            planning_active,
            request_user_input_enabled,
        );
        let snapshot = SessionToolCatalogSnapshot::new(
            version,
            self.current_epoch(),
            planning_active,
            request_user_input_enabled,
            filtered,
            false,
        );

        let mut cache_guard = self.cached_filtered.write().await;
        cache_guard.retain(|entry| entry.version == version);
        cache_guard.push(FilteredCacheEntry {
            version,
            planning_active,
            request_user_input_enabled,
            snapshot: snapshot.clone(),
        });
        snapshot
    }

    pub fn snapshot_for_defs(
        &self,
        defs: Vec<ToolDefinition>,
        planning_active: bool,
        request_user_input_enabled: bool,
    ) -> SessionToolCatalogSnapshot {
        let defs = sort_snapshot_definitions(defs);
        let filtered = filter_tool_definitions_for_mode(
            (!defs.is_empty()).then(|| Arc::new(defs)),
            planning_active,
            request_user_input_enabled,
        );
        SessionToolCatalogSnapshot::new(
            self.current_version(),
            self.current_epoch(),
            planning_active,
            request_user_input_enabled,
            filtered,
            false,
        )
    }

    pub fn snapshot_for_stable_defs_with_active_names(
        &self,
        defs: Vec<ToolDefinition>,
        active_tool_names: Vec<String>,
        planning_active: bool,
        request_user_input_enabled: bool,
    ) -> SessionToolCatalogSnapshot {
        let defs = sort_snapshot_definitions(defs);
        let requested_active_tool_names: BTreeSet<String> = active_tool_names.into_iter().collect();
        let active_tool_names = defs
            .iter()
            .filter_map(|def| {
                let name = def.function_name();
                requested_active_tool_names.contains(name).then(|| name.to_string())
            })
            .collect();
        let active_tool_names = Arc::new(active_tool_names);
        SessionToolCatalogSnapshot::with_active_tool_names(
            self.current_version(),
            self.current_epoch(),
            planning_active,
            request_user_input_enabled,
            (!defs.is_empty()).then(|| Arc::new(defs)),
            active_tool_names,
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
        *cache_guard = next_snapshot.as_ref().map(|snapshot| (version, Arc::clone(snapshot)));
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

    /// Attach the session's live model-facing tool definitions so registry
    /// consumers (e.g. local tool search) can read and un-defer them.
    ///
    /// The interactive runloop calls this once, right after it builds the
    /// `Arc<RwLock<Vec<ToolDefinition>>>` it hands to the provider. Headless
    /// paths may never call this; every consumer of `session_model_tools()`
    /// must treat `None` as "no session tool list available" and degrade
    /// gracefully rather than error.
    pub fn attach_session_model_tools(&self, tools: Arc<RwLock<Vec<ToolDefinition>>>) {
        let mut guard = self
            .session_model_tools
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some(tools);
    }

    /// Fetch the attached session tool list, if any.
    ///
    /// Clones the inner `Arc` out from behind the std `RwLock` so callers
    /// never hold that lock across an `.await`.
    pub fn session_model_tools(&self) -> Option<Arc<RwLock<Vec<ToolDefinition>>>> {
        self.session_model_tools
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
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
            function_tool(tools::CODE_SEARCH),
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

        assert_eq!(names, vec!["a_tool", tools::UNIFIED_FILE, "z_tool"]);
    }

    #[test]
    fn snapshot_for_stable_defs_preserves_full_order_with_normalized_active_subset() {
        let state = SessionToolCatalogState::new();
        let snapshot = state.snapshot_for_stable_defs_with_active_names(
            vec![
                function_tool("z_tool"),
                function_tool(tools::UNIFIED_EXEC),
                function_tool("a_tool"),
            ],
            vec![
                "phantom_tool".to_string(),
                "z_tool".to_string(),
                "a_tool".to_string(),
                "a_tool".to_string(),
            ],
            false,
            false,
        );

        let names: Vec<&str> = snapshot
            .snapshot
            .as_ref()
            .expect("stable snapshot")
            .iter()
            .map(|tool| tool.function_name())
            .collect();

        assert_eq!(names, vec!["a_tool", tools::UNIFIED_EXEC, "z_tool"]);
        assert_eq!(
            snapshot.active_tool_names.as_ref(),
            &vec!["a_tool".to_string(), "z_tool".to_string()]
        );
        assert_eq!(snapshot.catalog_tools(), 3);
        assert_eq!(snapshot.available_tools(), 2);
    }

    fn tool_with_desc(name: &str, desc: &str) -> ToolDefinition {
        ToolDefinition::function(name.to_string(), desc.to_string(), serde_json::json!({}))
    }

    #[tokio::test]
    async fn session_model_tools_defaults_to_none_and_round_trips_through_attach() {
        let registry =
            crate::tools::registry::ToolRegistry::new(std::path::PathBuf::from("/tmp/test")).await;

        assert!(
            registry.session_model_tools().is_none(),
            "session tools should be unattached by default"
        );

        let session_tools = Arc::new(RwLock::new(vec![function_tool("demo_tool")]));
        registry.attach_session_model_tools(Arc::clone(&session_tools));

        let fetched = registry
            .session_model_tools()
            .expect("session tools should be attached after attach_session_model_tools");
        assert!(
            Arc::ptr_eq(&fetched, &session_tools),
            "getter should return the same Arc that was attached"
        );
    }

    #[test]
    fn tool_groups_folds_namespaced_tools_and_skips_ungrouped_core_tools() {
        use crate::llm::provider::ToolNamespace;

        let core_tool = function_tool("read_file");

        let mut docs_search = tool_with_desc("docs_search", "Search the docs server");
        docs_search.namespace = Some(ToolNamespace {
            name: "docs".to_string(),
            description: "Tools provided by MCP server 'docs'".to_string(),
        });
        docs_search.defer_loading = Some(true);

        let mut docs_fetch = tool_with_desc("docs_fetch", "Fetch a doc by id");
        docs_fetch.namespace = Some(ToolNamespace {
            name: "docs".to_string(),
            description: "Tools provided by MCP server 'docs'".to_string(),
        });

        let mut github_issue = tool_with_desc("github_open_issue", "Open a GitHub issue");
        github_issue.namespace = Some(ToolNamespace {
            name: "github".to_string(),
            description: "Tools provided by MCP server 'github'".to_string(),
        });
        github_issue.defer_loading = Some(true);

        let tools = vec![core_tool, docs_search, docs_fetch, github_issue];
        let groups = tool_groups(&tools);

        // Sorted by name: "docs" before "github".
        assert_eq!(groups.len(), 2);

        let docs_group = &groups[0];
        assert_eq!(docs_group.name, "docs");
        assert_eq!(docs_group.tool_count, 2);
        assert_eq!(docs_group.deferred_count, 1);

        let github_group = &groups[1];
        assert_eq!(github_group.name, "github");
        assert_eq!(github_group.tool_count, 1);
        assert_eq!(github_group.deferred_count, 1);
    }
}
