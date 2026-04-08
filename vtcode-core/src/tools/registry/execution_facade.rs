//! Tool execution entrypoints for ToolRegistry.

use anyhow::{Context, Result, anyhow};
use hashbrown::HashMap;
use once_cell::sync::Lazy;
use serde_json::{Value, json};
use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};
use tokio::task::Id as TokioTaskId;
use tracing::{trace, warn};
use vtcode_commons::ErrorCategory;

use crate::config::constants::tools;
use crate::core::agent::harness_kernel::PreparedToolCall;
use crate::core::memory_pool::SizeRecommendation;
use crate::mcp::McpToolExecutor;
use crate::tool_policy::ToolExecutionDecision;
use crate::tools::error_messages::agent_execution;
use crate::tools::invocation::ToolInvocationId;
use crate::tools::mcp::{legacy_mcp_tool_name, parse_canonical_mcp_tool_name};
use crate::tools::safety_gateway::{
    SafetyContext, SafetyDecision, SafetyError as GatewaySafetyError,
};
use crate::ui::search::fuzzy_match;

use super::LOOP_THROTTLE_MAX_MS;
use super::assembly::public_tool_name_candidates;
use super::execution_kernel;
use super::normalize_tool_output;
use super::{
    ExecSettlementMode, ExecutionPolicySnapshot, ToolErrorType, ToolExecutionError,
    ToolExecutionOutcome, ToolExecutionRecord, ToolExecutionRequest, ToolHandler, ToolRegistry,
};

const REENTRANCY_STACK_DEPTH_LIMIT: usize = 64;
// Tools should never recursively re-enter themselves in a single task.
// Keeping this at 1 blocks the first re-entry (A -> ... -> A) to fail fast
// on alias/self-recursion bugs with minimal extra work.
const REENTRANCY_PER_TOOL_LIMIT: usize = 1;

static TOOL_REENTRANCY_STACKS: Lazy<Mutex<HashMap<TokioTaskId, Vec<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
thread_local! {
    static THREAD_REENTRANCY_STACK: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn lock_reentrancy_stacks() -> std::sync::MutexGuard<'static, HashMap<TokioTaskId, Vec<String>>> {
    TOOL_REENTRANCY_STACKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug)]
struct ReentrancyViolation {
    stack_depth: usize,
    tool_reentry_count: usize,
    stack_trace: String,
}

enum ReentrancyContext {
    Task(TokioTaskId),
    Thread,
}

struct ToolReentrancyGuard {
    context: Option<ReentrancyContext>,
}

impl ToolReentrancyGuard {
    fn enter(tool_name: &str) -> std::result::Result<Self, ReentrancyViolation> {
        if let Some(task_id) = tokio::task::try_id() {
            let mut stacks = lock_reentrancy_stacks();
            let stack = stacks.entry(task_id).or_default();
            let stack_depth = stack.len();
            let tool_reentry_count = stack
                .iter()
                .filter(|active_tool| active_tool.as_str() == tool_name)
                .count();

            if stack_depth >= REENTRANCY_STACK_DEPTH_LIMIT
                || tool_reentry_count >= REENTRANCY_PER_TOOL_LIMIT
            {
                let stack_trace = if stack.is_empty() {
                    "<empty>".to_string()
                } else {
                    stack.join(" -> ")
                };
                return Err(ReentrancyViolation {
                    stack_depth,
                    tool_reentry_count,
                    stack_trace,
                });
            }

            stack.push(tool_name.to_string());
            return Ok(Self {
                context: Some(ReentrancyContext::Task(task_id)),
            });
        }

        let violation = THREAD_REENTRANCY_STACK.with(|stack_cell| {
            let mut stack = stack_cell.borrow_mut();
            let stack_depth = stack.len();
            let tool_reentry_count = stack
                .iter()
                .filter(|active_tool| active_tool.as_str() == tool_name)
                .count();

            if stack_depth >= REENTRANCY_STACK_DEPTH_LIMIT
                || tool_reentry_count >= REENTRANCY_PER_TOOL_LIMIT
            {
                let stack_trace = if stack.is_empty() {
                    "<empty>".to_string()
                } else {
                    stack.join(" -> ")
                };
                Some(ReentrancyViolation {
                    stack_depth,
                    tool_reentry_count,
                    stack_trace,
                })
            } else {
                stack.push(tool_name.to_string());
                None
            }
        });

        if let Some(violation) = violation {
            return Err(violation);
        }

        Ok(Self {
            context: Some(ReentrancyContext::Thread),
        })
    }
}

