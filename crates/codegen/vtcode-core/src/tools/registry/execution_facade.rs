//! Tool execution entrypoints for ToolRegistry.

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::future::Future;
use std::time::{Duration, Instant};
use tracing::{trace, warn};
use vtcode_commons::ErrorCategory;

use crate::config::constants::tools;
use crate::core::agent::harness_kernel::PreparedToolCall;
use crate::core::memory_pool::SizeRecommendation;
use crate::tool_policy::ToolExecutionDecision;
use crate::tools::error_messages::agent_execution;
use crate::tools::request_response::{ToolCallRequest, ToolCallResponse};
use crate::tools::tool_intent;
use crate::tools::unified_error::UnifiedErrorKind;
use crate::tools::unified_error::UnifiedToolError;
use crate::ui::search::fuzzy_match;

use super::assembly::public_tool_name_candidates;
use super::execution_kernel;
use super::reentrancy::ToolReentrancyGuard;
use super::{
    ExecSettlementMode, ExecutionPolicySnapshot, ToolErrorType, ToolExecutionError, ToolExecutionOutcome,
    ToolExecutionRecord, ToolExecutionRequest, ToolRegistry,
};
use vtcode_config::constants::execution::{LOOP_THROTTLE_MAX_MS, LOOP_THROTTLE_REGISTRY_BASE_MS};

/// When a read-only tool call has been repeated this many times, stop returning
/// cached results and return a hard error instead.  Must be greater than
/// MIN_READONLY_IDENTICAL_LIMIT (currently 2).
const LOOP_HARD_BLOCK_REPEAT_COUNT: usize = 5;

impl ToolRegistry {
    fn annotate_timeout_error_payload(
        payload: &mut Value,
        timeout_category: &str,
        timeout_ms: u64,
        circuit_breaker: bool,
    ) {
        if let Some(obj) = payload.get_mut("error").and_then(|value| value.as_object_mut()) {
            obj.insert("timeout_category".into(), Value::String(timeout_category.to_string()));
            obj.insert("timeout_ms".into(), Value::from(timeout_ms));
            obj.insert("circuit_breaker".into(), Value::Bool(circuit_breaker));
        }
    }

    pub fn safety_gateway(&self) -> std::sync::Arc<crate::tools::safety_gateway::SafetyGateway> {
        std::sync::Arc::clone(&self.safety_gateway)
    }

