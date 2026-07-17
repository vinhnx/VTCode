use super::ToolRegistry;

impl ToolRegistry {
    pub(super) fn prewarm_search_runtime(&self) {
        let snapshot =
            crate::tools::search_runtime::snapshot_for_workspace(self.inventory.workspace_root());

        tracing::trace!(
            ripgrep_ready = snapshot.ripgrep_ready,
            ast_grep_ready = snapshot.ast_grep_ready,
            search_tools = ?snapshot.search_tools,
            code_tree_sitter_languages = ?snapshot.code_tree_sitter_languages,
            bash_tree_sitter_ready = snapshot.bash_tree_sitter_ready,
            workspace_languages = ?snapshot.workspace_languages,
            "Pre-warmed search runtime"
        );
    }
}