impl Drop for ToolReentrancyGuard {
    fn drop(&mut self) {
        let Some(context) = self.context.take() else {
            return;
        };

        match context {
            ReentrancyContext::Task(task_id) => {
                let mut stacks = lock_reentrancy_stacks();
                let should_remove = if let Some(stack) = stacks.get_mut(&task_id) {
                    let _ = stack.pop();
                    stack.is_empty()
                } else {
                    false
                };
                if should_remove {
                    stacks.remove(&task_id);
                }
            }
            ReentrancyContext::Thread => {
                THREAD_REENTRANCY_STACK.with(|stack_cell| {
                    let _ = stack_cell.borrow_mut().pop();
                });
            }
        }
    }
}

impl ToolRegistry {
    fn annotate_timeout_error_payload(
        payload: &mut Value,
        timeout_category: &str,
        timeout_ms: u64,
        circuit_breaker: bool,
    ) {
        if let Some(obj) = payload
            .get_mut("error")
            .and_then(|value| value.as_object_mut())
        {
            obj.insert(
                "timeout_category".into(),
                Value::String(timeout_category.to_string()),
            );
            obj.insert("timeout_ms".into(), Value::from(timeout_ms));
            obj.insert("circuit_breaker".into(), Value::Bool(circuit_breaker));
        }
    }

    fn safety_denial_error(
        &self,
        tool_name: &str,
        reason: &str,
        violation: Option<GatewaySafetyError>,
        retry_after: Option<Duration>,
    ) -> ToolExecutionError {
        let mut error = ToolExecutionError::policy_violation(
            tool_name.to_string(),
            format!("Safety gateway denied execution: {reason}"),
        );

        match violation {
            Some(GatewaySafetyError::RateLimitExceeded { .. }) => {
                error.error_type = ToolErrorType::NetworkError;
                error.category = ErrorCategory::RateLimit;
                error.retryable = true;
                error.is_recoverable = true;
            }
            Some(GatewaySafetyError::TurnLimitReached { .. })
            | Some(GatewaySafetyError::SessionLimitReached { .. }) => {
                error.error_type = ToolErrorType::ExecutionError;
                error.category = ErrorCategory::ResourceExhausted;
                error.retryable = false;
                error.is_recoverable = false;
            }
            Some(GatewaySafetyError::PlanModeViolation(_)) => {
                error.error_type = ToolErrorType::PolicyViolation;
                error.category = ErrorCategory::PlanModeViolation;
                error.retryable = false;
                error.is_recoverable = true;
            }
            Some(GatewaySafetyError::CommandPolicyDenied(_))
            | Some(GatewaySafetyError::DotfileProtectionViolation(_))
            | None => {}
        }

        if let Some(delay) = retry_after {
            error.retry_after_ms = Some(delay.as_millis() as u64);
        }
        error.circuit_breaker_impact = error.category.should_trip_circuit_breaker();
        error.recovery_suggestions = error.category.recovery_suggestions();
        error
    }

    pub fn safety_gateway(&self) -> std::sync::Arc<crate::tools::safety_gateway::SafetyGateway> {
        std::sync::Arc::clone(&self.safety_gateway)
    }

    async fn check_safety_for_request(
        &self,
        tool_name: &str,
        args: &Value,
        invocation_id: Option<String>,
    ) -> Option<ToolExecutionError> {
        let context = SafetyContext::new(self.harness_context_snapshot().session_id);
        let invocation_id = invocation_id
            .and_then(|id| ToolInvocationId::parse(&id).ok())
            .unwrap_or_default();
        let safety_result = self
            .safety_gateway
            .check_and_record_with_id(&context, tool_name, args, Some(invocation_id))
            .await;

        match safety_result.decision {
            SafetyDecision::Allow | SafetyDecision::NeedsApproval(_) => None,
            SafetyDecision::Deny(reason) => Some(
                self.safety_denial_error(
                    tool_name,
                    &reason,
                    safety_result.violation,
                    safety_result.retry_after,
                )
                .with_surface("tool_registry"),
            ),
        }
    }

    pub async fn execute_public_tool_request(
        &self,
        request: ToolExecutionRequest,
    ) -> ToolExecutionOutcome {
        self.execute_tool_request_internal(request).await
    }

    pub async fn execute_prepared_public_tool_request(
        &self,
        prepared: &PreparedToolCall,
        policy: ExecutionPolicySnapshot,
    ) -> ToolExecutionOutcome {
        let request = ToolExecutionRequest::new(
            prepared.canonical_name.clone(),
            prepared.effective_args.clone(),
        )
        .with_policy(
            policy
                .with_prevalidated(prepared.already_preflighted)
                .with_safety_prevalidated(false),
        );
        self.execute_tool_request_internal(request).await
    }

