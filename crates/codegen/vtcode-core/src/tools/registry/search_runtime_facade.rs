use super::ToolRegistry;

impl ToolRegistry {
    pub(super) fn prewarm_search_runtime(&self) {
        let snapshot = crate::tools::search_runtime::snapshot_for_workspace(self.inventory.workspace_root());

        tracing::trace!(
            search_tools = ?snapshot.search_tools,
            bash_tree_sitter_ready = snapshot.bash_tree_sitter_ready,
            workspace_languages = ?snapshot.workspace_languages,
            "Pre-warmed search runtime"
        );
    }
}