    /// Inline-delegating wrapper that returns the inner future directly to
    /// avoid an extra coroutine state machine (audit section 16).
    pub fn execute_public_tool_request(
        &self,
        request: ToolExecutionRequest,
    ) -> impl Future<Output = ToolExecutionOutcome> + '_ {
        self.execute_tool_request_internal(request)
    }

    pub async fn execute_prepared_public_tool_request(
        &self,
        prepared: &PreparedToolCall,
        policy: ExecutionPolicySnapshot,
    ) -> ToolExecutionOutcome {
        let request = ToolExecutionRequest::new(prepared.canonical_name.clone(), prepared.effective_args.clone())
            .with_policy(
                policy
                    .with_prevalidated(prepared.already_preflighted)
                    .with_safety_prevalidated(false),
            );
        self.execute_tool_request_internal(request).await
    }

    async fn should_skip_loop_detection_for_exec_continuation(&self, tool_name: &str, args: &Value) -> bool {
        if tool_name == tools::WRITE_STDIN {
            return matches!(
                crate::tools::command_args::write_stdin_dispatch(args),
                Ok(crate::tools::command_args::WriteStdinDispatch::Poll
                    | crate::tools::command_args::WriteStdinDispatch::Wait,)
            );
        }

        if tool_name != tools::UNIFIED_EXEC {
            return false;
        }

        if !tool_intent::command_session_action_in(args, &["poll", "continue"]) {
            return false;
        }
        if tool_intent::command_session_action_is(args, "continue")
            && crate::tools::command_args::interactive_input_text(args).is_some()
        {
            return false;
        }

        let Some(session_id) = crate::tools::command_args::session_id_text(args) else {
            return false;
        };

        matches!(self.exec_session_completed(session_id).await, Ok(None))
    }

    async fn public_tool_catalog_for_error(&self, requested_name: &str) -> (Vec<String>, Vec<String>) {
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

            if requested_candidates.iter().any(|candidate| fuzzy_match(candidate, tool)) {
                similar_tools.push(tool.clone());
            }
        }

        (tool_names, similar_tools)
    }

    pub fn preflight_validate_call(&self, name: &str, args: &Value) -> Result<super::ToolPreflightOutcome> {
        execution_kernel::preflight_validate_call(self, name, args)
    }

    /// Preflight a tool call from the harness path, allowing dispatch to
    /// internal (model-hidden) tool registrations such as read_file/write_file.
    /// Direct model-originated entry should use `preflight_validate_call`.
    pub fn preflight_validate_harness_call(&self, name: &str, args: &Value) -> Result<super::ToolPreflightOutcome> {
        execution_kernel::preflight_validate_call_with_mode(self, name, args, execution_kernel::DispatchMode::Harness)
    }

    pub fn admit_public_tool_call(&self, name: &str, args: &Value) -> Result<PreparedToolCall> {
        let preflight = self.preflight_validate_harness_call(name, args)?;
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
        self.execute_public_tool_ref_internal(name, args, false).await
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
    pub async fn execute_public_tool_ref_prevalidated(&self, name: &str, args: &Value) -> Result<Value> {
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
        // Prepared calls come from the harness admission gate
        // (`admit_public_tool_call`), which is the only producer of
        // `PreparedToolCall`, so internal (model-hidden) dispatch is authorized.
        self.execute_public_tool_ref_dispatch(
            prepared.canonical_name.as_str(),
            &prepared.effective_args,
            prepared.already_preflighted,
            execution_kernel::DispatchMode::Harness,
            exec_settlement_mode,
        )
        .await
    }

    async fn execute_public_tool_ref_internal(&self, name: &str, args: &Value, prevalidated: bool) -> Result<Value> {
        self.execute_public_tool_ref_dispatch(
            name,
            args,
            prevalidated,
            execution_kernel::DispatchMode::ModelPublic,
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
        self.execute_public_tool_ref_dispatch(
            name,
            args,
            prevalidated,
            execution_kernel::DispatchMode::ModelPublic,
            exec_settlement_mode,
        )
        .await
    }

    /// Core public-routing execution entrypoint.
    ///
    /// `dispatch_mode` is the authority to fall back to internal (model-hidden)
    /// tool registrations and is fully independent of `prevalidated` (a pure
    /// performance flag that skips re-running preflight). Only callers that went
    /// through the harness admission gate (`admit_public_tool_call`) pass
    /// [`execution_kernel::DispatchMode::Harness`]; direct model-originated entry always passes
    /// [`execution_kernel::DispatchMode::ModelPublic`], so a stray `prevalidated=true` can never by
    /// itself widen the dispatchable surface.
    pub(super) async fn execute_public_tool_ref_dispatch(
        &self,
        name: &str,
        args: &Value,
        prevalidated: bool,
        dispatch_mode: execution_kernel::DispatchMode,
        exec_settlement_mode: ExecSettlementMode,
    ) -> Result<Value> {
        let routed_name = execution_kernel::resolve_dispatch_target(self, name, dispatch_mode)
            .map_err(|err| anyhow!(err.to_string()))?;
        let effective_args = execution_kernel::remap_public_file_operation_alias_args(name, routed_name.as_str(), args)
            .or_else(|| execution_kernel::remap_consolidated_action_alias_args(name, routed_name.as_str(), args));
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
        if let Err(error) = crate::core::agent::snapshots::declare_prompt_edit(
            self.harness_context_snapshot().session_id,
            name.to_owned(),
            args.clone(),
        )
        .await
        {
            tracing::warn!(
                tool = %name,
                error = %error,
                "Checkpoint pre-image capture failed; rewind may not restore this edit"
            );
        }
        // PERFORMANCE OPTIMIZATION: Use memory pool for string allocations if enabled
        let _pool_guard = if self.optimization_config.memory_pool.enabled {
            Some(self.memory_pool.get_string())
        } else {
            None
        };

        // PERFORMANCE OPTIMIZATION: Auto-tune memory pool based on usage patterns
        if self.optimization_config.memory_pool.enabled {
            let recommendation = self.memory_pool.auto_tune(&self.optimization_config.memory_pool);

            // Log recommendation if significant changes are suggested
            if !matches!(
                (
                    recommendation.string_size_recommendation,
                    recommendation.value_size_recommendation,
                    recommendation.vec_size_recommendation
                ),
                (SizeRecommendation::Maintain, SizeRecommendation::Maintain, SizeRecommendation::Maintain)
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

        let resolved_name = self.resolve_tool_name_with_display(name);
        let tool_name = resolved_name.canonical;
        let tool_name_owned = tool_name.clone();
        let display_name = resolved_name.display;

        // PERFORMANCE OPTIMIZATION: Check hot cache for tool lookup using the canonical name.
        // This must happen AFTER alias resolution so that aliased tools resolve to their
        // canonical cache entry on the first hit. Without this, the cache lookup uses the
        // raw alias string while insertion uses the canonical name, making aliased tools
        // perpetually miss the cache.
        let cached_tool = if self.optimization_config.tool_registry.use_optimized_registry {
            let cache = self.hot_tool_cache.read();
            cache.peek(&tool_name).cloned()
        } else {
            None
        };

        // PERFORMANCE OPTIMIZATION: Update hot cache with resolved tool if optimizations enabled
        if let Some(tool_arc) = cached_tool.as_ref()
            && self.optimization_config.tool_registry.use_optimized_registry
            && tool_name != name
        {
            // Cache the canonical name too for faster future lookups
            self.hot_tool_cache.write().put(tool_name.clone(), tool_arc.clone());
        }

        let execution_args = self.prepare_execution_args(&tool_name, args)?;
        let is_verification_command = execution_args.is_verification_command;
        let max_output_tokens = execution_args.max_output_tokens;
        let args = execution_args.handler_args.as_ref();
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
            self.execution_history.add_record(ToolExecutionRecord::failure(
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

        let allow_parallel_sibling = prevalidated && tool_intent::is_parallel_safe_call(&tool_name, args);
        let _reentrancy_guard = match ToolReentrancyGuard::enter(&tool_name, allow_parallel_sibling) {
            Ok(guard) => guard,
            Err(violation) => {
                let reentry_count = violation.tool_reentry_count + 1;
                let error_message = format!(
                    "Reentrancy guard: tool '{}' is already running in this call stack, so this recursive call was blocked. \
                     Repeating the same call is blocked the same way; change the control flow or use a different tool.\n\
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
                    error_message.clone(),
                    None,
                    None,
                    None,
                    None,
                    false,
                );
                return Err(anyhow!(error_message).context("tool reentrancy blocked"));
            }
        };

        // Classify the tool intent once: the prevalidated fast path skips full
        // preflight and classifies here; the full-preflight path reuses the
        // intent the kernel already computed on the validated (executed) args,
        // so planning enforcement below cannot disagree with
        // `readonly_classification`.
        let (intent, readonly_classification) = if prevalidated {
            #[cfg(debug_assertions)]
            {
                if let Err(err) = execution_kernel::preflight_validate_resolved_call(self, &tool_name, args)
                    && !agent_execution::is_planning_active_denial(&err.to_string())
                {
                    debug_assert!(false, "prevalidated execution received invalid call for '{tool_name}': {err}");
                }
            }
            let intent = tool_intent::classify_tool_intent(&tool_name, args);
            (intent, !intent.mutating)
        } else {
            match execution_kernel::preflight_validate_resolved_call(self, &tool_name, args) {
                Ok(outcome) => (outcome.intent, outcome.readonly_classification),
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

        // Defense-in-depth: prevalidated fast path skips full preflight, but planning workflow
        // mutating-tool enforcement remains a hard safety invariant. Reuse the already-computed
        // `intent` so we don't reclassify on the hot path.
        if self.is_planning_active() && !self.is_planning_active_allowed_with_intent(&tool_name, args, &intent) {
            let error_msg = agent_execution::planning_workflow_denial_message(&display_name);
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
            return Err(anyhow!(error_msg).context(agent_execution::PLANNING_DENIED_CONTEXT));
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
                "Tool '{display_name}' is temporarily disabled due to high failure rate (Circuit Breaker OPEN).{retry_after}"
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

        let timeout_category = self.timeout_category_for_args(&tool_name, args).await;

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
            let calls_last_minute = self.execution_history.calls_in_window(Duration::from_secs(60));
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

        let fresh_patch_read = self.consume_patch_recovery_read(&tool_name, args);
        // Stateful tracker/decision calls must observe the current canonical
        // task, including identical calls after request or permission changes.
        let reusable_result =
            readonly_classification && !matches!(tool_name.as_str(), tools::RECORD_DECISION | tools::TASK_TRACKER);
        let skip_loop_detection = self.should_skip_loop_detection_for_exec_continuation(&tool_name, args).await;
        if skip_loop_detection {
            trace!(
                tool = %tool_name,
                "Skipping identical-call loop detection for stateful exec continuation"
            );
        }

        // FAST REUSE: Read-only inspection calls are often repeated verbatim within a
        // single turn (e.g. `diff a b | wc -l`, `find ... | grep`). Reuse the most recent
        // successful result immediately instead of paying for another round-trip and
        // another spool file. This is gated by a short TTL and the read-only classification
        // so mutating calls never take this path.
        //
        // Verification commands (`cargo check`, `cargo nextest run`, `cargo fmt
        // --check`, and pure `&&` chains thereof) are read-only by intent but
        // must always re-execute: reusing a stale success would clear the
        // anti-blind-editing gate without verifying the current worktree, and
        // reusing a stale failure would keep the gate pending after a fix.
        // (`is_verification_command` is bound once at preview-budget
        // resolution above; the stripped output-metadata field does not
        // affect shell classification.)
        if reusable_result && !is_verification_command && !skip_loop_detection && !fresh_patch_read {
            let fast_reuse_max_age = Duration::from_secs(60);
            let fast_reused = self
                .execution_history
                .find_recent_spooled_result(&tool_name, args, fast_reuse_max_age)
                .or_else(|| {
                    self.execution_history
                        .find_recent_successful_result(&tool_name, args, fast_reuse_max_age)
                });
            if let Some(mut reused_value) = fast_reused {
                if let Some(obj) = reused_value.as_object_mut() {
                    obj.insert("reused_recent_result".into(), json!(true));
                    obj.insert("tool".into(), json!(display_name));
                    let reused_spooled = obj.get("spool_path").and_then(|v| v.as_str()).is_some();
                    let note = if reused_spooled {
                        "Reusing a recent spooled output for this identical read-only call. Continue from the spool file instead of re-running the tool."
                    } else {
                        "Reusing a recent successful output for this identical read-only call."
                    };
                    obj.insert("reused_result_note".into(), json!(note));
                }
                // Record a synthetic "reused" entry so subsequent
                // `detect_loop` / `find_recent_*` queries see this call in the
                // history.  Previously the fast-reuse path returned the
                // cached payload without recording, which meant `detect_loop`
                // could not account for the reused call — leading to
                // undercounting on the very next turn and silent cache
                // poisoning.
                //
                // We pass `is_mcp_tool = false` and `mcp_provider = None`
                // because the fast-reuse gate has already filtered for
                // read-only calls and we don't have those locals in scope at
                // this point in the function.  The cached record itself
                // (added when the original call ran) carries the true MCP
                // metadata; the synthetic record exists only to make
                // `detect_loop` see this call in the rolling window.
                self.execution_history.add_record(ToolExecutionRecord::success(
                    tool_name.clone(),
                    requested_name.clone(),
                    false,
                    None,
                    args_for_recording.clone(),
                    reused_value.clone(),
                    context_snapshot.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                ));
                trace!(
                    tool = %tool_name,
                    "Fast-reusing recent successful read-only result"
                );
                return Ok(reused_value);
            }
        }

        // LOOP DETECTION: Check if we're calling the same tool repeatedly with identical params
        let loop_limit = if skip_loop_detection {
            0
        } else {
            self.execution_history.loop_limit_for(&tool_name, args)
        };
        let loop_result = if skip_loop_detection {
            crate::tools::registry::execution_history::LoopDetectionResult {
                detected: false,
                repeat_count: 0,
                tool_name: tool_name.clone(),
            }
        } else {
            self.execution_history.detect_loop(&tool_name, args)
        };
        if loop_result.detected && loop_result.repeat_count > 1 {
            let delay_ms = (LOOP_THROTTLE_REGISTRY_BASE_MS * loop_result.repeat_count as u64).min(LOOP_THROTTLE_MAX_MS);
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
        if loop_limit > 0 && loop_result.detected {
            warn!(
                tool = %tool_name,
                repeats = loop_result.repeat_count,
                "Loop detected: agent calling same tool with identical parameters {} times",
                loop_result.repeat_count
            );
            if loop_result.repeat_count >= loop_limit {
                // Hard block: when the model has been repeating the same call far
                // beyond the limit, stop returning cached results and return an
                // error.  Returning a cached result at high repeat counts causes
                // the model to see "success" and keep retrying.
                let hard_block = loop_result.repeat_count >= LOOP_HARD_BLOCK_REPEAT_COUNT;

                if reusable_result && !hard_block && !fresh_patch_read {
                    let reuse_max_age = Duration::from_secs(120);
                    let reused = self
                        .execution_history
                        .find_recent_spooled_result(&tool_name, args, reuse_max_age)
                        .or_else(|| {
                            self.execution_history
                                .find_recent_successful_result(&tool_name, args, reuse_max_age)
                        });
                    if let Some(mut reused_value) = reused {
                        if let Some(obj) = reused_value.as_object_mut() {
                            obj.insert("reused_recent_result".into(), json!(true));
                            obj.insert("loop_detected".into(), json!(true));
                            obj.insert("repeat_count".into(), json!(loop_result.repeat_count));
                            obj.insert("limit".into(), json!(loop_limit));
                            obj.insert("tool".into(), json!(display_name));
                            let reused_spooled = obj.get("spool_path").and_then(|v| v.as_str()).is_some();
                            let note = if reused_spooled {
                                "Loop detected: this identical read-only call has been repeated, so the earlier result was reused. The full output is in the spool file and the conversation history; further repeats return an error instead."
                            } else {
                                "Loop detected: this identical read-only call has been repeated with no new information, so the earlier result was reused. It is already in the conversation history; further repeats return an error instead."
                            };
                            obj.insert("loop_detected_note".into(), json!(note));
                        }
                        return Ok(reused_value);
                    }
                }

                let delay_ms =
                    (LOOP_THROTTLE_REGISTRY_BASE_MS * loop_result.repeat_count as u64).min(LOOP_THROTTLE_MAX_MS);
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }

                let error = ToolExecutionError::new(
                    tool_name_owned.clone(),
                    ToolErrorType::PolicyViolation,
                    agent_execution::loop_detection_block_message(&display_name, loop_result.repeat_count as u64, None),
                );
                let mut payload = error.to_json_value();
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("loop_detected".into(), json!(true));
                    obj.insert("repeat_count".into(), json!(loop_result.repeat_count));
                    obj.insert("limit".into(), json!(loop_limit));
                    obj.insert("tool".into(), json!(display_name));
                    obj.insert(
                        "next_action".into(),
                        json!("Identical calls to this tool are blocked. Use the data already in the conversation history; for different information, change the arguments or use another tool."),
                    );
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
            let gateway = self.policy_gateway.clone();
            let tool_name_ref = &tool_name;
            async move { gateway.is_denied_in_full_auto(tool_name_ref).await }
        };
        let full_auto_denied = full_auto_denied.await;
        if full_auto_denied {
            let _error = ToolExecutionError::new(
                tool_name_owned.clone(),
                ToolErrorType::PolicyViolation,
                format!("Tool '{display_name}' is not permitted while full-auto permission review is active"),
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

            return Err(anyhow!("Tool '{display_name}' is not permitted while full-auto permission review is active")
                .context("tool denied by full-auto allowlist"));
        }

        let skip_policy_prompt = self.policy_gateway.take_preapproved(&tool_name).await;

        let decision = if skip_policy_prompt {
            ToolExecutionDecision::Allowed
        } else {
            self.policy_gateway.should_execute_tool(&tool_name).await?
        };

        if !decision.is_allowed() {
            let error_msg = match decision {
                ToolExecutionDecision::DeniedWithFeedback(feedback) => {
                    format!("Tool '{display_name}' denied by user: {feedback}")
                }
                _ => format!("Tool '{display_name}' execution denied by policy"),
            };

            let _error =
                ToolExecutionError::new(tool_name_owned.clone(), ToolErrorType::PolicyViolation, error_msg.clone());

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

            return Err(anyhow!("{error_msg}").context("tool denied by policy"));
        }

        let gateway = self.policy_gateway.clone();
        let constrained_result = gateway.apply_policy_constraints(&tool_name, args).await;
        let mut args = match constrained_result {
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
                    format!("Failed to apply policy constraints: {err}"),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                );

                return Err(anyhow!(error.to_json_value()).context("tool denied by policy constraints"));
            }
        };

        let super::execution_stages::ExecutionRoute {
            route:
                super::execution_stages::ToolRoute {
                    needs_pty,
                    tool_exists,
                    is_mcp: is_mcp_tool,
                    mcp_provider,
                    mcp_tool_name,
                },
            mcp_lookup_error,
        } = self.resolve_execution_route(name, &tool_name).await;

        // If tool doesn't exist in either registry, return an error
        if !tool_exists {
            if let Some(err) = mcp_lookup_error {
                let error = ToolExecutionError::with_original_error(
                    tool_name_owned.clone(),
                    ToolErrorType::ExecutionError,
                    format!("Failed to resolve MCP tool '{display_name}': {err}"),
                    err.to_string(),
                );

                record_failure(
                    tool_name_owned,
                    is_mcp_tool,
                    mcp_provider.clone(),
                    args_for_recording,
                    format!("Failed to resolve MCP tool '{display_name}': {err}"),
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
            let message = format!("Unknown tool: {display_name}. Available tools: {available_tool_list}.{suggestion}");
            let error = ToolExecutionError::new(tool_name_owned.clone(), ToolErrorType::ToolNotFound, message.clone());

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
                format!("MCP circuit breaker {:?}; skipping execution", diag.status),
            );
            let payload = json!({
                "error": error.to_json_value(),
                "circuit_breaker_state": format!("{:?}", diag.status),
                "consecutive_failures": diag.consecutive_failures,
                "note": "MCP provider circuit breaker open; execution skipped",
                "last_failed_at_ago_ms": diag.last_failure_time
                    .map(|ts| ts.elapsed().as_millis() as u64),
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
                    format!("MCP circuit breaker {:?}; execution skipped", diag.status),
                    context_snapshot.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                )
                .with_circuit_breaker_state(format!("{:?}", diag.status))
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
        // Effective timeout: explicit waits self-bound inside the command
        // session executor (no outer deadline); long-running runs get the
        // generous long-running ceiling. See `effective_timeout_for_call`.
        let effective_timeout = self.effective_timeout_for_call(timeout_category, &args);
        let effective_timeout_ms = effective_timeout.map(|d| d.as_millis() as u64);

        let fail_open = self.optimization_config.tool_registry.middleware_fail_open;
        let middleware_req = ToolCallRequest {
            id: requested_name.clone(),
            tool_name: tool_name.as_str().into(),
            args: args.clone(),
            metadata: None,
        };
        if let Err(err) = self.middleware.before_execute_opt(&middleware_req, fail_open).await {
            if !fail_open {
                let error_msg = format!("Middleware denied execution: {err}");
                record_failure(
                    tool_name_owned.clone(),
                    is_mcp_tool,
                    mcp_provider.clone(),
                    args_for_recording.clone(),
                    error_msg.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                );
                return Err(anyhow!(error_msg).context("tool denied by middleware"));
            }
        }

        // Preserve registered approval/sandbox wrappers. The nonce changes any
        // wrapper cache key and only disables file caching; it grants no authority.
        if fresh_patch_read
            && !tool_intent::is_command_run_tool_call(&tool_name, &args)
            && let Some(object) = args.as_object_mut()
        {
            object.insert(
                crate::tools::file_ops::PATCH_READ_CACHE_NONCE.to_string(),
                json!(uuid::Uuid::new_v4().to_string()),
            );
        }
        let exec_future = async {
            if is_mcp_tool {
                let mcp_name = mcp_tool_name
                    .as_deref()
                    .context("MCP tool routing inconsistency: resolved MCP tool name missing")?;
                self.execute_mcp_tool(mcp_name, args).await
            } else if exec_settlement_mode.settle_noninteractive()
                && matches!(tool_name.as_str(), tools::UNIFIED_EXEC | tools::EXEC_COMMAND | tools::EXEC_PTY_CMD)
            {
                let exec_args = match tool_name.as_str() {
                    tools::EXEC_COMMAND => super::executors::normalize_command_session_run_alias_args(&args, false)?,
                    tools::EXEC_PTY_CMD => super::executors::normalize_command_session_run_alias_args(&args, true)?,
                    _ => args.clone(),
                };
                if self.optimization_config.memory_pool.enabled {
                    let _execution_guard = self.memory_pool.get_value();
                    let _string_guard = self.memory_pool.get_string();
                    let _vec_guard = self.memory_pool.get_vec();
                    self.execute_command_session_internal(exec_args, exec_settlement_mode).await
                } else {
                    self.execute_command_session_internal(exec_args, exec_settlement_mode).await
                }
            } else if exec_settlement_mode.settle_noninteractive() && tool_name == tools::WRITE_STDIN {
                self.execute_write_stdin(args, exec_settlement_mode).await
            } else if let Some(registration) = self.inventory.registration_for(&tool_name) {
                self.execute_registered_handler(&tool_name, &registration, args, cached_tool.as_ref())
                    .await
            } else {
                // This should theoretically never happen since we checked tool_exists above
                // Generate helpful error message with available tools
                let (tool_names, similar_tools) = self.public_tool_catalog_for_error(&requested_name).await;
                let available_tool_list = tool_names.join(", ");

                let suggestion = if !similar_tools.is_empty() {
                    format!(" Did you mean: {}?", similar_tools.join(", "))
                } else {
                    String::new()
                };

                let error_msg = format!(
                    "Tool '{display_name}' not found in registry. Available tools: {available_tool_list}.{suggestion}"
                );

                let error =
                    ToolExecutionError::new(tool_name_owned.clone(), ToolErrorType::ToolNotFound, error_msg.clone());

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
                            Cow::Borrowed("Use write_stdin with empty chars to poll command progress"),
                            Cow::Borrowed("Use exec_command with a fresh command if the original session is stale"),
                            Cow::Borrowed("Ask for manual cleanup if a stale session is still active"),
                        ];
                    }

                    if let Some(delay) = retry_after {
                        timeout_error.retry_after_ms = Some(delay.as_millis().min(u128::from(u64::MAX)) as u64);
                    }

                    let mut timeout_payload = timeout_error.to_json_value();
                    Self::annotate_timeout_error_payload(
                        &mut timeout_payload,
                        timeout_category.label(),
                        timeout_ms,
                        tripped,
                    );

                    if let Some(breaker) = shared_circuit_breaker.as_ref() {
                        breaker.record_failure_category_for_tool(&tool_name_owned, ErrorCategory::Timeout);
                    }
                    if is_mcp_tool {
                        self.mcp_circuit_breaker.record_failure_category(ErrorCategory::Timeout);
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
                let super::execution_results::ExecutionOutput { normalized_value, structured_error } = self
                    .prepare_execution_output(
                        &tool_name_owned,
                        &args_for_recording,
                        value,
                        is_mcp_tool,
                        max_output_tokens,
                    )
                    .await;

                if !readonly_classification {
                    self.invalidate_mutated_reads(&tool_name_owned, &args_for_recording);
                }

                if let Some(error_msg) = structured_error {
                    self.execution_history.add_record(ToolExecutionRecord::failure(
                        tool_name_owned,
                        requested_name,
                        is_mcp_tool,
                        mcp_provider,
                        args_for_recording,
                        error_msg,
                        context_snapshot.clone(),
                        timeout_category_label.clone(),
                        base_timeout_ms,
                        adaptive_timeout_ms,
                        effective_timeout_ms,
                        false,
                    ));
                } else {
                    self.execution_history.add_record(ToolExecutionRecord::success(
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
                }

                let _ = self
                    .middleware
                    .after_execute(
                        &middleware_req,
                        &ToolCallResponse {
                            id: middleware_req.id.clone(),
                            success: true,
                            result: Some(normalized_value.clone()),
                            error: None,
                            duration_ms: Some(execution_started_at.elapsed().as_millis() as u64),
                            cache_hit: None,
                        },
                    )
                    .await;

                Ok(normalized_value)
            }
            Err(err) => {
                // Reentrancy violations must surface as hard errors rather
                // than wrapped Ok(error_object) so nested callers see the
                // failure and can react appropriately.
                if err.to_string().contains("tool reentrancy blocked") {
                    return Err(err);
                }

                let error = ToolExecutionError::from_anyhow(
                    tool_name_owned.clone(),
                    &err,
                    0,
                    false,
                    false,
                    Some("tool_registry"),
                )
                .with_tool_call_context(&tool_name_owned, &args_for_recording);
                self.grant_patch_recovery_read(&error).await;
                let error_category = error.category;
                if error.circuit_breaker_impact
                    && let Some(breaker) = shared_circuit_breaker.as_ref()
                {
                    breaker.record_failure_category_for_tool(&tool_name_owned, error_category);
                }
                if error.circuit_breaker_impact && is_mcp_tool {
                    self.mcp_circuit_breaker.record_failure_category(error_category);
                }

                let tripped = if error.circuit_breaker_impact {
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
                    format!("Tool execution failed: {err}"),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    effective_timeout_ms,
                    tripped,
                );

                let _ = self
                    .middleware
                    .on_error(
                        &middleware_req,
                        &UnifiedToolError::new(
                            UnifiedErrorKind::from(vtcode_commons::classify_anyhow_error(&err)),
                            err.to_string(),
                        ),
                    )
                    .await;

                Ok(payload)
            }
        }
    }
}
