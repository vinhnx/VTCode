//! Tool registry and function declarations

mod approval_recorder;
mod availability_facade;
mod builder;
mod builtins;
mod cache;
mod circuit_breaker;
mod commands_facade;
mod config_helpers;
mod declarations;
mod dual_output;
mod error;
mod execution_facade;
mod execution_history;
mod execution_kernel;
mod executors;
mod file_helpers;
mod harness;
mod harness_facade;
mod history_facade;
mod inventory;
mod inventory_facade;
mod justification;
mod justification_extractor;
pub mod labels;
mod maintenance;
mod mcp_facade;
mod mcp_helpers;
mod metrics_facade;
mod optimization_facade;
mod output_processing;
mod plan_mode_checks;
mod plan_mode_facade;
mod policy;
mod policy_facade;
mod progress_facade;
mod progressive_docs;
mod pty;
mod pty_facade;
mod registration;
mod registration_facade;
mod resiliency;
mod resiliency_facade;
mod risk_scorer;
mod shell_policy;
mod shell_policy_facade;
mod spooler_facade;
mod telemetry;
mod timeout;
mod timeout_category;
mod timeout_facade;
mod tool_executor_impl;
mod utils;

use std::borrow::Cow;

pub use approval_recorder::ApprovalRecorder;
pub use declarations::{
    build_function_declarations, build_function_declarations_cached,
    build_function_declarations_for_level, build_function_declarations_with_mode,
};
pub use error::{ToolErrorType, ToolExecutionError, classify_error};
pub use execution_history::{HarnessContextSnapshot, ToolExecutionHistory, ToolExecutionRecord};
pub use execution_kernel::ToolPreflightOutcome;
pub use harness::HarnessContext;
pub use justification::{ApprovalPattern, JustificationManager, ToolJustification};
pub use justification_extractor::JustificationExtractor;
pub use progressive_docs::{
    ToolDocumentationMode, ToolSignature, build_minimal_declarations,
    build_progressive_declarations, estimate_tokens, minimal_tool_signatures,
};
pub use pty::{PtySessionGuard, PtySessionManager};
pub use registration::{ToolExecutorFn, ToolHandler, ToolRegistration};
pub use resiliency::{ResiliencyContext, ToolFailureTracker};
pub use risk_scorer::{RiskLevel, ToolRiskContext, ToolRiskScorer, ToolSource, WorkspaceTrust};
pub use shell_policy::ShellPolicyChecker;
pub use telemetry::ToolTelemetryEvent;
pub use timeout::{
    AdaptiveTimeoutTuning, ToolLatencyStats, ToolTimeoutCategory, ToolTimeoutPolicy,
};

use inventory::ToolInventory;
use policy::ToolPolicyGateway;
use utils::normalize_tool_output;

use crate::tools::handlers::PlanModeState;
pub(super) use crate::tools::pty::PtyManager;
use crate::tools::result::ToolResult as SplitToolResult;
use parking_lot::Mutex; // Use parking_lot for better performance
use rustc_hash::FxHashMap;
use std::sync::Arc;

// Match agent runner throttle ceiling
const LOOP_THROTTLE_MAX_MS: u64 = 500;

use crate::mcp::McpClient;
use std::sync::RwLock;

/// Callback for tool progress and output streaming
pub type ToolProgressCallback = Arc<dyn Fn(&str, &str) + Send + Sync>;

use super::traits::Tool;
#[cfg(test)]
use crate::config::types::CapabilityLevel;

/// Default window size for loop detection.
const DEFAULT_LOOP_DETECT_WINDOW: usize = 5;

#[derive(Clone)]
pub struct ToolRegistry {
    inventory: ToolInventory,
    policy_gateway: Arc<tokio::sync::RwLock<ToolPolicyGateway>>,
    pty_sessions: PtySessionManager,
    mcp_client: Arc<std::sync::RwLock<Option<Arc<McpClient>>>>,
    mcp_tool_index: Arc<tokio::sync::RwLock<FxHashMap<String, Vec<String>>>>,
    mcp_reverse_index: Arc<tokio::sync::RwLock<FxHashMap<String, String>>>,
    timeout_policy: Arc<std::sync::RwLock<ToolTimeoutPolicy>>,
    execution_history: ToolExecutionHistory,
    harness_context: HarnessContext,

