pub(super) use super::*;
pub(super) use crate::config::TimeoutsConfig;
pub(super) use crate::config::ToolDocumentationMode as ConfigToolDocumentationMode;
pub(super) use crate::config::ToolsConfig;
pub(super) use crate::constants::tools;
pub(super) use crate::tool_policy::{ToolConstraints, ToolPolicy, ToolPolicyConfig};
pub(super) use crate::tools::handlers::{
    PlanningWorkflowState, SessionSurface, SessionToolsConfig, TaskTrackerTool, ToolModelCapabilities, ToolProfile,
};
pub(super) use crate::tools::registry::mcp_helpers::normalize_mcp_tool_identifier;
pub(super) use crate::tools::traits::Tool;
pub(super) use anyhow::Result;
pub(super) use async_trait::async_trait;
pub(super) use futures::future::BoxFuture;
pub(super) use serde_json::Value;
pub(super) use serde_json::json;
pub(super) use std::fs;
pub(super) use std::sync::Arc;
pub(super) use std::sync::atomic::{AtomicUsize, Ordering};
pub(super) use std::time::Duration;
pub(super) use tempfile::TempDir;
pub(super) use vtcode_commons::canonicalize;

const CUSTOM_TOOL_NAME: &str = "custom_test_tool";

const SLOW_TIMEOUT_TOOL_NAME: &str = "slow_timeout_test_tool";

const REENTRANT_TOOL_NAME: &str = "reentrant_guard_test_tool";

const MUTUAL_REENTRANT_TOOL_A: &str = "mutual_reentrant_tool_a";

const MUTUAL_REENTRANT_TOOL_B: &str = "mutual_reentrant_tool_b";

const REPLACE_DISPATCH_TOOL_NAME: &str = "replace_dispatch_test_tool";

struct CustomEchoTool;
struct SlowTimeoutTool;

#[async_trait]
impl Tool for CustomEchoTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        Ok(json!({
            "success": true,
            "args": args,
        }))
    }

    fn name(&self) -> &str {
        CUSTOM_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Custom echo tool for testing"
    }
}

#[async_trait]
impl Tool for SlowTimeoutTool {
    async fn execute(&self, _args: Value) -> Result<Value> {
        tokio::time::sleep(Duration::from_millis(1_100)).await;
        Ok(json!({
            "ok": true,
        }))
    }

    fn name(&self) -> &str {
        SLOW_TIMEOUT_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Tool that intentionally exceeds low timeout ceilings"
    }
}

fn reentrant_tool_executor<'a>(registry: &'a ToolRegistry, args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move { registry.execute_tool_ref(REENTRANT_TOOL_NAME, &args).await })
}

fn mutual_reentrant_tool_a_executor<'a>(registry: &'a ToolRegistry, args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move { registry.execute_tool_ref(MUTUAL_REENTRANT_TOOL_B, &args).await })
}

fn mutual_reentrant_tool_b_executor<'a>(registry: &'a ToolRegistry, args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move { registry.execute_tool_ref(MUTUAL_REENTRANT_TOOL_A, &args).await })
}

fn replacement_first_executor<'a>(_registry: &'a ToolRegistry, _args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move { Ok(json!({"version": 1})) })
}

fn replacement_second_executor<'a>(_registry: &'a ToolRegistry, _args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async move { Ok(json!({"version": 2})) })
}

fn catalogue_race_executor<'a>(_registry: &'a ToolRegistry, _args: Value) -> BoxFuture<'a, Result<Value>> {
    Box::pin(async { Ok(json!({"status": "ok"})) })
}

fn advanced_session_tools_config() -> SessionToolsConfig {
    SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ConfigToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode)
}

async fn policy_catalogue_test_hooks(registry: &ToolRegistry) -> Arc<policy::PolicyCatalogueTestHooks> {
    registry.policy_gateway.full_auto_catalogue_test_hooks()
}

async fn wait_for_catalogue_pause(pause: &policy::PolicyCatalogueTestPause) {
    tokio::time::timeout(Duration::from_secs(5), pause.wait_until_reached())
        .await
        .expect("catalogue operation reached the controlled pause");
}

async fn assert_catalogue_task_remains_pending<T>(task: &mut tokio::task::JoinHandle<T>, task_name: &str) {
    tokio::select! {
        outcome = &mut *task => match outcome {
            Ok(_) => panic!("{task_name} completed while catalogue refresh was paused"),
            Err(error) => panic!(
                "{task_name} failed while catalogue refresh was paused: {error}"
            ),
        },
        () = tokio::time::sleep(Duration::from_millis(50)) => {}
    }
}

fn delayed_exec_args(tty: bool, yield_time_ms: u64) -> Value {
    json!({
        "cmd": "printf first && sleep 0.2 && printf second",
        "tty": tty,
        "yield_time_ms": yield_time_ms,
    })
}

fn long_running_exec_args(tty: bool, yield_time_ms: u64) -> Value {
    json!({
        "cmd": "sleep 0.4 && printf second && sleep 0.4 && printf third && sleep 0.4 && printf done",
        "tty": tty,
        "yield_time_ms": yield_time_ms,
    })
}

mod aliases_mcp;
mod decisions_verifier;
mod exec_modes;
mod file_editing;
mod guards_misc;
mod planning_workflow;
mod preflight_aliases;
mod registration;
mod session_controls;
mod session_reuse;
