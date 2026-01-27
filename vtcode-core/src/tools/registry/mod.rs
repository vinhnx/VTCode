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
use std::collections::HashMap;
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
    mcp_tool_index: Arc<tokio::sync::RwLock<HashMap<String, Vec<String>>>>,
    mcp_tool_presence: Arc<tokio::sync::RwLock<HashMap<String, bool>>>,
    timeout_policy: Arc<std::sync::RwLock<ToolTimeoutPolicy>>,
    execution_history: ToolExecutionHistory,
    harness_context: HarnessContext,

    // Mutable runtime state wrapped for concurrent access
    resiliency: Arc<Mutex<ResiliencyContext>>,

    /// MP-3: Circuit breaker for MCP client failures
    mcp_circuit_breaker: Arc<circuit_breaker::McpCircuitBreaker>,
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
