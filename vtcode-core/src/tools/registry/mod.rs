//! Tool registry and function declarations

mod approval_recorder;
mod availability_facade;
mod commands_facade;
mod builtins;
mod cache;
mod circuit_breaker;
mod config_helpers;
mod declarations;
mod error;
mod execution_history;
mod executors;
mod file_helpers;
mod builder;
mod dual_output;
mod harness;
mod harness_facade;
mod history_facade;
mod inventory;
mod inventory_facade;
mod justification;
mod justification_extractor;
pub mod labels;
mod policy;
mod policy_facade;
mod plan_mode_checks;
mod plan_mode_facade;
mod progressive_docs;
mod pty;
mod pty_facade;
mod progress_facade;
mod registration;
mod registration_facade;
mod resiliency;
mod resiliency_facade;
mod risk_scorer;
mod shell_policy;
mod shell_policy_facade;
mod telemetry;
mod timeout;
mod timeout_facade;
mod timeout_category;
mod utils;
mod mcp_helpers;
mod mcp_facade;
mod optimization_facade;
mod metrics_facade;
mod maintenance;
mod output_processing;
mod spooler_facade;

use std::borrow::Cow;

pub use approval_recorder::ApprovalRecorder;
pub use declarations::{
    build_function_declarations, build_function_declarations_for_level,
    build_function_declarations_with_mode,
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

use crate::core::memory_pool::SizeRecommendation;
use crate::tool_policy::ToolExecutionDecision;
use crate::tools::handlers::PlanModeState;
pub(super) use crate::tools::pty::PtyManager;
use crate::tools::result::ToolResult as SplitToolResult;
use anyhow::{Result, anyhow};
use parking_lot::Mutex; // Use parking_lot for better performance
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, trace, warn};

// Match agent runner throttle ceiling
const LOOP_THROTTLE_MAX_MS: u64 = 500;

use crate::mcp::{McpClient, McpToolExecutor};
use crate::ui::search::fuzzy_match;
use std::sync::RwLock;
use std::time::SystemTime;

/// Callback for tool progress and output streaming
pub type ToolProgressCallback = Arc<dyn Fn(&str, &str) + Send + Sync>;

use super::traits::Tool;
use super::traits::ToolExecutor;
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

impl ToolRegistry {
    // Removed policy_manager_mut as it requires &mut self.
    // Use self.policy_gateway.write().await.policy_manager_mut() instead.

    // Removed policy_manager() as it cannot return a reference through a lock.
    // Use get_tool_policy() or other specific methods instead.

    pub async fn execute_tool(&self, name: &str, args: Value) -> Result<Value> {
        self.execute_tool_ref(name, &args).await
    }

