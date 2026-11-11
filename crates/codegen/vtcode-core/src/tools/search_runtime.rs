use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tracing::warn;

use crate::command_safety::shell_parser::prewarm_bash_parser;
use crate::utils::common::detect_workspace_languages;

/// The readiness of a search backend.
///
/// Search is implemented natively, so there is nothing to install and nothing
/// to probe: the backend is ready whenever the runtime is available. The
/// variant set is kept so the status surface stays uniform for callers that
/// report search health.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchToolReadiness {
    /// The backend is available.
    Ready,
    /// The backend is not available.
    Missing,
    /// The backend failed to initialise.
    Error,
}

impl SearchToolReadiness {
    #[must_use]
    pub fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Missing => "missing",
            Self::Error => "error",
        }
    }
}

/// The status of the search backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchToolBundleStatus {
    /// The native content-search backend.
    pub content_search: SearchToolReadiness,
    /// The native declaration-search backend.
    pub declaration_search: SearchToolReadiness,
}

impl SearchToolBundleStatus {
    #[must_use]
    pub fn all_ready(self) -> bool {
        self.content_search.is_ready() && self.declaration_search.is_ready()
    }

    #[must_use]
    pub fn all_unavailable(self) -> bool {
        !self.content_search.is_ready() && !self.declaration_search.is_ready()
    }

    #[must_use]
    pub fn has_errors(self) -> bool {
        matches!(self.content_search, SearchToolReadiness::Error)
            || matches!(self.declaration_search, SearchToolReadiness::Error)
    }

    #[must_use]
    pub fn header_summary(self) -> String {
        if self.all_ready() {
            return "Search: native".to_string();
        }
        format!(
            "Search: content {} \u{00b7} declarations {}",
            self.content_search.label(),
            self.declaration_search.label()
        )
    }
}

/// A snapshot of the search runtime for one workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchRuntimeSnapshot {
    /// The languages present in the workspace.
    pub(crate) workspace_languages: Vec<String>,
    /// The status of the search backends.
    pub(crate) search_tools: SearchToolBundleStatus,
    /// Whether the bash safety parser is available.
    pub(crate) bash_tree_sitter_ready: bool,
}

/// Bounded per-workspace search-runtime snapshot cache. Snapshots are cheap to
/// recompute relative to the value of caching (detect/prewarm only happens on
/// miss), and keying by workspace path means an unbounded map would leak one
/// entry per distinct workspace ever seen in a long-lived process. A small LRU
/// cap bounds memory while keeping the hot workspace cached.
const SEARCH_RUNTIME_CACHE_CAP: usize = 16;

static SEARCH_RUNTIME_CACHE: OnceLock<Mutex<lru::LruCache<PathBuf, SearchRuntimeSnapshot>>> = OnceLock::new();

pub(crate) fn snapshot_for_workspace(workspace_root: &Path) -> SearchRuntimeSnapshot {
    let workspace_root = workspace_root.to_path_buf();
    let cache = SEARCH_RUNTIME_CACHE.get_or_init(|| {
        let capacity = std::num::NonZeroUsize::new(SEARCH_RUNTIME_CACHE_CAP)
            .expect("SEARCH_RUNTIME_CACHE_CAP is a non-zero const");
        Mutex::new(lru::LruCache::new(capacity))
    });

    // Check cache first; recover from poison by clearing and returning None to recompute
    {
        let mut cache_guard = cache.lock().unwrap_or_else(|poisoned| {
            warn!("search runtime cache mutex poisoned; recovering");
            poisoned.into_inner()
        });
        if let Some(snapshot) = cache_guard.get(&workspace_root).cloned() {
            return snapshot;
        }
    }

    let workspace_languages = detect_workspace_languages(&workspace_root);
    // Both search backends are native, so they are ready whenever the runtime
    // is available. Only the bash safety parser can fail, and it reports its
    // own readiness separately.
    let search_tools = SearchToolBundleStatus {
        content_search: SearchToolReadiness::Ready,
        declaration_search: SearchToolReadiness::Ready,
    };
    let snapshot = SearchRuntimeSnapshot {
        workspace_languages,
        search_tools,
        bash_tree_sitter_ready: prewarm_bash_parser().is_ok(),
    };

    let mut guard = cache.lock().unwrap_or_else(|poisoned| {
        warn!("search runtime cache mutex poisoned; recovering");
        poisoned.into_inner()
    });
    guard.get_or_insert(workspace_root, || snapshot.clone()).clone()
}

pub fn dominant_workspace_language(workspace_root: &Path) -> Option<String> {
    snapshot_for_workspace(workspace_root).workspace_languages.into_iter().next()
}

pub fn search_tool_bundle_status(workspace_root: &Path) -> SearchToolBundleStatus {
    snapshot_for_workspace(workspace_root).search_tools
}