    // Mutable runtime state wrapped for concurrent access
    resiliency: Arc<Mutex<ResiliencyContext>>,

    /// MP-3: Circuit breaker for MCP client failures
    mcp_circuit_breaker: Arc<circuit_breaker::McpCircuitBreaker>,
    /// Shared per-tool circuit breaker state used by the runloop.
    shared_circuit_breaker:
        Arc<std::sync::RwLock<Option<Arc<crate::tools::circuit_breaker::CircuitBreaker>>>>,
    initialized: Arc<std::sync::atomic::AtomicBool>,
    // Security & Identity
    shell_policy: Arc<RwLock<ShellPolicyChecker>>,
    agent_type: Arc<std::sync::RwLock<Cow<'static, str>>>,
    // PTY Session Management
    active_pty_sessions: Arc<std::sync::RwLock<Option<Arc<std::sync::atomic::AtomicUsize>>>>,

    // Caching
    cached_available_tools: Arc<RwLock<Option<Vec<String>>>>,
    /// Callback for streaming tool output and progress
    progress_callback: Arc<std::sync::RwLock<Option<ToolProgressCallback>>>,
    // Performance Observability
    /// Total tool calls made in current session
    pub(crate) tool_call_counter: Arc<std::sync::atomic::AtomicU64>,
    /// Total PTY poll iterations (for monitoring CPU usage)
    pub(crate) pty_poll_counter: Arc<std::sync::atomic::AtomicU64>,

    // PERFORMANCE OPTIMIZATIONS - Actually integrated into the real registry
    /// Memory pool for reducing allocations in hot paths
    memory_pool: Arc<crate::core::memory_pool::MemoryPool>,
    /// Hot cache for frequently accessed tools (reduces HashMap lookups)
    hot_tool_cache: Arc<parking_lot::RwLock<lru::LruCache<String, Arc<dyn Tool>>>>,
    /// Optimization configuration
    optimization_config: vtcode_config::OptimizationConfig,

    /// Output spooler for dynamic context discovery (large outputs to files)
    output_spooler: Arc<super::output_spooler::ToolOutputSpooler>,

    /// Plan mode: read-only enforcement for planning sessions
    plan_read_only_mode: Arc<std::sync::atomic::AtomicBool>,