    /// Reference-taking version of execute_tool to avoid cloning by callers
    /// that already have access to an existing `Value`.
    pub async fn execute_tool_ref(&self, name: &str, args: &Value) -> Result<Value> {
        // PERFORMANCE OPTIMIZATION: Use memory pool for string allocations if enabled
        let _pool_guard = if self.optimization_config.memory_pool.enabled {
            Some(self.memory_pool.get_string())
        } else {
            None
        };

        // PERFORMANCE OPTIMIZATION: Auto-tune memory pool based on usage patterns
        if self.optimization_config.memory_pool.enabled {
            let recommendation = self
                .memory_pool
                .auto_tune(&self.optimization_config.memory_pool);

            // Log recommendation if significant changes are suggested
            if !matches!(
                (
                    recommendation.string_size_recommendation,
                    recommendation.value_size_recommendation,
                    recommendation.vec_size_recommendation
                ),
                (
                    SizeRecommendation::Maintain,
                    SizeRecommendation::Maintain,
                    SizeRecommendation::Maintain
                )
            ) {
                tracing::debug!(
                    "Memory pool tuning recommendation: string={:?}, value={:?}, vec={:?}, allocations_avoided={}",
                    recommendation.string_size_recommendation,
                    recommendation.value_size_recommendation,
                    recommendation.vec_size_recommendation,
                    recommendation.total_allocations_avoided
                );
            }
        }

        // PERFORMANCE OPTIMIZATION: Check hot cache for tool lookup if optimizations enabled
        let cached_tool = if self
            .optimization_config
            .tool_registry
            .use_optimized_registry
        {
            let cache = self.hot_tool_cache.read();
            cache.peek(name).cloned()
        } else {
            None
        };

        // Look up the canonical tool name by trying to resolve the alias
        // The inventory's registration_for() handles alias resolution
        let (tool_name, tool_name_owned, display_name) =
            if let Some(registration) = self.inventory.registration_for(name) {
                let canonical = registration.name().to_string();
                let display = if canonical == name {
                    canonical.clone()
                } else {
                    format!("{} (alias for {})", name, canonical)
                };
                (canonical.clone(), canonical.clone(), display)
            } else {
                // If not found in registration, use the name as-is (for potential MCP tools or error handling)
                let tool_name_owned = name.to_string();
                let display_name = tool_name_owned.clone();
                (tool_name_owned.clone(), tool_name_owned, display_name)
            };

        // PERFORMANCE OPTIMIZATION: Update hot cache with resolved tool if optimizations enabled
        if let Some(tool_arc) = cached_tool.as_ref() {
            if self
                .optimization_config
                .tool_registry
                .use_optimized_registry
                && tool_name != name
            {
                // Cache the canonical name too for faster future lookups
                self.hot_tool_cache
                    .write()
                    .put(tool_name.clone(), tool_arc.clone());
            }
        }

        let requested_name = name.to_string();

        // Clone args once at the start for error recording paths (clone only here)
        let args_for_recording = args.clone();
        // Capture harness context snapshot for structured telemetry and history
        let context_snapshot = self.harness_context_snapshot();
        let context_payload = context_snapshot.to_json();

        // Validate arguments against schema if available
        if let Some(registration) = self.inventory.registration_for(&tool_name)
            && let Some(schema) = registration.parameter_schema()
            && let Err(errors) = jsonschema::validate(schema, args)
        {
            return Err(anyhow::anyhow!(
                "Invalid arguments for tool '{}': {}",
                tool_name,
                errors
            ));
        }

        // Plan mode enforcement: block mutating tools in read-only mode
        // Exceptions:
        // - Allow writes to .vtcode/plans/ so the agent can write its plan
        // - Allow unified tools when their action is read-only (unified_file: read; unified_exec: poll/list)
        if self.is_plan_mode() && self.is_mutating_tool(&tool_name) {
            let allowed_plan_write = self.is_plan_file_operation(&tool_name, &args);
            let allowed_unified_readonly = self.is_readonly_unified_action(&tool_name, &args);
            // Deny only if neither exception applies
            if !allowed_plan_write && !allowed_unified_readonly {
                let msg = format!(
                    "Tool '{}' execution failed: tool denied by plan mode\n\n\
                     ACTION REQUIRED: You are in Plan Mode (read-only). To start implementation:\n\
                     1. Call `exit_plan_mode` tool to show the user your plan for approval\n\
                     2. Wait for user to confirm (they will see the Implementation Blueprint)\n\
                     3. After approval, mutating tools will be enabled\n\n\
                     DO NOT retry this tool or use /plan off. The proper workflow is to call `exit_plan_mode`.",
                    display_name
                );

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned.clone(),
                        requested_name.clone(),
                        false,
                        None,
                        args_for_recording.clone(),
                        msg.clone(),
                        context_snapshot.clone(),
                        None,
                        None,
                        None,
                        None,
                        false, // Mark as policy block, not execution failure
                    ));

                // Return a structured error that indicates this is a policy block, not a failure
                // This helps distinguish between actual tool failures and intentional blocks
                return Err(anyhow::anyhow!(msg).context("tool denied by plan mode"));
            } else {
                debug!(
                    tool = %tool_name,
                    "Allowing read-only operation in Plan Mode"
                );
            }
        }

        let timeout_category = self.timeout_category_for(&tool_name).await;

        if let Some(backoff) = self.should_circuit_break(timeout_category) {
            warn!(
                tool = %tool_name,
                category = %timeout_category.label(),
                delay_ms = %backoff.as_millis(),
                "Circuit breaker active for tool category; backing off before execution"
            );
            tokio::time::sleep(backoff).await;
        }

        let execution_span = tracing::debug_span!(
            "tool_execution",
            tool = %tool_name,
            requested = %name,
            session_id = %context_snapshot.session_id,
            task_id = %context_snapshot.task_id.as_deref().unwrap_or("")
        );
        let _span_guard = execution_span.enter();

        debug!(
            tool = %tool_name,
            session_id = %context_snapshot.session_id,
            task_id = %context_snapshot.task_id.as_deref().unwrap_or(""),
            "Executing tool with harness context"
        );

        if tool_name != name {
            trace!(
                requested = %name,
                canonical = %tool_name,
                "Resolved tool alias to canonical name"
            );
        }

        let base_timeout_ms = self
            .timeout_policy
            .read()
            .unwrap()
            .ceiling_for(timeout_category)
            .map(|d| d.as_millis() as u64);
        let adaptive_timeout_ms = self
            .resiliency
            .lock()
            .adaptive_timeout_ceiling
            .get(&timeout_category)
            .filter(|d| d.as_millis() > 0)
            .map(|d| d.as_millis() as u64);
        let timeout_category_label = Some(timeout_category.label().to_string());

        if let Some(rate_limit) = self.execution_history.rate_limit_per_minute() {
            let calls_last_minute = self
                .execution_history
                .calls_in_window(Duration::from_secs(60));
            if calls_last_minute >= rate_limit {
                let _error = ToolExecutionError::new(
                    tool_name_owned.clone(),
                    ToolErrorType::PolicyViolation,
                    format!(
                        "Tool '{}' skipped: rate limit reached ({} calls/min)",
                        display_name, rate_limit
                    ),
                );

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned.clone(),
                        requested_name.clone(),
                        false,
                        None,
                        args_for_recording.clone(),
                        "Tool rate limit reached".to_string(),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        None,
                        false,
                    ));

                return Err(anyhow!(
                    "Tool '{}' rate limited ({} calls/min, {} recent)",
                    display_name,
                    rate_limit,
                    calls_last_minute
                )
                .context("tool rate limited"));
            }
        }

        // LOOP DETECTION: Check if we're calling the same tool repeatedly with identical params
        let loop_limit = self.execution_history.loop_limit_for(&tool_name);
        let (is_loop, repeat_count, _) = self.execution_history.detect_loop(&tool_name, args);
        if is_loop && repeat_count > 1 {
            let delay_ms = (25 * repeat_count as u64).min(LOOP_THROTTLE_MAX_MS);
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
        if loop_limit > 0 && is_loop {
            warn!(
                tool = %tool_name,
                repeats = repeat_count,
                "Loop detected: agent calling same tool with identical parameters {} times",
                repeat_count
            );
            if repeat_count >= loop_limit {
                let delay_ms = (75 * repeat_count as u64).min(500);
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }

                let error = ToolExecutionError::new(
                    tool_name_owned.clone(),
                    ToolErrorType::PolicyViolation,
                    format!(
                        "LOOP DETECTION: Tool '{}' has been called {} times with identical parameters and is now blocked.\n\n\
                        ACTION REQUIRED: DO NOT retry this tool call. The tool execution has been prevented to avoid infinite loops.\n\n\
                        If you need the result from this tool:\n\
                        1. Check if you already have the result from a previous successful call in your conversation history\n\
                        2. If not available, use a different approach or modify your request",
                        display_name, repeat_count
                    ),
                );
                let mut payload = error.to_json_value();
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("loop_detected".into(), json!(true));
                    obj.insert("repeat_count".into(), json!(repeat_count));
                    obj.insert("limit".into(), json!(loop_limit));
                    obj.insert("tool".into(), json!(display_name));
                }

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned,
                        requested_name.clone(),
                        false,
                        None,
                        args_for_recording,
                        "Tool call blocked due to repeated identical invocations".to_string(),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        None,
                        false,
                    ));

                return Ok(payload);
            }
        }

        if self.policy_gateway.read().await.has_full_auto_allowlist()
            && !self
                .policy_gateway
                .read()
                .await
                .is_allowed_in_full_auto(&tool_name)
        {
            let _error = ToolExecutionError::new(
                tool_name_owned.clone(),
                ToolErrorType::PolicyViolation,
                format!(
                    "Tool '{}' is not permitted while full-auto mode is active",
                    display_name
                ),
            );

            self.execution_history
                .add_record(ToolExecutionRecord::failure(
                    tool_name_owned.clone(),
                    requested_name.clone(),
                    false,
                    None,
                    args_for_recording.clone(),
                    "Tool execution denied by policy".to_string(),
                    context_snapshot.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                ));

            return Err(anyhow!(
                "Tool '{}' is not permitted while full-auto mode is active",
                display_name
            )
            .context("tool denied by full-auto allowlist"));
        }

        let skip_policy_prompt = self
            .policy_gateway
            .write()
            .await
            .take_preapproved(&tool_name);

        let decision = if skip_policy_prompt {
            ToolExecutionDecision::Allowed
        } else {
            // In TUI mode, permission should have been collected via ensure_tool_permission().
            // If not preapproved, check policy as fallback.
            self.policy_gateway
                .write()
                .await
                .should_execute_tool(&tool_name)
                .await?
        };

        if !decision.is_allowed() {
            let error_msg = match decision {
                ToolExecutionDecision::DeniedWithFeedback(feedback) => {
                    format!("Tool '{}' denied by user: {}", display_name, feedback)
                }
                _ => format!("Tool '{}' execution denied by policy", display_name),
            };

            let _error = ToolExecutionError::new(
                tool_name_owned.clone(),
                ToolErrorType::PolicyViolation,
                error_msg.clone(),
            );

            self.execution_history
                .add_record(ToolExecutionRecord::failure(
                    tool_name_owned.clone(),
                    requested_name.clone(),
                    false,
                    None,
                    args_for_recording.clone(),
                    error_msg.clone(),
                    context_snapshot.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                ));

            return Err(anyhow!("{}", error_msg).context("tool denied by policy"));
        }

        let args = match self
            .policy_gateway
            .read()
            .await
            .apply_policy_constraints(&tool_name, args)
        {
            Ok(processed_args) => processed_args,
            Err(err) => {
                let error = ToolExecutionError::with_original_error(
                    tool_name_owned.clone(),
                    ToolErrorType::InvalidParameters,
                    "Failed to apply policy constraints".to_string(),
                    err.to_string(),
                );

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned,
                        requested_name.clone(),
                        false,
                        None,
                        args_for_recording,
                        format!("Failed to apply policy constraints: {}", err),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        None,
                        false,
                    ));

                return Ok(error.to_json_value());
            }
        };

        // First, check if we need a PTY session by checking if the tool exists and needs PTY
        let mut needs_pty = false;
        let mut tool_exists = false;
        let mut is_mcp_tool = false;
        let mut mcp_provider: Option<String> = None;
        let mut mcp_tool_name: Option<String> = None;
        let mut mcp_lookup_error: Option<anyhow::Error> = None;

        // Check if it's a standard tool first
        if let Some(registration) = self.inventory.registration_for(&tool_name) {
            needs_pty = registration.uses_pty();
            tool_exists = true;
        }
        // If not a standard tool, check if it's an MCP tool
        let mcp_client_opt = { self.mcp_client.read().unwrap().clone() };
        if let Some(mcp_client) = mcp_client_opt {
            let mut resolved_mcp_name = if let Some(stripped) = name.strip_prefix("mcp_") {
                stripped.to_string()
            } else {
                tool_name_owned.clone()
            };

            if let Some(alias_target) = self.resolve_mcp_tool_alias(&resolved_mcp_name).await
                && alias_target != resolved_mcp_name
            {
                trace!(
                    requested = %resolved_mcp_name,
                    resolved = %alias_target,
                    "Resolved MCP tool alias"
                );
                resolved_mcp_name = alias_target;
            }

            match mcp_client.has_mcp_tool(&resolved_mcp_name).await {
                Ok(true) => {
                    needs_pty = true;
                    tool_exists = true;
                    is_mcp_tool = true;
                    mcp_tool_name = Some(resolved_mcp_name);
                    mcp_provider = self
                        .find_mcp_provider(mcp_tool_name.as_deref().unwrap())
                        .await;
                }
                Ok(false) => {
                    tool_exists = false;
                }
                Err(err) => {
                    warn!("Error checking MCP tool '{}': {}", resolved_mcp_name, err);
                    mcp_lookup_error = Some(err);
                }
            }
        }

        // If tool doesn't exist in either registry, return an error
        if !tool_exists {
            if let Some(err) = mcp_lookup_error {
                let error = ToolExecutionError::with_original_error(
                    tool_name_owned.clone(),
                    ToolErrorType::ExecutionError,
                    format!("Failed to resolve MCP tool '{}': {}", display_name, err),
                    err.to_string(),
                );

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned,
                        requested_name.clone(),
                        is_mcp_tool,
                        mcp_provider.clone(),
                        args_for_recording,
                        format!("Failed to resolve MCP tool '{}': {}", display_name, err),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        None,
                        false,
                    ));

                return Ok(error.to_json_value());
            }

            let error = ToolExecutionError::new(
                tool_name_owned.clone(),
                ToolErrorType::ToolNotFound,
                format!("Unknown tool: {}", display_name),
            );

            self.execution_history
                .add_record(ToolExecutionRecord::failure(
                    tool_name_owned,
                    requested_name.clone(),
                    is_mcp_tool,
                    mcp_provider.clone(),
                    args_for_recording,
                    format!("Unknown tool: {}", display_name),
                    context_snapshot.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                ));

            return Ok(error.to_json_value());
        }

        // MP-3: Circuit breaker check for MCP tools
        if is_mcp_tool && !self.mcp_circuit_breaker.allow_request() {
            let diag = self.mcp_circuit_breaker.diagnostics();
            let error = ToolExecutionError::new(
                tool_name_owned.clone(),
                ToolErrorType::ExecutionError,
                format!("MCP circuit breaker {:?}; skipping execution", diag.state),
            );
            let payload = json!({
                "error": error.to_json_value(),
                "circuit_breaker_state": format!("{:?}", diag.state),
                "consecutive_failures": diag.consecutive_failures,
                "note": "MCP provider circuit breaker open; execution skipped",
                "last_failed_at": diag.last_failure_time
                    .and_then(|ts| ts.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs()),
                "current_timeout_seconds": diag.current_timeout.as_secs(),
                "mcp_provider": mcp_provider,
            });
            warn!(
                tool = %tool_name_owned,
                payload = %payload,
                "Skipping MCP tool execution due to circuit breaker"
            );
            self.execution_history
                .add_record(ToolExecutionRecord::failure(
                    tool_name_owned,
                    requested_name,
                    is_mcp_tool,
                    mcp_provider.clone(),
                    args_for_recording,
                    format!("MCP circuit breaker {:?}; execution skipped", diag.state),
                    context_snapshot.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                ));
            return Ok(payload);
        }

        debug!(
            tool = %tool_name,
            requested = %name,
            is_mcp = is_mcp_tool,
            uses_pty = needs_pty,
            alias = %if tool_name == name { "" } else { name },
            mcp_provider = %mcp_provider.as_deref().unwrap_or(""),
            "Resolved tool route"
        );
        trace!(
            tool = %tool_name,
            requested = %name,
            mcp_provider = %mcp_provider.as_deref().unwrap_or(""),
            mcp_tool = %mcp_tool_name.as_deref().unwrap_or(""),
            context = %context_payload,
            "Tool execution context and routing finalized"
        );

        // Start PTY session if needed (using RAII guard for automatic cleanup)
        let _pty_guard = if needs_pty {
            match self.start_pty_session() {
                Ok(guard) => Some(guard),
                Err(err) => {
                    let error = ToolExecutionError::with_original_error(
                        tool_name_owned.clone(),
                        ToolErrorType::ExecutionError,
                        "Failed to start PTY session".to_string(),
                        err.to_string(),
                    );

                    self.execution_history
                        .add_record(ToolExecutionRecord::failure(
                            tool_name_owned,
                            requested_name.clone(),
                            is_mcp_tool,
                            mcp_provider.clone(),
                            args_for_recording,
                            "Failed to start PTY session".to_string(),
                            context_snapshot.clone(),
                            timeout_category_label.clone(),
                            base_timeout_ms,
                            adaptive_timeout_ms,
                            None,
                            false,
                        ));

                    return Ok(error.to_json_value());
                }
            }
        } else {
            None
        };

        // Execute the appropriate tool based on its type
        // The _pty_guard will automatically decrement the session count when dropped
        let execution_started_at = Instant::now();
        let effective_timeout = self.effective_timeout(timeout_category);
        let effective_timeout_ms = effective_timeout.map(|d| d.as_millis() as u64);
        let exec_future = async {
            if is_mcp_tool {
                let mcp_name =
                    mcp_tool_name.expect("mcp_tool_name should be set when is_mcp_tool is true");
                self.execute_mcp_tool(&mcp_name, args).await
            } else if let Some(registration) = self.inventory.registration_for(&tool_name) {
                // Log deprecation warning if tool is deprecated
                if registration.is_deprecated() {
                    if let Some(msg) = registration.deprecation_message() {
                        warn!("Tool '{}' is deprecated: {}", tool_name, msg);
                    } else {
                        warn!(
                            "Tool '{}' is deprecated and may be removed in a future version",
                            tool_name
                        );
                    }
                }

                let handler = registration.handler();
                match handler {
                    ToolHandler::RegistryFn(executor) => {
                        // PERFORMANCE OPTIMIZATION: Use memory pool for tool execution if enabled
                        if self.optimization_config.memory_pool.enabled {
                            let _execution_guard = self.memory_pool.get_value();
                            let _string_guard = self.memory_pool.get_string();
                            let _vec_guard = self.memory_pool.get_vec();
                            executor(self, args).await
                        } else {
                            executor(self, args).await
                        }
                    }
                    ToolHandler::TraitObject(tool) => {
                        // PERFORMANCE OPTIMIZATION: Use cached tool if available and optimizations enabled
                        if self
                            .optimization_config
                            .tool_registry
                            .use_optimized_registry
                        {
                            if let Some(cached_tool) = cached_tool.as_ref() {
                                // Use cached tool instance to avoid registry lookup overhead
                                cached_tool.execute(args).await
                            } else {
                                // Cache the tool for future use
                                self.hot_tool_cache
                                    .write()
                                    .put(tool_name.clone(), tool.clone());
                                tool.execute(args).await
                            }
                        } else {
                            tool.execute(args).await
                        }
                    }
                }
            } else {
                // This should theoretically never happen since we checked tool_exists above
                // Generate helpful error message with available tools
                let available_tools = self.inventory.available_tools();
                let mut tool_names = available_tools.to_vec();
                tool_names.extend(self.inventory.registered_aliases());
                tool_names.sort_unstable();
                let available_tool_list = tool_names.join(", ");

                let error_msg = if tool_name != requested_name {
                    // An alias was attempted but didn't resolve to an actual tool
                    format!(
                        "Tool '{}' (registered alias for '{}') not found in registry. \
                        Available tools: {}. \
                        Note: Tool aliases are defined during tool registration.",
                        requested_name, tool_name, available_tool_list
                    )
                } else {
                    // Find similar tools using fuzzy matching
                    let similar_tools: Vec<String> = tool_names
                        .iter()
                        .filter(|tool| fuzzy_match(&requested_name, tool))
                        .take(3) // Limit to 3 suggestions
                        .cloned()
                        .collect();

                    let suggestion = if !similar_tools.is_empty() {
                        format!(" Did you mean: {}?", similar_tools.join(", "))
                    } else {
                        String::new()
                    };

                    format!(
                        "Tool '{}' not found in registry. Available tools: {}.{}",
                        display_name, available_tool_list, suggestion
                    )
                };

                let error = ToolExecutionError::new(
                    tool_name_owned.clone(),
                    ToolErrorType::ToolNotFound,
                    error_msg.clone(),
                );

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned.clone(),
                        requested_name.clone(),
                        is_mcp_tool,
                        mcp_provider.clone(),
                        args_for_recording.clone(),
                        error_msg,
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        effective_timeout_ms,
                        false,
                    ));

                Ok(error.to_json_value())
            }
        };

        let result = if let Some(limit) = effective_timeout {
            debug!(
                tool = %tool_name_owned,
                category = %timeout_category.label(),
                timeout_ms = %limit.as_millis(),
                "Executing tool with effective timeout"
            );
            match tokio::time::timeout(limit, exec_future).await {
                Ok(res) => res,
                Err(_) => {
                    let timeout_ms = limit.as_millis() as u64;
                    let timeout_payload = json!({
                        "error": {
                            "message": format!("Tool execution timed out after {:?} (category: {})", limit, timeout_category.label()),
                            "timeout_category": timeout_category.label(),
                            "timeout_ms": timeout_ms,
                            "circuit_breaker": false,
                        }
                    });
                    self.execution_history
                        .add_record(ToolExecutionRecord::failure(
                            tool_name_owned,
                            requested_name,
                            is_mcp_tool,
                            mcp_provider,
                            args_for_recording,
                            "Tool execution timed out".to_string(),
                            context_snapshot.clone(),
                            timeout_category_label.clone(),
                            base_timeout_ms,
                            adaptive_timeout_ms,
                            Some(timeout_ms),
                            false,
                        ));
                    return Ok(timeout_payload);
                }
            }
        } else {
            exec_future.await
        };

        // PTY session will be automatically cleaned up when _pty_guard is dropped

        // Handle the execution result and record it

        match result {
            Ok(value) => {
                self.reset_tool_failure(timeout_category);
                let should_decay = {
                    let mut state = self.resiliency.lock();
                    let success_streak = state.adaptive_tuning.success_streak;
                    if let Some(counter) = state.success_trackers.get_mut(&timeout_category) {
                        *counter = counter.saturating_add(1);
                        let counter_val = *counter;
                        if counter_val >= success_streak {
                            *counter = 0;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };
                if should_decay {
                    self.decay_adaptive_timeout(timeout_category);
                }
                self.record_tool_latency(timeout_category, execution_started_at.elapsed());
                // Dynamic context discovery: spool large outputs to files
                let processed_value = self
                    .process_tool_output(&tool_name_owned, value, is_mcp_tool)
                    .await;
                let normalized_value = normalize_tool_output(processed_value);

                self.execution_history
                    .add_record(ToolExecutionRecord::success(
                        tool_name_owned,
                        requested_name,
                        is_mcp_tool,
                        mcp_provider,
                        args_for_recording,
                        normalized_value.clone(),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        effective_timeout_ms,
                        false,
                    ));

                Ok(normalized_value)
            }
            Err(err) => {
                let error_type = classify_error(&err);
                let error = ToolExecutionError::with_original_error(
                    tool_name_owned.clone(),
                    error_type,
                    format!("Tool execution failed: {}", err),
                    err.to_string(),
                );

                let tripped = self.record_tool_failure(timeout_category);
                if tripped {
                    warn!(
                        tool = %tool_name_owned,
                        category = %timeout_category.label(),
                        "Tool circuit breaker tripped after consecutive failures"
                    );
                }

                let mut payload = error.to_json_value();
                if let Some(obj) = payload.get_mut("error").and_then(|v| v.as_object_mut()) {
                    obj.insert(
                        "timeout_category".into(),
                        serde_json::Value::String(timeout_category.label().to_string()),
                    );
                    obj.insert(
                        "timeout_ms".into(),
                        serde_json::Value::from(effective_timeout_ms.unwrap_or(0)),
                    );
                    obj.insert("circuit_breaker".into(), serde_json::Value::Bool(tripped));
                }

                self.execution_history
                    .add_record(ToolExecutionRecord::failure(
                        tool_name_owned,
                        requested_name,
                        is_mcp_tool,
                        mcp_provider,
                        args_for_recording,
                        format!("Tool execution failed: {}", err),
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        effective_timeout_ms,
                        tripped,
                    ));

                Ok(payload)
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TimeoutsConfig;
    use crate::tools::registry::mcp_helpers::normalize_mcp_tool_identifier;
    use async_trait::async_trait;
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

#[async_trait::async_trait]
impl ToolExecutor for ToolRegistry {
    async fn execute_tool(&self, name: &str, args: Value) -> Result<Value> {
        self.execute_tool(name, args).await
    }

    async fn execute_tool_ref(&self, name: &str, args: &Value) -> Result<Value> {
        self.execute_tool_ref(name, args).await
    }

    async fn available_tools(&self) -> Vec<String> {
        self.available_tools().await
    }

    async fn has_tool(&self, name: &str) -> bool {
        // Optimized check: check inventory first, then cached MCP presence
        if self.inventory.has_tool(name) {
            return true;
        }

        let presence = self.mcp_tool_presence.read().await;
        if let Some(&present) = presence.get(name) {
            return present;
        }

        // Fallback to provider check if not in quick cache
        if self.find_mcp_provider(name).await.is_some() {
            return true;
        }

        false
    }
}
