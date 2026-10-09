//! Timeout category helpers for ToolRegistry.

use super::{ToolRegistry, ToolTimeoutCategory};
use crate::config::constants::tools;
use crate::tools::mcp::legacy_mcp_tool_name;

impl ToolRegistry {
    pub async fn timeout_category_for(&self, name: &str) -> ToolTimeoutCategory {
        // Resolve alias through registration lookup
        let registration_opt = self.inventory.registration_for(name);
        if let Some(registration) = registration_opt {
            if registration.name().starts_with("mcp::") {
                return ToolTimeoutCategory::Mcp;
            }
            return if registration.uses_pty() {
                ToolTimeoutCategory::Pty
            } else {
                ToolTimeoutCategory::Default
            };
        }

        if let Some(stripped) = legacy_mcp_tool_name(name) {
            if self.has_mcp_tool(stripped).await {
                return ToolTimeoutCategory::Mcp;
            }
        } else if self.find_mcp_provider(name).await.is_some() || self.has_mcp_tool(name).await {
            return ToolTimeoutCategory::Mcp;
        }

        ToolTimeoutCategory::Default
    }

    pub async fn timeout_category_for_args(&self, name: &str, args: &serde_json::Value) -> ToolTimeoutCategory {
        if name == tools::WRITE_STDIN
            && matches!(
                crate::tools::command_args::write_stdin_dispatch(args),
                Ok(crate::tools::command_args::WriteStdinDispatch::Wait)
            )
        {
            return ToolTimeoutCategory::LongRunningCommand;
        }
        if name == tools::UNIFIED_EXEC && crate::tools::tool_intent::command_session_action_is(args, "wait") {
            return ToolTimeoutCategory::LongRunningCommand;
        }
        // A run that explicitly asks to yield only after a long window is a
        // long-running command: the executor enforces its own yield deadline
        // (`MAX_EXEC_YIELD_MS` clamp) and an outer default-ceiling timeout
        // would kill a healthy in-progress session instead of returning it.
        if crate::tools::tool_intent::is_command_session_tool(name)
            && crate::tools::tool_intent::command_session_action_is(args, "run")
            && args
                .get("yield_time_ms")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|yield_ms| yield_ms > DEFAULT_RUN_YIELD_MS)
        {
            return ToolTimeoutCategory::LongRunningCommand;
        }
        self.timeout_category_for(name).await
    }
}

/// Yield window above which an explicit `run` is treated as a long-running
/// command (no outer registry timeout; the executor enforces its own clamped
/// yield deadline). Chosen just above the common quick-command yield default
/// (10s) so ordinary calls keep their outer safety ceiling.
const DEFAULT_RUN_YIELD_MS: u64 = 10_000;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn long_yield_run_is_classified_long_running() {
        let temp = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

        // A run yielding beyond the quick-command window gets the
        // long-running category (no outer registry timeout).
        let long = registry
            .timeout_category_for_args(tools::EXEC_COMMAND, &json!({"cmd": "cargo build", "yield_time_ms": 20_000}))
            .await;
        assert_eq!(long, ToolTimeoutCategory::LongRunningCommand);

        // Boundary: exactly at the threshold keeps the ordinary category.
        let boundary = registry
            .timeout_category_for_args(tools::EXEC_COMMAND, &json!({"cmd": "cargo build", "yield_time_ms": 10_000}))
            .await;
        assert_ne!(boundary, ToolTimeoutCategory::LongRunningCommand);

        // The default yield (10s) keeps the ordinary outer ceiling.
        let default_yield = registry
            .timeout_category_for_args(tools::EXEC_COMMAND, &json!({"cmd": "echo hi"}))
            .await;
        assert_ne!(default_yield, ToolTimeoutCategory::LongRunningCommand);
    }

    #[tokio::test]
    async fn non_run_actions_never_get_long_running_from_yield() {
        let temp = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

        // poll with a large yield_time_ms must keep its ordinary category —
        // only `run` coordinates a fresh long-lived child.
        let poll = registry
            .timeout_category_for_args(
                tools::UNIFIED_EXEC,
                &json!({"action": "poll", "session_id": "run-1", "yield_time_ms": 30_000}),
            )
            .await;
        assert_ne!(poll, ToolTimeoutCategory::LongRunningCommand);

        // Non-exec tools with a yield_time_ms field are unaffected.
        let read = registry
            .timeout_category_for_args(tools::READ_FILE, &json!({"path": "x.txt", "yield_time_ms": 30_000}))
            .await;
        assert_ne!(read, ToolTimeoutCategory::LongRunningCommand);
    }

    #[tokio::test]
    async fn wait_actions_remain_long_running() {
        let temp = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;

        let wait = registry
            .timeout_category_for_args(tools::UNIFIED_EXEC, &json!({"action": "wait", "session_id": "run-1"}))
            .await;
        assert_eq!(wait, ToolTimeoutCategory::LongRunningCommand);
    }
}