    async fn execute_tool_request_internal(
        &self,
        request: ToolExecutionRequest,
    ) -> ToolExecutionOutcome {
        let policy = request.policy.clone();
        let mut retry_policy = crate::retry::RetryPolicy::from_retries(
            policy.max_retries as u32,
            policy.retry_base_delay,
            policy.retry_max_delay,
            policy.retry_multiplier,
        );
        retry_policy.jitter = policy.retry_jitter.clamp(0.0, 1.0);

        let max_attempts = retry_policy.max_attempts.max(1);
        let mut attempt_index: u32 = 0;
        let mut last_error: Option<ToolExecutionError> = None;

        while attempt_index < max_attempts {
            if !policy.safety_prevalidated
                && let Some(safety_error) = self
                    .check_safety_for_request(
                        request.tool_name.as_str(),
                        &request.args,
                        policy.invocation_id.clone(),
                    )
                    .await
            {
                let structured = retry_policy.apply_to_tool_execution_error(
                    safety_error
                        .with_tool_call_context(request.tool_name.as_str(), &request.args)
                        .with_attempt(attempt_index + 1)
                        .with_surface("tool_registry"),
                    attempt_index,
                    Some(request.tool_name.as_str()),
                );
                let retry_delay = structured
                    .retry_after()
                    .or_else(|| structured.retry_delay());
                if structured.retryable
                    && attempt_index + 1 < max_attempts
                    && let Some(delay) = retry_delay
                {
                    last_error = Some(structured);
                    tokio::time::sleep(delay).await;
                    attempt_index = attempt_index.saturating_add(1);
                    continue;
                }

                return ToolExecutionOutcome::failure(
                    request.tool_name.clone(),
                    attempt_index + 1,
                    structured,
                );
            }

            let result = self
                .execute_public_tool_ref_internal_with_mode(
                    request.tool_name.as_str(),
                    &request.args,
                    policy.prevalidated,
                    policy.exec_settlement_mode,
                )
                .await;

            match result {
                Ok(output) => {
                    if let Some(structured_error) = ToolExecutionError::from_tool_output(&output) {
                        let structured = retry_policy.apply_to_tool_execution_error(
                            structured_error
                                .with_tool_call_context(request.tool_name.as_str(), &request.args)
                                .with_attempt(attempt_index + 1)
                                .with_surface("tool_registry"),
                            attempt_index,
                            Some(request.tool_name.as_str()),
                        );

                        let retry_delay = structured
                            .retry_after()
                            .or_else(|| structured.retry_delay());
                        if structured.retryable
                            && attempt_index + 1 < max_attempts
                            && let Some(delay) = retry_delay
                        {
                            last_error = Some(structured);
                            tokio::time::sleep(delay).await;
                            attempt_index = attempt_index.saturating_add(1);
                            continue;
                        }

                        return ToolExecutionOutcome::failure(
                            request.tool_name.clone(),
                            attempt_index + 1,
                            structured,
                        );
                    }

                    return ToolExecutionOutcome::success(
                        request.tool_name.clone(),
                        attempt_index + 1,
                        output,
                    );
                }
                Err(error) => {
                    let mut base = ToolExecutionError::from_anyhow(
                        request.tool_name.clone(),
                        &error,
                        attempt_index,
                        false,
                        false,
                        Some("tool_registry"),
                    );
                    let lower_message = base.message.to_ascii_lowercase();
                    let lower_original = base
                        .original_error
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    if lower_message.contains("circuit breaker")
                        || lower_original.contains("circuit breaker")
                    {
                        base.category = ErrorCategory::CircuitOpen;
                        base.retryable = true;
                        base.is_recoverable = true;
                        if base.retry_delay_ms.is_none() {
                            base.retry_delay_ms = Some(policy.retry_base_delay.as_millis() as u64);
                        }
                    }

                    let structured = retry_policy.apply_to_tool_execution_error(
                        base,
                        attempt_index,
                        Some(request.tool_name.as_str()),
                    );
                    let retry_delay = structured
                        .retry_after()
                        .or_else(|| structured.retry_delay());
                    if structured.retryable
                        && attempt_index + 1 < max_attempts
                        && let Some(delay) = retry_delay
                    {
                        last_error = Some(structured);
                        tokio::time::sleep(delay).await;
                        attempt_index = attempt_index.saturating_add(1);
                        continue;
                    }

                    return ToolExecutionOutcome::failure(
                        request.tool_name.clone(),
                        attempt_index + 1,
                        structured,
                    );
                }
            }
        }

        ToolExecutionOutcome::failure(
            request.tool_name.clone(),
            max_attempts,
            last_error.unwrap_or_else(|| {
                ToolExecutionError::new(
                    request.tool_name.clone(),
                    ToolErrorType::ExecutionError,
                    format!(
                        "Tool '{}' failed after {} attempts with no structured error",
                        request.tool_name, max_attempts
                    ),
                )
                .with_surface("tool_registry")
            }),
        )
    }

