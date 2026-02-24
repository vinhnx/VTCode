//! Plan mode and mutation detection helpers for ToolRegistry.

use serde_json::Value;

use crate::utils::path::normalize_ascii_identifier;

use super::ToolRegistry;

impl ToolRegistry {
    fn normalize_tool_key(name: &str) -> String {
        normalize_ascii_identifier(name)
    }

    /// Check if a tool is mutating (modifies files or environment).
    ///
    /// Returns true if the tool is mutating or unknown (conservative default).
    pub fn is_mutating_tool(&self, name: &str) -> bool {
        use crate::config::constants::tools as tool_names;
        use crate::tools::names::canonical_tool_name;

        let canonical = canonical_tool_name(name);
        let normalized = canonical.as_ref();
        let normalized_key = Self::normalize_tool_key(normalized);

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
            tool_names::ASK_QUESTIONS,
            tool_names::REQUEST_USER_INPUT,
            tool_names::LIST_SKILLS,
            tool_names::LOAD_SKILL,
            tool_names::LOAD_SKILL_RESOURCE,
            tool_names::SPAWN_SUBAGENT,
            "get_errors",
            "search_tools",
            "think",
        ];

        if read_only_tools
            .iter()
            .any(|tool| Self::normalize_tool_key(tool) == normalized_key)
        {
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

    /// Check if a tool is allowed to run in plan mode without switching modes.
    ///
    /// Returns true for non-mutating tools and plan-safe exceptions like
    /// writing to `.vtcode/plans/` or read-only unified tool actions.
    pub fn is_plan_mode_allowed(&self, tool_name: &str, args: &Value) -> bool {
        use crate::config::constants::tools;
        use crate::tools::names::canonical_tool_name;

        // Enforce plan/task tracker mode split across all execution paths.
        let canonical = canonical_tool_name(tool_name);
        match canonical.as_ref() {
            tools::TASK_TRACKER => return false,
            tools::PLAN_TASK_TRACKER => return true,
            _ => {}
        }

        let intent = crate::tools::tool_intent::classify_tool_intent(tool_name, args);
        if !intent.mutating {
            return true;
        }

        let allowed_plan_write = self.is_plan_file_operation(tool_name, args);
        let allowed_unified_readonly = intent.readonly_unified_action;

        allowed_plan_write || allowed_unified_readonly
    }

    /// Check whether a tool invocation is safe to retry.
    ///
    /// Retries are allowed for read-only operations and for unified tools when
    /// their specific action is read-only (`unified_file:read`, `unified_exec:poll|list`).
    pub fn is_retry_safe_call(&self, tool_name: &str, args: &Value) -> bool {
        crate::tools::tool_intent::classify_tool_intent(tool_name, args).retry_safe
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
    #[allow(dead_code)]
    pub(super) fn is_readonly_unified_action(&self, tool_name: &str, args: &Value) -> bool {
        crate::tools::tool_intent::classify_tool_intent(tool_name, args).readonly_unified_action
    }
}

#[cfg(test)]
mod tests {
    use super::ToolRegistry;
    use crate::config::constants::tools;
    use anyhow::Result;
    use serde_json::json;
    use tempfile::TempDir;

    #[tokio::test]
    async fn retry_safe_for_readonly_calls() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        assert!(registry.is_retry_safe_call(
            tools::UNIFIED_FILE,
            &json!({"action": "read", "path": "README.md"})
        ));
        assert!(registry.is_retry_safe_call(
            tools::UNIFIED_EXEC,
            &json!({"action": "poll", "session_id": 42})
        ));
        assert!(registry.is_retry_safe_call(tools::READ_FILE, &json!({"path": "README.md"})));

        Ok(())
    }

    #[tokio::test]
    async fn retry_unsafe_for_mutating_calls() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        assert!(!registry.is_retry_safe_call(
            tools::UNIFIED_FILE,
            &json!({"action": "write", "path": "foo.txt", "content": "x"})
        ));
        assert!(!registry.is_retry_safe_call(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "echo hi"})
        ));
        assert!(!registry.is_retry_safe_call(
            tools::WRITE_FILE,
            &json!({"path": "foo.txt", "content": "x"})
        ));

        Ok(())
    }

    #[tokio::test]
    async fn plan_mode_enforces_tracker_split() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry.enable_plan_mode();

        assert!(!registry.is_plan_mode_allowed(tools::TASK_TRACKER, &json!({"action": "list"})));
        assert!(
            registry.is_plan_mode_allowed(tools::PLAN_TASK_TRACKER, &json!({"action": "list"}))
        );

        Ok(())
    }
}