    /// Shared Plan Mode state (plan file tracking, active flag) for enter/exit tools
    plan_mode_state: PlanModeState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermissionDecision {
    Allow,
    Deny,
    Prompt,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TimeoutsConfig;
    use crate::constants::tools;
    use crate::tools::registry::mcp_helpers::normalize_mcp_tool_identifier;
    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::Value;
    use serde_json::json;
    use std::time::Duration;
    use tempfile::TempDir;

    const CUSTOM_TOOL_NAME: &str = "custom_test_tool";
    const REENTRANT_TOOL_NAME: &str = "reentrant_guard_test_tool";
    const MUTUAL_REENTRANT_TOOL_A: &str = "mutual_reentrant_tool_a";
    const MUTUAL_REENTRANT_TOOL_B: &str = "mutual_reentrant_tool_b";

    struct CustomEchoTool;

    #[async_trait]
    impl Tool for CustomEchoTool {
        async fn execute(&self, args: Value) -> Result<Value> {
            Ok(json!({
                "success": true,
                "args": args,
            }))
        }

        fn name(&self) -> &'static str {
            CUSTOM_TOOL_NAME
        }

        fn description(&self) -> &'static str {
            "Custom echo tool for testing"
        }
    }

    fn reentrant_tool_executor<'a>(
        registry: &'a ToolRegistry,
        args: Value,
    ) -> futures::future::BoxFuture<'a, Result<Value>> {
        Box::pin(async move { registry.execute_tool_ref(REENTRANT_TOOL_NAME, &args).await })
    }

    fn mutual_reentrant_tool_a_executor<'a>(
        registry: &'a ToolRegistry,
        args: Value,
    ) -> futures::future::BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            registry
                .execute_tool_ref(MUTUAL_REENTRANT_TOOL_B, &args)
                .await
        })
    }

    fn mutual_reentrant_tool_b_executor<'a>(
        registry: &'a ToolRegistry,
        args: Value,
    ) -> futures::future::BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            registry
                .execute_tool_ref(MUTUAL_REENTRANT_TOOL_A, &args)
                .await
        })
    }

    #[tokio::test]
    async fn registers_builtin_tools() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        let available = registry.available_tools().await;

        assert!(available.contains(&tools::READ_FILE.to_string()));
        assert!(available.contains(&tools::RUN_PTY_CMD.to_string()));
        Ok(())
    }

    #[tokio::test]
    async fn request_user_input_aliases_are_not_registered() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        assert!(registry.get_tool(tools::REQUEST_USER_INPUT).is_some());
        assert!(registry.get_tool(tools::ASK_QUESTIONS).is_none());
        assert!(registry.get_tool(tools::ASK_USER_QUESTION).is_none());

        Ok(())
    }

    #[tokio::test]
    async fn allows_registering_custom_tools() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry
            .register_tool(ToolRegistration::from_tool_instance(
                CUSTOM_TOOL_NAME,
                CapabilityLevel::CodeSearch,
                CustomEchoTool,
            ))
            .await?;

        registry.allow_all_tools().await.ok();

        let available = registry.available_tools().await;
        assert!(available.contains(&CUSTOM_TOOL_NAME.to_string()));

        let response = registry
            .execute_tool(CUSTOM_TOOL_NAME, json!({"input": "value"}))
            .await?;
        assert!(response["success"].as_bool().unwrap_or(false));
        Ok(())
    }

    #[tokio::test]
    async fn executes_prevalidated_tool_path() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry
            .register_tool(ToolRegistration::from_tool_instance(
                CUSTOM_TOOL_NAME,
                CapabilityLevel::CodeSearch,
                CustomEchoTool,
            ))
            .await?;
        registry.allow_all_tools().await?;

        let args = json!({"input": "value"});
        let response = registry
            .execute_tool_ref_prevalidated(CUSTOM_TOOL_NAME, &args)
            .await?;
        assert!(response["success"].as_bool().unwrap_or(false));

        Ok(())
    }

    #[tokio::test]
    async fn prevalidated_execution_enforces_plan_mode_guards() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry.allow_all_tools().await?;
        registry.enable_plan_mode();
        registry.plan_mode_state().enable();

        let blocked_path = temp_dir.path().join("blocked.txt");
        let args = json!({
            "path": blocked_path.to_string_lossy().to_string(),
            "content": "should-not-write"
        });

        let err = registry
            .execute_tool_ref_prevalidated(tools::WRITE_FILE, &args)
            .await
            .expect_err("plan mode should block prevalidated mutating tool call");
        assert!(err.to_string().contains("plan mode"));
        assert!(!blocked_path.exists());

        Ok(())
    }

    #[tokio::test]
    async fn prevalidated_execution_blocks_task_tracker_in_plan_mode() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry.allow_all_tools().await?;
        registry.enable_plan_mode();

        let args = json!({
            "action": "list"
        });

        let err = registry
            .execute_tool_ref_prevalidated(tools::TASK_TRACKER, &args)
            .await
            .expect_err("plan mode should block task_tracker on prevalidated path");
        assert!(err.to_string().contains("plan mode"));

        Ok(())
    }

    #[tokio::test]
    async fn preflight_normalizes_exec_code_alias_to_unified_exec() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let outcome = registry.preflight_validate_call(
            "exec_code",
            &json!({
                "command": "echo vtcode"
            }),
        )?;
        assert_eq!(outcome.normalized_tool_name, tools::UNIFIED_EXEC);

        Ok(())
    }

    #[tokio::test]
    async fn preflight_normalizes_humanized_exec_label_to_unified_exec() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let outcome = registry.preflight_validate_call(
            "Exec code",
            &json!({
                "command": "echo vtcode"
            }),
        )?;
        assert_eq!(outcome.normalized_tool_name, tools::UNIFIED_EXEC);

        Ok(())
    }

    #[tokio::test]
    async fn preflight_normalizes_repo_browser_aliases() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let read_outcome = registry.preflight_validate_call(
            "repo_browser.read_file",
            &json!({"path": "vtcode-core/src/lib.rs"}),
        )?;
        assert_eq!(read_outcome.normalized_tool_name, tools::UNIFIED_FILE);

        let list_outcome = registry.preflight_validate_call(
            "repo_browser.list_files",
            &json!({"path": "vtcode-core/src"}),
        )?;
        assert_eq!(list_outcome.normalized_tool_name, tools::UNIFIED_SEARCH);

        Ok(())
    }

    #[tokio::test]
    async fn preflight_normalizes_plan_mode_force_on_aliases() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let on_outcome = registry.preflight_validate_call("plan_on", &json!({}))?;
        assert_eq!(on_outcome.normalized_tool_name, tools::ENTER_PLAN_MODE);

        let slash_outcome = registry.preflight_validate_call("/plan", &json!({}))?;
        assert_eq!(slash_outcome.normalized_tool_name, tools::ENTER_PLAN_MODE);

        Ok(())
    }

    #[tokio::test]
    async fn preflight_normalizes_plan_mode_force_off_aliases() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let off_outcome = registry.preflight_validate_call("mode_edit", &json!({}))?;
        assert_eq!(off_outcome.normalized_tool_name, tools::EXIT_PLAN_MODE);

        let slash_outcome = registry.preflight_validate_call("/edit", &json!({}))?;
        assert_eq!(slash_outcome.normalized_tool_name, tools::EXIT_PLAN_MODE);

        Ok(())
    }

    #[tokio::test]
    async fn suggest_fallback_prefers_unified_exec_for_exec_code_alias() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let fallback = registry.suggest_fallback_tool("exec_code").await;
        assert_eq!(fallback.as_deref(), Some(tools::UNIFIED_EXEC));

        Ok(())
    }

    #[tokio::test]
    async fn suggest_fallback_prefers_unified_exec_for_humanized_exec_label() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        let fallback = registry.suggest_fallback_tool("Exec code").await;
        assert_eq!(fallback.as_deref(), Some(tools::UNIFIED_EXEC));

        Ok(())
    }

    #[tokio::test]
    async fn apply_patch_alias_executes_without_recursive_reentry() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry.allow_all_tools().await?;

        let patch =
            "*** Begin Patch\n*** Add File: patched_via_alias.txt\n+patched\n*** End Patch\n";
        let response = registry
            .execute_tool(tools::APPLY_PATCH, json!({ "patch": patch }))
            .await?;

        assert_eq!(response.get("success").and_then(Value::as_bool), Some(true));

        let file_contents = std::fs::read_to_string(temp_dir.path().join("patched_via_alias.txt"))?;
        assert_eq!(file_contents, "patched\n");

        Ok(())
    }

    #[tokio::test]
    async fn execution_history_records_harness_context() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry.set_harness_session("session-history");
        registry.set_harness_task(Some("task-history".to_owned()));

        registry
            .register_tool(ToolRegistration::from_tool_instance(
                CUSTOM_TOOL_NAME,
                CapabilityLevel::CodeSearch,
                CustomEchoTool,
            ))
            .await?;
        registry.allow_all_tools().await?;

        let args = json!({"input": "value"});
        let response = registry
            .execute_tool(CUSTOM_TOOL_NAME, args.clone())
            .await?;
        assert!(response["success"].as_bool().unwrap_or(false));

        let records = registry.get_recent_tool_records(1);
        let record = records.first().expect("execution record captured");
        assert_eq!(record.tool_name, CUSTOM_TOOL_NAME);
        assert_eq!(record.context.session_id, "session-history");
        assert_eq!(record.context.task_id.as_deref(), Some("task-history"));
        assert_eq!(record.args, args);
        assert!(record.success);

        Ok(())
    }

    #[tokio::test]
    async fn reentrancy_guard_blocks_recursive_tool_loops() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry
            .register_tool(ToolRegistration::new(
                REENTRANT_TOOL_NAME,
                CapabilityLevel::CodeSearch,
                false,
                reentrant_tool_executor,
            ))
            .await?;
        registry.allow_all_tools().await?;

        let response = registry
            .execute_tool(REENTRANT_TOOL_NAME, json!({"input": "loop"}))
            .await?;

        assert_eq!(
            response
                .get("reentrant_call_blocked")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            response
                .pointer("/error/error_type")
                .and_then(Value::as_str),
            Some("PolicyViolation")
        );
        assert!(
            response
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .contains("REENTRANCY GUARD")
        );

        Ok(())
    }

    #[tokio::test]
    async fn reentrancy_guard_blocks_cross_tool_cycles() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry
            .register_tool(ToolRegistration::new(
                MUTUAL_REENTRANT_TOOL_A,
                CapabilityLevel::CodeSearch,
                false,
                mutual_reentrant_tool_a_executor,
            ))
            .await?;
        registry
            .register_tool(ToolRegistration::new(
                MUTUAL_REENTRANT_TOOL_B,
                CapabilityLevel::CodeSearch,
                false,
                mutual_reentrant_tool_b_executor,
            ))
            .await?;
        registry.allow_all_tools().await?;

        let response = registry
            .execute_tool(MUTUAL_REENTRANT_TOOL_A, json!({"input": "cycle"}))
            .await?;

        assert_eq!(
            response
                .get("reentrant_call_blocked")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            response
                .pointer("/error/error_type")
                .and_then(Value::as_str),
            Some("PolicyViolation")
        );

        let stack_trace = response
            .get("stack_trace")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(stack_trace.contains(MUTUAL_REENTRANT_TOOL_A));
        assert!(stack_trace.contains(MUTUAL_REENTRANT_TOOL_B));

        Ok(())
    }

    #[tokio::test]
    async fn full_auto_allowlist_enforced() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        registry
            .enable_full_auto_mode(&vec![tools::READ_FILE.to_string()])
            .await;

        assert!(registry.preflight_tool_permission(tools::READ_FILE).await?);
        assert!(
            !registry
                .preflight_tool_permission(tools::RUN_PTY_CMD)
                .await?
        );

        Ok(())
    }

    #[test]
    fn normalizes_mcp_tool_identifiers() {
        assert_eq!(
            normalize_mcp_tool_identifier("sequential-thinking"),
            "sequentialthinking"
        );
        assert_eq!(
            normalize_mcp_tool_identifier("Context7.Lookup"),
            "context7lookup"
        );
        assert_eq!(normalize_mcp_tool_identifier("alpha_beta"), "alphabeta");
    }

    #[test]
    fn timeout_policy_derives_from_config() {
        let mut config = TimeoutsConfig::default();
        config.default_ceiling_seconds = 0;
        config.pty_ceiling_seconds = 600;
        config.mcp_ceiling_seconds = 90;
        config.warning_threshold_percent = 75;

        let policy = ToolTimeoutPolicy::from_config(&config);
        assert_eq!(policy.ceiling_for(ToolTimeoutCategory::Default), None);
        assert_eq!(
            policy.ceiling_for(ToolTimeoutCategory::Pty),
            Some(Duration::from_secs(600))
        );
        assert_eq!(
            policy.ceiling_for(ToolTimeoutCategory::Mcp),
            Some(Duration::from_secs(90))
        );
        assert!((policy.warning_fraction() - 0.75).abs() < f32::EPSILON);
    }
}