    async fn should_skip_loop_detection_for_active_exec_continuation(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> bool {
        if tool_name != tools::UNIFIED_EXEC {
            return false;
        }

        let Some(action) = crate::tools::tool_intent::unified_exec_action(args) else {
            return false;
        };
        let is_continuation = action.eq_ignore_ascii_case("poll")
            || (action.eq_ignore_ascii_case("continue")
                && crate::tools::command_args::interactive_input_text(args).is_none());
        if !is_continuation {
            return false;
        }

        let Some(session_id) = crate::tools::command_args::session_id_text(args) else {
            return false;
        };

        matches!(self.exec_session_completed(session_id).await, Ok(None))
    }

    async fn public_tool_catalog_for_error(
        &self,
        requested_name: &str,
    ) -> (Vec<String>, Vec<String>) {
        let mut tool_names = self.available_tools().await;
        tool_names.sort_unstable();
        tool_names.dedup();

        let requested_candidates = public_tool_name_candidates(requested_name);
        let mut similar_tools = Vec::new();

        if let Ok(resolved) = self.resolve_public_tool_name_sync(requested_name)
            && tool_names.iter().any(|tool| tool == &resolved)
        {
            similar_tools.push(resolved);
        }

        for tool in &tool_names {
            if similar_tools.len() >= 3 {
                break;
            }

            if similar_tools.iter().any(|candidate| candidate == tool) {
                continue;
            }

            if requested_candidates
                .iter()
                .any(|candidate| fuzzy_match(candidate, tool))
            {
                similar_tools.push(tool.clone());
            }
        }

        (tool_names, similar_tools)
    }

    pub fn preflight_validate_call(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<super::ToolPreflightOutcome> {
        execution_kernel::preflight_validate_call(self, name, args)
    }

    pub fn admit_public_tool_call(&self, name: &str, args: &Value) -> Result<PreparedToolCall> {
        let preflight = self.preflight_validate_call(name, args)?;
        Ok(PreparedToolCall::new(
            preflight.normalized_tool_name,
            preflight.readonly_classification,
            preflight.parallel_safe_after_preflight,
            preflight.effective_args,
        ))
    }

    pub async fn execute_tool(&self, name: &str, args: Value) -> Result<Value> {
        self.execute_tool_ref(name, &args).await
    }

    /// Execute a model-originated tool call through the public routing assembly.
    pub async fn execute_public_tool_ref(&self, name: &str, args: &Value) -> Result<Value> {
        self.execute_public_tool_ref_internal(name, args, false)
            .await
    }

    /// Reference-taking version of execute_tool to avoid cloning by callers
    /// that already have access to an existing `Value`.
    pub async fn execute_tool_ref(&self, name: &str, args: &Value) -> Result<Value> {
        self.execute_tool_ref_internal(name, args, false, ExecSettlementMode::Manual)
            .await
    }

    /// Reference-taking execution entrypoint for calls that were already preflight-validated.
    ///
    /// This avoids re-running argument/schema/path/command preflight in hot paths
    /// where validation already happened in the runloop.
    pub async fn execute_tool_ref_prevalidated(&self, name: &str, args: &Value) -> Result<Value> {
        self.execute_tool_ref_internal(name, args, true, ExecSettlementMode::Manual)
            .await
    }

    /// Prevalidated model-originated execution that still routes through the public assembly.
    pub async fn execute_public_tool_ref_prevalidated(
        &self,
        name: &str,
        args: &Value,
    ) -> Result<Value> {
        self.execute_public_tool_ref_prevalidated_with_mode(name, args, ExecSettlementMode::Manual)
            .await
    }

    #[doc(hidden)]
    pub async fn execute_public_tool_ref_prevalidated_with_mode(
        &self,
        name: &str,
        args: &Value,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
        self.execute_public_tool_ref_internal_with_mode(name, args, true, exec_settlement_mode)
            .await
    }

    pub async fn execute_prepared_public_tool_ref_with_mode(
        &self,
        prepared: &PreparedToolCall,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
        self.execute_public_tool_ref_internal_with_mode(
            prepared.canonical_name.as_str(),
            &prepared.effective_args,
            prepared.already_preflighted,
            exec_settlement_mode,
        )
        .await
    }

    async fn execute_public_tool_ref_internal(
        &self,
        name: &str,
        args: &Value,
        prevalidated: bool,
    ) -> Result<Value> {
        self.execute_public_tool_ref_internal_with_mode(
            name,
            args,
            prevalidated,
            ExecSettlementMode::Manual,
        )
        .await
    }

    async fn execute_public_tool_ref_internal_with_mode(
        &self,
        name: &str,
        args: &Value,
        prevalidated: bool,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
        let routed_name = self
            .resolve_public_tool(name)
            .map(|resolution| resolution.registration_name().to_string())
            .map_err(|error| anyhow!(error.to_string()))?;
        let effective_args = execution_kernel::remap_public_unified_file_alias_args(
            name,
            routed_name.as_str(),
            args,
        );
        self.execute_tool_ref_internal(
            routed_name.as_str(),
            effective_args.as_ref().unwrap_or(args),
            prevalidated,
            exec_settlement_mode,
        )
        .await
    }

    async fn execute_tool_ref_internal(
        &self,
        name: &str,
        args: &Value,
        prevalidated: bool,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
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
        if let Some(tool_arc) = cached_tool.as_ref()
            && self
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

        let parameter_schema = self
            .inventory
            .registration_for(&tool_name)
            .and_then(|registration| registration.parameter_schema().cloned());
        let normalized_args =
            execution_kernel::normalize_tool_args(&tool_name, args, parameter_schema.as_ref())?;
        let args = normalized_args.as_ref();
        let requested_name = name.to_string();

        // Clone args once at the start for error recording paths (clone only here)
        let args_for_recording = args.clone();
        // Capture harness context snapshot for structured telemetry and history
        let context_snapshot = self.harness_context_snapshot();
        let record_failure = |tool_name: String,
                              is_mcp_tool: bool,
                              mcp_provider: Option<String>,
                              args: Value,
                              error_msg: String,
                              timeout_category: Option<String>,
                              base_timeout_ms: Option<u64>,
                              adaptive_timeout_ms: Option<u64>,
                              effective_timeout_ms: Option<u64>,
                              circuit_breaker: bool| {
            self.execution_history
                .add_record(ToolExecutionRecord::failure(
                    tool_name,
                    requested_name.clone(),
                    is_mcp_tool,
                    mcp_provider,
                    args,
                    error_msg,
                    context_snapshot.clone(),
                    timeout_category,
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    effective_timeout_ms,
                    circuit_breaker,
                ));
        };

        let _reentrancy_guard = match ToolReentrancyGuard::enter(&tool_name) {
            Ok(guard) => guard,
            Err(violation) => {
                let reentry_count = violation.tool_reentry_count + 1;
                let error_message = format!(
                    "REENTRANCY GUARD: Tool '{}' was blocked to prevent recursive execution.\n\n\
                     ACTION REQUIRED: DO NOT retry this same tool call without changing control flow.\n\
                     Current stack depth: {}. Re-entry count for this tool in the current task: {}.\n\
                     Stack trace: {}",
                    display_name, violation.stack_depth, reentry_count, violation.stack_trace
                );
                let error = ToolExecutionError::new(
                    tool_name_owned.clone(),
                    ToolErrorType::PolicyViolation,
                    error_message.clone(),
                );
                let mut payload = error.to_json_value();
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("reentrant_call_blocked".into(), json!(true));
                    obj.insert("stack_depth".into(), json!(violation.stack_depth));
                    obj.insert("reentry_count".into(), json!(reentry_count));
                    obj.insert("tool".into(), json!(display_name));
                    obj.insert("stack_trace".into(), json!(violation.stack_trace));
                }
                record_failure(
                    tool_name_owned.clone(),
                    false,
                    None,
                    args_for_recording.clone(),
                    error_message,
                    None,
                    None,
                    None,
                    None,
                    false,
                );
                return Ok(payload);
            }
        };

        let readonly_classification = if prevalidated {
            #[cfg(debug_assertions)]
            {
                if let Err(err) =
                    execution_kernel::preflight_validate_resolved_call(self, &tool_name, args)
                    && !agent_execution::is_plan_mode_denial(&err.to_string())
                {
                    debug_assert!(
                        false,
                        "prevalidated execution received invalid call for '{}': {}",
                        tool_name, err
                    );
                }
            }
            !crate::tools::tool_intent::classify_tool_intent(&tool_name, args).mutating
        } else {
            match execution_kernel::preflight_validate_resolved_call(self, &tool_name, args) {
                Ok(outcome) => outcome.readonly_classification,
                Err(err) => {
                    let err_msg = err.to_string();
                    record_failure(
                        tool_name_owned.clone(),
                        false,
                        None,
                        args_for_recording.clone(),
                        err_msg,
                        None,
                        None,
                        None,
                        None,
                        false,
                    );
                    return Err(err);
                }
            }
        };

        if readonly_classification {
            trace!(tool = %tool_name, "Validation classified tool as read-only");
        }

        // Defense-in-depth: prevalidated fast path skips full preflight, but plan-mode
        // mutating-tool enforcement remains a hard safety invariant.
        if self.is_plan_mode() && !self.is_plan_mode_allowed(&tool_name, args) {
            let error_msg = agent_execution::plan_mode_denial_message(&display_name);
            record_failure(
                tool_name_owned.clone(),
                false,
                None,
                args_for_recording.clone(),
                error_msg.clone(),
                None,
                None,
                None,
                None,
                false,
            );
            return Err(anyhow!(error_msg).context(agent_execution::PLAN_MODE_DENIED_CONTEXT));
        }

        let shared_circuit_breaker = self.shared_circuit_breaker();
        if let Some(breaker) = shared_circuit_breaker.as_ref()
            && !breaker.allow_request_for_tool(&tool_name)
        {
            let diagnostics = breaker.get_diagnostics(&tool_name);
            let retry_after = diagnostics
                .remaining_backoff
                .map(|backoff| format!(" retry_after={}s.", backoff.as_secs()))
                .unwrap_or_default();
            let error_msg = format!(
                "Tool '{}' is temporarily disabled due to high failure rate (Circuit Breaker OPEN).{}",
                display_name, retry_after
            );
            self.execution_history.add_record(
                ToolExecutionRecord::failure(
                    tool_name_owned.clone(),
                    requested_name.clone(),
                    false,
                    None,
                    args_for_recording.clone(),
                    error_msg.clone(),
                    context_snapshot.clone(),
                    None,
                    None,
                    None,
                    None,
                    true,
                )
                .with_circuit_breaker_state(format!("{:?}", diagnostics.status))
                .with_retry_after(diagnostics.remaining_backoff),
            );
            return Err(anyhow!(error_msg).context("tool denied by circuit breaker"));
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

        trace!(
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
            .unwrap_or_else(|poisoned| {
                warn!("timeout policy lock poisoned while reading execution timeout; recovering");
                poisoned.into_inner()
            })
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
                warn!(
                    tool = %tool_name_owned,
                    requested = %requested_name,
                    calls_last_minute,
                    rate_limit,
                    "Execution history rate-limit threshold exceeded (observability-only)"
                );
            }
        }

        let skip_loop_detection = self
            .should_skip_loop_detection_for_active_exec_continuation(&tool_name, args)
            .await;
        if skip_loop_detection {
            trace!(
                tool = %tool_name,
                "Skipping identical-call loop detection for active exec continuation"
            );
        }

        // LOOP DETECTION: Check if we're calling the same tool repeatedly with identical params
        let loop_limit = if skip_loop_detection {
            0
        } else {
            self.execution_history.loop_limit_for(&tool_name, args)
        };
        let (is_loop, repeat_count, _) = if skip_loop_detection {
            (false, 0, String::new())
        } else {
            self.execution_history.detect_loop(&tool_name, args)
        };
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
                if readonly_classification {
                    let reuse_max_age = Duration::from_secs(120);
                    let reused = self
                        .execution_history
                        .find_recent_spooled_result(&tool_name, args, reuse_max_age)
                        .or_else(|| {
                            self.execution_history.find_recent_successful_result(
                                &tool_name,
                                args,
                                reuse_max_age,
                            )
                        });
                    if let Some(mut reused_value) = reused {
                        if let Some(obj) = reused_value.as_object_mut() {
                            obj.insert("reused_recent_result".into(), json!(true));
                            obj.insert("loop_detected".into(), json!(true));
                            obj.insert("repeat_count".into(), json!(repeat_count));
                            obj.insert("limit".into(), json!(loop_limit));
                            obj.insert("tool".into(), json!(display_name));
                            let reused_spooled =
                                obj.get("spool_path").and_then(|v| v.as_str()).is_some();
                            let note = if reused_spooled {
                                "Loop detected; reusing a recent spooled output for this identical read-only call. Continue from the spool file instead of re-running the tool."
                            } else {
                                "Loop detected; reusing a recent successful output for this identical read-only call. Change approach before calling the same tool again."
                            };
                            obj.insert("loop_detected_note".into(), json!(note));
                        }
                        return Ok(reused_value);
                    }
                }

                let delay_ms = (75 * repeat_count as u64).min(500);
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }

                let error = ToolExecutionError::new(
                    tool_name_owned.clone(),
                    ToolErrorType::PolicyViolation,
                    agent_execution::loop_detection_block_message(
                        &display_name,
                        repeat_count as u64,
                        None,
                    ),
                );
                let mut payload = error.to_json_value();
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("loop_detected".into(), json!(true));
                    obj.insert("repeat_count".into(), json!(repeat_count));
                    obj.insert("limit".into(), json!(loop_limit));
                    obj.insert("tool".into(), json!(display_name));
                }

