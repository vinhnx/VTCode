//! Plan mode and mutation detection helpers for ToolRegistry.

use serde_json::Value;

use super::ToolRegistry;

impl ToolRegistry {
    /// Check if a tool is mutating (modifies files or environment).
    ///
    /// Returns true if the tool is mutating or unknown (conservative default).
    pub fn is_mutating_tool(&self, name: &str) -> bool {
        use crate::config::constants::tools as tool_names;
        use crate::tools::names::canonical_tool_name;

        let canonical = canonical_tool_name(name);
        let normalized = canonical.as_ref();

        // Check if it's a known read-only tool
        let read_only_tools = [
            tool_names::READ_FILE,
            tool_names::LIST_FILES,
            tool_names::GREP_FILE,
            tool_names::CODE_INTELLIGENCE,
            tool_names::UNIFIED_SEARCH,
            tool_names::AGENT_INFO,
            tool_names::ENTER_PLAN_MODE,
            tool_names::EXIT_PLAN_MODE,
            tool_names::ASK_USER_QUESTION,
            tool_names::REQUEST_USER_INPUT,
            "get_errors",
            "search_tools",
            "think",
        ];

        if read_only_tools.contains(&normalized) {
            return false;
        }

        // Check trait-based tools
        if let Some(reg) = self.inventory.get_registration(normalized) {
            if let super::ToolHandler::TraitObject(tool) = reg.handler() {
                return tool.is_mutating();
            }
        }

        // Conservative default: unknown tools are considered mutating
        true
    }

    /// Check if a tool operation is targeting the plans directory.
    /// In plan mode, writes to .vtcode/plans/ are allowed for the agent to write its plan.
    pub(super) fn is_plan_file_operation(&self, tool_name: &str, args: &Value) -> bool {
        use crate::config::constants::tools as tool_names;
        use crate::tools::names::canonical_tool_name;

        let canonical = canonical_tool_name(tool_name);
        let normalized = canonical.as_ref();

        // Only check file-writing tools
        let file_writing_tools = [
            tool_names::WRITE_FILE,
            tool_names::UNIFIED_FILE,
            tool_names::CREATE_FILE,
            tool_names::EDIT_FILE,
            tool_names::SEARCH_REPLACE,
        ];

        if !file_writing_tools.contains(&normalized) {
            return false;
        }

        // Extract file path from arguments
        let path_str = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("filePath"))
            .and_then(|v| v.as_str());

        let Some(path_str) = path_str else {
            return false;
        };
        let path = std::path::Path::new(path_str);

        // Check if the path is within .vtcode/plans/
        // Handle both absolute and relative paths
        let plans_suffix = std::path::Path::new(".vtcode").join("plans");

        // Check if path contains .vtcode/plans/
        if path_str.contains(".vtcode/plans/") || path_str.contains(".vtcode\\plans\\") {
            return true;
        }

        // Also check if it's a relative path under plans directory
        if path.starts_with(&plans_suffix) {
            return true;
        }

        // Check absolute path against workspace root
        let workspace = self.inventory.workspace_root();
        let plans_dir = workspace.join(".vtcode").join("plans");
        if path.starts_with(&plans_dir) {
            return true;
        }

        false
    }

    /// Check if a unified tool call represents a read-only action.
    /// Allows `unified_file` with action "read" and `unified_exec` with actions "poll" or "list".
    pub(super) fn is_readonly_unified_action(&self, tool_name: &str, args: &Value) -> bool {
        use crate::config::constants::tools as tool_names;
        use crate::tools::names::canonical_tool_name;

        let canonical = canonical_tool_name(tool_name);
        let normalized = canonical.as_ref();

        let action_opt = args.get("action").and_then(|v| v.as_str());
        match (normalized, action_opt) {
            (tool_names::UNIFIED_FILE, Some(action)) => action.eq_ignore_ascii_case("read"),
            (tool_names::UNIFIED_EXEC, Some(action)) => {
                matches!(action.to_ascii_lowercase().as_str(), "poll" | "list")
            }
            (tool_names::UNIFIED_SEARCH, Some(action)) => action.eq_ignore_ascii_case("list"),
            _ => false,
        }
    }
}
