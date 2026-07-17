//! Planning workflow and mutation detection helpers for ToolRegistry.

use serde_json::Value;

use super::ToolRegistry;
use crate::tools::tool_intent::{ToolIntent, classify_tool_intent};

/// Tools that can write a plan file. Hoisted to a `const` so
/// [`ToolRegistry::is_plan_file_operation`] does not allocate a fresh array on
/// every tool call on the agent runloop's hot path.
const FILE_WRITING_TOOLS: &[&str] = &[
    crate::config::constants::tools::WRITE_FILE,
    crate::config::constants::tools::UNIFIED_FILE,
    crate::config::constants::tools::CREATE_FILE,
    crate::config::constants::tools::EDIT_FILE,
    crate::config::constants::tools::SEARCH_REPLACE,
];

impl ToolRegistry {
    /// Check if a tool is mutating (modifies files or environment).
    ///
    /// Returns true if the tool is mutating or unknown (conservative default).
    pub fn is_mutating_tool(&self, name: &str) -> bool {
        let resolved_name = self
            .resolve_public_tool_name_sync(name)
            .ok()
            .unwrap_or_else(|| name.to_string());

        if let Some(reg) = self.inventory.get_registration(&resolved_name) {
            if let Some(behavior) = reg.metadata().behavior() {
                return !matches!(
                    behavior.mutation_model,
                    crate::tools::tool_intent::ToolMutationModel::ReadOnly
                );
            }

            if let super::ToolHandler::TraitObject(tool) = reg.handler() {
                return tool.is_mutating();
            }
        }

        if let Some(reg) = self.inventory.get_registration(name)
            && let super::ToolHandler::TraitObject(tool) = reg.handler()
        {
            return tool.is_mutating();
        }

        // Conservative default: unknown tools are considered mutating
        true
    }

    /// Check if a tool is allowed to run in planning workflow without switching modes.
    ///
    /// Returns true for non-mutating tools and plan-safe exceptions like
    /// writing to active plan storage (`/tmp/vtcode-plans/` by default) or read-only unified tool actions.
    pub fn is_planning_active_allowed(&self, tool_name: &str, args: &Value) -> bool {
        let intent = classify_tool_intent(tool_name, args);
        self.is_planning_active_allowed_with_intent(tool_name, args, &intent)
    }

    /// Variant of [`Self::is_planning_active_allowed`] that reuses an already
    /// computed [`ToolIntent`], avoiding a redundant `classify_tool_intent` on
    /// the per-tool-call hot path. Callers typically classify the same
    /// `(tool_name, args)` once for `readonly_classification` / parallel-safety
    /// and should pass that intent in rather than recomputing it.
    pub(super) fn is_planning_active_allowed_with_intent(
        &self,
        tool_name: &str,
        args: &Value,
        intent: &ToolIntent,
    ) -> bool {
        use crate::config::constants::tools;
        use crate::tools::names::canonical_tool_name;

        // Keep adaptive task tracker available in all modes.
        let canonical = canonical_tool_name(tool_name);
        if canonical == tools::TASK_TRACKER {
            return true;
        }

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
    /// their specific action is read-only (`file_operation:read`, `command_session:poll|list`).
    pub fn is_retry_safe_call(&self, tool_name: &str, args: &Value) -> bool {
        classify_tool_intent(tool_name, args).retry_safe
    }

    /// Check if a tool operation is targeting the plans directory.
    /// In planning workflow, writes to active plan storage are allowed for the agent to write its plan.
    pub(super) fn is_plan_file_operation(&self, tool_name: &str, args: &Value) -> bool {
        use crate::tools::names::canonical_tool_name;

        let canonical = canonical_tool_name(tool_name);
        let normalized = canonical;

        // Only check file-writing tools
        if !FILE_WRITING_TOOLS.contains(&normalized) {
            return false;
        }

        // Extract file path from arguments
        let path_str = args
            .get("path")
            .or_else(|| args.get("file_path"))
            .or_else(|| args.get("filePath"))
            .or_else(|| args.get("destination"))
            .or_else(|| args.get("destination_path"))
            .and_then(|v| v.as_str());

        let Some(path_str) = path_str else {
            return false;
        };
        let path = std::path::Path::new(path_str);

        // Legacy workspace-scoped plan path (.vtcode/plans/) compatibility.
        let plans_suffix = std::path::Path::new(".vtcode").join("plans");

        // Check if path contains .vtcode/plans/
        if path_str.contains(".vtcode/plans/") || path_str.contains(".vtcode\\plans\\") {
            return true;
        }

        // Also check if it's a relative path under plans directory
        if path.starts_with(&plans_suffix) {
            return true;
        }

        // Check absolute legacy path against workspace root
        let workspace = self.inventory.workspace_root();
        let plans_dir = workspace.join(".vtcode").join("plans");
        if path.starts_with(&plans_dir) {
            return true;
        }

        // Default ephemeral plan storage path under /tmp.
        if path_str.contains("/tmp/vtcode-plans/") || path_str.contains("\\tmp\\vtcode-plans\\") {
            return true;
        }
        let tmp_plans = std::env::temp_dir().join("vtcode-plans");
        if path.starts_with(&tmp_plans) {
            return true;
        }

        false
    }

    /// Check if a unified tool call represents a read-only action.
    /// Allows `file_operation` with action "read" and `command_session` with read-only actions
    /// (poll/list/inspect/continue without input) plus allowlisted run commands or `--dry-run`.
    #[expect(dead_code)]
    pub(super) fn is_readonly_unified_action(&self, tool_name: &str, args: &Value) -> bool {
        classify_tool_intent(tool_name, args).readonly_unified_action
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
        assert!(
            registry.is_retry_safe_call(
                tools::UNIFIED_EXEC,
                &json!({"action": "poll", "session_id": 42})
            )
        );
        assert!(registry.is_retry_safe_call(
            tools::UNIFIED_EXEC,
            &json!({"action": "inspect", "spool_path": ".vtcode/context/tool_outputs/run-1.txt"})
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
        assert!(
            !registry
                .is_retry_safe_call(tools::WRITE_FILE, &json!({"path": "foo.txt", "content": "x"}))
        );

        Ok(())
    }

    #[tokio::test]
    async fn planning_workflow_allows_adaptive_task_tracker() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry.enable_planning();

        assert!(
            registry.is_planning_active_allowed(tools::TASK_TRACKER, &json!({"action": "list"}))
        );
        Ok(())
    }

    #[tokio::test]
    async fn planning_workflow_allows_readonly_command_session_runs() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry.enable_planning();

        assert!(registry.is_planning_active_allowed(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "ls -la"})
        ));
        assert!(registry.is_planning_active_allowed(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "npm install --dry-run"})
        ));
        assert!(!registry.is_planning_active_allowed(
            tools::UNIFIED_EXEC,
            &json!({"action": "run", "command": "echo hi"})
        ));

        Ok(())
    }
}