                record_failure(
                    tool_name_owned,
                    false,
                    None,
                    args_for_recording,
                    "Tool call blocked due to repeated identical invocations".to_string(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                );

                return Ok(payload);
            }
        }

        let full_auto_denied = {
            let gateway = self.policy_gateway.lock().await;
            gateway.has_full_auto_allowlist() && !gateway.is_allowed_in_full_auto(&tool_name)
        };
        if full_auto_denied {
            let _error = ToolExecutionError::new(
                tool_name_owned.clone(),
                ToolErrorType::PolicyViolation,
                format!(
                    "Tool '{}' is not permitted while full-auto mode is active",
                    display_name
                ),
            );

            record_failure(
                tool_name_owned.clone(),
                false,
                None,
                args_for_recording.clone(),
                "Tool execution denied by policy".to_string(),
                timeout_category_label.clone(),
                base_timeout_ms,
                adaptive_timeout_ms,
                None,
                false,
            );

            return Err(anyhow!(
                "Tool '{}' is not permitted while full-auto mode is active",
                display_name
            )
            .context("tool denied by full-auto allowlist"));
        }

        let skip_policy_prompt = self
            .policy_gateway
            .lock()
            .await
            .take_preapproved(&tool_name);

        let decision = if skip_policy_prompt {
            ToolExecutionDecision::Allowed
        } else {
            // In TUI mode, permission should have been collected via ensure_tool_permission().
            // If not preapproved, check policy as fallback.
            self.policy_gateway
                .lock()
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

            record_failure(
                tool_name_owned.clone(),
                false,
                None,
                args_for_recording.clone(),
                error_msg.clone(),
                timeout_category_label.clone(),
                base_timeout_ms,
                adaptive_timeout_ms,
                None,
                false,
            );

            return Err(anyhow!("{}", error_msg).context("tool denied by policy"));
        }

        let args = match self
            .policy_gateway
            .lock()
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

                record_failure(
                    tool_name_owned,
                    false,
                    None,
                    args_for_recording,
                    format!("Failed to apply policy constraints: {}", err),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                );

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
        if let Some((provider, remote_tool)) = parse_canonical_mcp_tool_name(&tool_name) {
            needs_pty = true;
            tool_exists = true;
            is_mcp_tool = true;
            mcp_provider = Some(provider.to_string());
            mcp_tool_name = Some(remote_tool.to_string());
        }

        let mcp_client_opt = { self.mcp_client.read().ok().and_then(|g| g.clone()) };
        if !is_mcp_tool && let Some(mcp_client) = mcp_client_opt {
            let mut resolved_mcp_name = legacy_mcp_tool_name(name)
                .map(str::to_string)
                .unwrap_or_else(|| tool_name_owned.clone());

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
                    mcp_provider = self.find_mcp_provider(&resolved_mcp_name).await;
                    mcp_tool_name = Some(resolved_mcp_name);
                }
                Ok(false) => {
                    // Don't modify tool_exists here - keep the result from standard tool check.
                    // Setting tool_exists = false would incorrectly override a valid standard tool.
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

                record_failure(
                    tool_name_owned,
                    is_mcp_tool,
                    mcp_provider.clone(),
                    args_for_recording,
                    format!("Failed to resolve MCP tool '{}': {}", display_name, err),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                );

                return Ok(error.to_json_value());
            }

            let (all_tool_names, similar_tools) = self.public_tool_catalog_for_error(name).await;
            let suggestion = if !similar_tools.is_empty() {
                format!(" Did you mean: {}?", similar_tools.join(", "))
            } else {
                String::new()
            };
            let available_tool_list = all_tool_names.join(", ");
            let message = format!(
                "Unknown tool: {}. Available tools: {}.{}",
                display_name, available_tool_list, suggestion
            );
            let error = ToolExecutionError::new(
                tool_name_owned.clone(),
                ToolErrorType::ToolNotFound,
                message.clone(),
            );

            record_failure(
                tool_name_owned,
                is_mcp_tool,
                mcp_provider.clone(),
                args_for_recording,
                message,
                timeout_category_label.clone(),
                base_timeout_ms,
                adaptive_timeout_ms,
                None,
                false,
            );

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
            self.execution_history.add_record(
                ToolExecutionRecord::failure(
                    tool_name_owned,
                    requested_name.clone(),
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
                )
                .with_circuit_breaker_state(format!("{:?}", diag.state))
                .with_retry_after(diag.retry_after),
            );
            return Ok(payload);
        }

        trace!(
            tool = %tool_name,
            requested = %name,
            is_mcp = is_mcp_tool,
            uses_pty = needs_pty,
            alias = %if tool_name == name { "" } else { name },
            mcp_provider = %mcp_provider.as_deref().unwrap_or(""),
            "Resolved tool route"
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

                    record_failure(
                        tool_name_owned,
                        is_mcp_tool,
                        mcp_provider.clone(),
                        args_for_recording,
                        "Failed to start PTY session".to_string(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        None,
                        false,
                    );

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
                let mcp_name = mcp_tool_name
                    .as_deref()
                    .context("MCP tool routing inconsistency: resolved MCP tool name missing")?;
                self.execute_mcp_tool(mcp_name, args).await
            } else if exec_settlement_mode.settle_noninteractive()
                && tool_name == tools::UNIFIED_EXEC
            {
                if self.optimization_config.memory_pool.enabled {
                    let _execution_guard = self.memory_pool.get_value();
                    let _string_guard = self.memory_pool.get_string();
                    let _vec_guard = self.memory_pool.get_vec();
                    self.execute_unified_exec_internal(args, exec_settlement_mode)
                        .await
                } else {
                    self.execute_unified_exec_internal(args, exec_settlement_mode)
                        .await
                }
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
                let (tool_names, similar_tools) =
                    self.public_tool_catalog_for_error(&requested_name).await;
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

                record_failure(
                    tool_name_owned.clone(),
                    is_mcp_tool,
                    mcp_provider.clone(),
                    args_for_recording.clone(),
                    error_msg,
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    effective_timeout_ms,
                    false,
                );

                Ok(error.to_json_value())
            }
        };

        let result = if let Some(limit) = effective_timeout {
            trace!(
                tool = %tool_name_owned,
                category = %timeout_category.label(),
                timeout_ms = %limit.as_millis(),
                "Executing tool with effective timeout"
            );
            match tokio::time::timeout(limit, exec_future).await {
                Ok(res) => res,
                Err(_) => {
                    let timeout_ms = limit.as_millis() as u64;
                    let tripped = self.record_tool_failure(timeout_category);
                    if tripped {
                        warn!(
                            tool = %tool_name_owned,
                            category = %timeout_category.label(),
                            "Tool circuit breaker tripped after consecutive timeout failures"
                        );
                    }
                    let retry_after = self.should_circuit_break(timeout_category);

                    let mut timeout_error = ToolExecutionError::new(
                        tool_name_owned.clone(),
                        ToolErrorType::Timeout,
                        format!(
                            "Operation '{}' exceeded the {} timeout ceiling ({}s)",
                            tool_name_owned,
                            timeout_category.label(),
                            limit.as_secs()
                        ),
                    )
                    .with_tool_call_context(&tool_name_owned, &args_for_recording)
                    .with_surface("tool_registry")
                    .with_debug_metadata("timeout_category", timeout_category.label())
                    .with_debug_metadata("timeout_ms", timeout_ms.to_string());

                    if tool_name_owned == tools::UNIFIED_EXEC {
                        timeout_error.recovery_suggestions = vec![
                            Cow::Borrowed(
                                "Use unified_exec with action='poll' to check command progress",
                            ),
                            Cow::Borrowed(
                                "Use unified_exec with action='list' to find active sessions",
                            ),
                            Cow::Borrowed(
                                "Use unified_exec with action='close' if a stale session is still active",
                            ),
                        ];
                    }

                    if let Some(delay) = retry_after {
                        timeout_error.retry_after_ms =
                            Some(delay.as_millis().min(u128::from(u64::MAX)) as u64);
                    }

                    let mut timeout_payload = timeout_error.to_json_value();
                    Self::annotate_timeout_error_payload(
                        &mut timeout_payload,
                        timeout_category.label(),
                        timeout_ms,
                        tripped,
                    );

                    if let Some(breaker) = shared_circuit_breaker.as_ref() {
                        breaker.record_failure_category_for_tool(
                            &tool_name_owned,
                            ErrorCategory::Timeout,
                        );
                    }
                    if is_mcp_tool {
                        self.mcp_circuit_breaker
                            .record_failure_category(ErrorCategory::Timeout);
                    }
                    record_failure(
                        tool_name_owned,
                        is_mcp_tool,
                        mcp_provider,
                        args_for_recording,
                        timeout_error.user_message(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        Some(timeout_ms),
                        tripped,
                    );
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
                if let Some(breaker) = shared_circuit_breaker.as_ref() {
                    breaker.record_success_for_tool(&tool_name_owned);
                }
                if is_mcp_tool {
                    self.mcp_circuit_breaker.record_success();
                }
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

                if !readonly_classification {
                    self.execution_history.clear();
                }

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
                let error = ToolExecutionError::from_anyhow(
                    tool_name_owned.clone(),
                    &err,
                    0,
                    false,
                    false,
                    Some("tool_registry"),
                )
                .with_tool_call_context(&tool_name_owned, &args_for_recording);
                let error_category = error.category;
                if let Some(breaker) = shared_circuit_breaker.as_ref() {
                    breaker.record_failure_category_for_tool(&tool_name_owned, error_category);
                }
                if is_mcp_tool {
                    self.mcp_circuit_breaker
                        .record_failure_category(error_category);
                }

                let tripped = if error_category.should_trip_circuit_breaker() {
                    let tripped = self.record_tool_failure(timeout_category);
                    if tripped {
                        warn!(
                            tool = %tool_name_owned,
                            category = %timeout_category.label(),
                            "Tool circuit breaker tripped after consecutive failures"
                        );
                    }
                    tripped
                } else {
                    false
                };

                let mut payload = error.to_json_value();
                Self::annotate_timeout_error_payload(
                    &mut payload,
                    timeout_category.label(),
                    effective_timeout_ms.unwrap_or(0),
                    tripped,
                );

                record_failure(
                    tool_name_owned,
                    is_mcp_tool,
                    mcp_provider,
                    args_for_recording,
                    format!("Tool execution failed: {}", err),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    effective_timeout_ms,
                    tripped,
                );

                Ok(payload)
            }
        }
    }
}
