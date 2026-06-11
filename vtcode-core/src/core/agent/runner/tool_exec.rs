use super::AgentRunner;
use super::constants::{LOOP_THROTTLE_BASE_MS, LOOP_THROTTLE_MAX_MS};
use super::tool_execution_guard::ToolExecutionGuard;
use super::types::ToolFailureContext;
use crate::core::agent::events::{
    ExecEventRecorder, tool_invocation_completed_event, tool_output_payload_from_value,
};
use crate::core::agent::harness_kernel::{
    FallbackRecommendation, PreparedToolBatch, PreparedToolBatchKind, PreparedToolCall,
    reduce_tool_result,
};
use crate::core::agent::runtime::{AgentRuntime, RuntimeControl};
use crate::exec::events::{ItemCompletedEvent, ThreadEvent, ThreadItemDetails, ToolCallStatus};
use crate::llm::provider::ToolCall;
use crate::tools::registry::{ToolErrorType, ToolExecutionError};
use anyhow::Result;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{error, info, warn};
use vtcode_commons::ErrorCategory;

struct ToolCallItemRef {
    call_item_id: String,
    synthetic_invocation: bool,
}

#[derive(Clone)]
struct PreparedRunnerToolCall {
    tool_call_id: String,
    prepared: PreparedToolCall,
}

struct PreparedRunnerToolBatch {
    kind: PreparedToolBatchKind,
    calls: Vec<PreparedRunnerToolCall>,
}

enum RunnerCallAdmission {
    Prepared(Box<PreparedRunnerToolCall>),
    Rejected,
    StopTurn,
}

fn snapshot_circuit_diagnostics(
    runner: &AgentRunner,
    tool_name: &str,
) -> Option<crate::tools::circuit_breaker::ToolCircuitDiagnostics> {
    runner
        .tool_registry
        .shared_circuit_breaker()
        .map(|breaker| breaker.get_diagnostics(tool_name))
}

fn record_circuit_transition(
    runner: &AgentRunner,
    error_recovery: &Arc<
        parking_lot::Mutex<crate::core::agent::error_recovery::ErrorRecoveryState>,
    >,
    tool_name: &str,
    before: Option<crate::tools::circuit_breaker::ToolCircuitDiagnostics>,
) {
    let Some(before) = before else {
        return;
    };
    let Some(breaker) = runner.tool_registry.shared_circuit_breaker() else {
        return;
    };
    let after = breaker.get_diagnostics(tool_name);
    if before.status != after.status || after.denied_requests > before.denied_requests {
        error_recovery
            .lock()
            .record_circuit_transition(&before, &after);
    }
}

fn resolve_tool_call_item(
    runtime: &AgentRuntime,
    event_recorder: &mut ExecEventRecorder,
    tool_name: &str,
    args: &serde_json::Value,
    tool_call_id: &str,
) -> ToolCallItemRef {
    if let Some(call_item_id) = runtime.tool_call_item_id(tool_call_id) {
        return ToolCallItemRef {
            call_item_id,
            synthetic_invocation: false,
        };
    }

    let handle = event_recorder.tool_started(tool_name, Some(args), Some(tool_call_id));
    ToolCallItemRef {
        call_item_id: handle.item_id().to_string(),
        synthetic_invocation: true,
    }
}

fn complete_tool_invocation(
    runtime: &mut AgentRuntime,
    event_recorder: &mut ExecEventRecorder,
    tool_call_id: &str,
    tool_name: &str,
    args: &serde_json::Value,
    tool_call_item: &ToolCallItemRef,
    status: ToolCallStatus,
) {
    if tool_call_item.synthetic_invocation {
        event_recorder.record_thread_event(tool_invocation_completed_event(
            tool_call_item.call_item_id.clone(),
            tool_name,
            Some(args),
            Some(tool_call_id),
            status,
        ));
        return;
    }

    runtime.complete_tool_call(tool_call_id, status);
    event_recorder.record_thread_events(runtime.take_emitted_events());
}

fn reject_tool_call(
    runtime: &mut AgentRuntime,
    event_recorder: &mut ExecEventRecorder,
    tool_name: &str,
    args: Option<&serde_json::Value>,
    tool_call_id: &str,
    detail: &str,
) {
    if runtime.tool_call_item_id(tool_call_id).is_some() {
        runtime.complete_tool_call(tool_call_id, ToolCallStatus::Failed);
        let lifecycle_events = runtime.take_emitted_events();
        event_recorder.record_thread_events(lifecycle_events.clone());
        emit_failed_tool_outputs_for_completed_invocations(
            event_recorder,
            &lifecycle_events,
            detail,
        );
        event_recorder.warning(detail);
        return;
    }

    event_recorder.tool_rejected(tool_name, args, Some(tool_call_id), detail);
}

/// Reject a tool call whose arguments could not be parsed or admitted.
///
/// Logs at error level, pushes a structured tool error onto the conversation,
/// and emits the rejection lifecycle events.
fn reject_invalid_args(
    runtime: &mut AgentRuntime,
    event_recorder: &mut ExecEventRecorder,
    agent_prefix: &str,
    tool_name: &str,
    tool_call_id: &str,
    args: Option<&serde_json::Value>,
    err: &dyn std::fmt::Display,
    is_gemini: bool,
    log_msg: &'static str,
) {
    let detail = format!("Invalid arguments for tool '{tool_name}': {err}");
    error!(agent = %agent_prefix, tool = %tool_name, error = %err, "{log_msg}");
    reject_tool_call(
        runtime,
        event_recorder,
        tool_name,
        args,
        tool_call_id,
        &detail,
    );
    runtime.state.push_tool_error(
        tool_call_id.to_string(),
        tool_name,
        &serde_json::Value::String(detail),
        is_gemini,
    );
}

/// Reject a tool call that policy or feature gating disallows.
///
/// Records the warning on the session, logs at warn level (unless quiet), and
/// emits the rejection lifecycle events.
fn reject_denied_tool(
    runtime: &mut AgentRuntime,
    event_recorder: &mut ExecEventRecorder,
    agent_prefix: &str,
    tool_name: &str,
    tool_call_id: &str,
    args: Option<&serde_json::Value>,
    is_gemini: bool,
    quiet: bool,
) {
    let detail = format!("Tool execution denied: {tool_name}");
    if !quiet {
        warn!(agent = %agent_prefix, tool = %tool_name, message = %detail);
    }
    runtime.state.warnings.push(detail.clone());
    runtime.state.push_tool_error(
        tool_call_id.to_string(),
        tool_name,
        &serde_json::Value::String(detail.clone()),
        is_gemini,
    );
    reject_tool_call(
        runtime,
        event_recorder,
        tool_name,
        args,
        tool_call_id,
        &detail,
    );
}

fn emit_failed_tool_outputs_for_completed_invocations(
    event_recorder: &mut ExecEventRecorder,
    lifecycle_events: &[ThreadEvent],
    detail: &str,
) {
    for event in lifecycle_events {
        let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = event else {
            continue;
        };
        let ThreadItemDetails::ToolInvocation(details) = &item.details else {
            continue;
        };
        event_recorder.tool_output_started(&item.id, details.tool_call_id.as_deref());
        event_recorder.tool_output_finished(
            &item.id,
            details.tool_call_id.as_deref(),
            details.status.clone(),
            None,
            detail,
            None,
        );
    }
}

fn finish_successful_tool_output(
    event_recorder: &mut ExecEventRecorder,
    call_item_id: &str,
    tool_call_id: &str,
    output: &serde_json::Value,
) {
    let payload = tool_output_payload_from_value(output);
    event_recorder.tool_output_finished(
        call_item_id,
        Some(tool_call_id),
        ToolCallStatus::Completed,
        None,
        &payload.aggregated_output,
        payload.spool_path.as_deref(),
    );
}

fn apply_tool_success(
    runner: &AgentRunner,
    runtime: &mut AgentRuntime,
    event_recorder: &mut ExecEventRecorder,
    agent_prefix: &str,
    call_id: &str,
    name: &str,
    args: &serde_json::Value,
    tool_call_item: &ToolCallItemRef,
    result: serde_json::Value,
    is_gemini: bool,
) {
    if !runner.quiet {
        info!(agent = %agent_prefix, tool = %name, "Tool executed successfully");
    }
    let optimized_result = reduce_tool_result(name, result);
    runner.update_last_paths_from_args(name, args, &mut runtime.state);
    runtime
        .state
        .push_tool_result(call_id.to_string(), name, &optimized_result, is_gemini);
    complete_tool_invocation(
        runtime,
        event_recorder,
        call_id,
        name,
        args,
        tool_call_item,
        ToolCallStatus::Completed,
    );
    finish_successful_tool_output(
        event_recorder,
        &tool_call_item.call_item_id,
        call_id,
        &optimized_result,
    );
}

/// The outcome of evaluating whether a tool failure should halt further tool
/// calls in the current turn.
#[must_use]
#[derive(Debug, Clone, PartialEq)]
enum ToolHaltDecision {
    /// Continue executing remaining tool calls normally.
    Continue,
    /// Stop dispatching further tool calls and surface a warning.
    Halt {
        /// Human-readable reason appended to session warnings.
        warning: &'static str,
        /// Whether the session's tool-loop-limit counter should be incremented.
        mark_loop_limit: bool,
    },
}

/// Pure classification function: maps an `ErrorCategory` to a halt decision
/// without touching any mutable state. This is the single place that encodes
/// which error categories should abort the current tool-call sequence.
#[inline]
fn classify_halt_decision(category: ErrorCategory) -> ToolHaltDecision {
    match category {
        ErrorCategory::RateLimit => ToolHaltDecision::Halt {
            warning: "Tool was rate limited; halting further tool calls this turn.",
            mark_loop_limit: true,
        },
        ErrorCategory::PolicyViolation | ErrorCategory::PlanningPolicyViolation => {
            ToolHaltDecision::Halt {
                warning: "Tool denied by policy; halting further tool calls this turn.",
                mark_loop_limit: false,
            }
        }
        _ => ToolHaltDecision::Continue,
    }
}

fn apply_tool_failure_halt_policy(
    session_state: &mut crate::core::agent::session::AgentSessionState,
    category: ErrorCategory,
) -> bool {
    match classify_halt_decision(category) {
        ToolHaltDecision::Continue => false,
        ToolHaltDecision::Halt {
            warning,
            mark_loop_limit,
        } => {
            session_state.warnings.push(warning.into());
            if mark_loop_limit {
                session_state.mark_tool_loop_limit_hit();
            }
            true
        }
    }
}

fn align_prepared_batches(
    calls: Vec<PreparedRunnerToolCall>,
    allow_parallel: bool,
) -> Vec<PreparedRunnerToolBatch> {
    let layout = PreparedToolBatch::plan_layout_with_names(
        calls.iter().map(|call| {
            (
                call.prepared.can_parallelize(),
                call.prepared.canonical_name.as_str(),
            )
        }),
        allow_parallel,
    );
    let mut calls = calls.into_iter();

    layout
        .into_iter()
        .map(|(kind, len)| PreparedRunnerToolBatch {
            kind,
            calls: calls.by_ref().take(len).collect(),
        })
        .collect()
}

impl AgentRunner {
    async fn throttle_repeated_tool(&self, name: &str) {
        let repeat_count = self.loop_detector.lock().get_call_count(name);
        if repeat_count > 1 {
            let delay_ms = (LOOP_THROTTLE_BASE_MS * repeat_count as u64).min(LOOP_THROTTLE_MAX_MS);
            if !self.quiet {
                info!(
                    agent = %self.agent_type,
                    tool = %name,
                    repeat_count,
                    delay_ms,
                    "Throttling repeated tool call"
                );
            }
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
    }

    async fn try_execute_fallback(
        &self,
        fallback: &FallbackRecommendation,
        runtime: &mut AgentRuntime,
        agent_prefix: &str,
        name: &str,
    ) -> Option<(serde_json::Value, String, serde_json::Value)> {
        info!(
            agent = %agent_prefix,
            tool = %name,
            fallback_tool = %fallback.tool_name,
            "Main tool execution failed; attempting fallback recommendation"
        );

        match self.admit_tool_call(
            &fallback.tool_name,
            fallback.args.clone(),
            &mut runtime.state,
        ) {
            Ok(fallback_prepared) => {
                match self
                    .execute_prepared_tool_internal(&fallback_prepared)
                    .await
                {
                    Ok(res) => Some((res, fallback.tool_name.clone(), fallback.args.clone())),
                    Err(fallback_err) => {
                        warn!(
                            agent = %agent_prefix,
                            tool = %fallback.tool_name,
                            error = %fallback_err.message,
                            "Fallback tool execution failed"
                        );
                        None
                    }
                }
            }
            Err(admit_err) => {
                warn!(
                    agent = %agent_prefix,
                    tool = %fallback.tool_name,
                    error = %admit_err,
                    "Failed to admit fallback tool call"
                );
                None
            }
        }
    }

    async fn admit_runner_tool_call(
        &self,
        call: ToolCall,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
    ) -> Result<RunnerCallAdmission> {
        let requested_name = match call.tool_name() {
            Some(name) => name.to_string(),
            None => return Ok(RunnerCallAdmission::Rejected),
        };
        let args = match call.execution_arguments() {
            Ok(args) => args,
            Err(err) => {
                reject_invalid_args(
                    runtime,
                    event_recorder,
                    agent_prefix,
                    &requested_name,
                    call.id.as_str(),
                    None,
                    &err,
                    is_gemini,
                    "Invalid tool arguments",
                );
                return Ok(RunnerCallAdmission::Rejected);
            }
        };
        if self
            .resolve_executable_tool_name(&requested_name)
            .await
            .is_none()
        {
            reject_denied_tool(
                runtime,
                event_recorder,
                agent_prefix,
                &requested_name,
                &call.id,
                Some(&args),
                is_gemini,
                self.quiet,
            );
            return Ok(RunnerCallAdmission::Rejected);
        }
        let args_for_error = args.clone();
        let prepared = match self.admit_tool_call(&requested_name, args, &mut runtime.state) {
            Ok(prepared) => prepared,
            Err(err) => {
                reject_invalid_args(
                    runtime,
                    event_recorder,
                    agent_prefix,
                    &requested_name,
                    call.id.as_str(),
                    Some(&args_for_error),
                    &err,
                    is_gemini,
                    "Tool admission failed",
                );
                return Ok(RunnerCallAdmission::Rejected);
            }
        };

        if self.check_for_loop(
            &prepared.canonical_name,
            &prepared.effective_args,
            &mut runtime.state,
        ) {
            return Ok(RunnerCallAdmission::StopTurn);
        }

        if !self.is_valid_tool(&prepared.canonical_name).await {
            reject_denied_tool(
                runtime,
                event_recorder,
                agent_prefix,
                &prepared.canonical_name,
                &call.id,
                Some(&prepared.effective_args),
                is_gemini,
                self.quiet,
            );
            return Ok(RunnerCallAdmission::Rejected);
        }

        Ok(RunnerCallAdmission::Prepared(Box::new(
            PreparedRunnerToolCall {
                tool_call_id: call.id,
                prepared,
            },
        )))
    }

    async fn execute_prepared_parallel_tool_calls(
        &self,
        prepared_calls: Vec<PreparedRunnerToolCall>,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
    ) -> Result<bool> {
        use futures::future::join_all;

        // Extract fallback recommendations before consuming the Vec.  Only
        // entries that actually carry a fallback are stored, keeping the map
        // small (most tool calls have no fallback).
        let fallback_map: std::collections::HashMap<String, FallbackRecommendation> =
            prepared_calls
                .iter()
                .filter_map(|c| {
                    c.prepared
                        .fallback_recommendation
                        .as_ref()
                        .map(|fb| (c.tool_call_id.clone(), fb.clone()))
                })
                .collect();

        let max_parallel = self.config().agent.harness.max_parallel_tool_calls;
        // Build a semaphore only when a finite concurrency cap is configured.
        // `0` means unlimited — matches the pre-semaphore behaviour.
        let semaphore: Option<Arc<tokio::sync::Semaphore>> =
            (max_parallel > 0).then(|| Arc::new(tokio::sync::Semaphore::new(max_parallel)));

        info!(
            agent = %self.agent_type,
            count = prepared_calls.len(),
            max_parallel,
            "Executing parallel tool calls"
        );

        let mut futures = Vec::with_capacity(prepared_calls.len());
        // Consume the Vec directly to avoid building an intermediate HashMap
        // and the associated key clones.  Fallback data was extracted above.
        for call in prepared_calls {
            let PreparedRunnerToolCall {
                tool_call_id,
                prepared,
            } = call;
            let name = prepared.canonical_name.clone();
            let args = prepared.effective_args.clone();
            let tool_call_item =
                resolve_tool_call_item(runtime, event_recorder, &name, &args, &tool_call_id);
            let runner = self;
            let circuit_before = snapshot_circuit_diagnostics(runner, &name);
            let sem = semaphore.clone();
            futures.push(async move {
                // Acquire a concurrency slot when the cap is in force.  The permit
                // is held for the duration of tool execution and released on drop.
                let _permit = if let Some(s) = sem {
                    match s.acquire_owned().await {
                        Ok(permit) => Some(permit),
                        Err(_) => {
                            return (
                                name,
                                tool_call_id,
                                args,
                                tool_call_item,
                                Err(ToolExecutionError::new(
                                    "parallel_tool_semaphore",
                                    ToolErrorType::ExecutionError,
                                    "parallel tool semaphore closed",
                                )),
                                circuit_before,
                            );
                        }
                    }
                } else {
                    None
                };
                runner.throttle_repeated_tool(&name).await;
                let result = runner.execute_prepared_tool_internal(&prepared).await;
                (
                    name,
                    tool_call_id,
                    args,
                    tool_call_item,
                    result,
                    circuit_before,
                )
            });
        }

        let results = join_all(futures).await;
        let mut halt_turn = false;
        for (name, call_id, args, tool_call_item, result, circuit_before) in results {
            record_circuit_transition(self, &runtime.state.error_recovery, &name, circuit_before);
            match result {
                Ok(result) => {
                    event_recorder
                        .tool_output_started(&tool_call_item.call_item_id, Some(&call_id));
                    apply_tool_success(
                        self,
                        runtime,
                        event_recorder,
                        agent_prefix,
                        &call_id,
                        &name,
                        &args,
                        &tool_call_item,
                        result,
                        is_gemini,
                    );
                }
                Err(e) => {
                    // Try fallback first before declaring failure
                    let mut fallback_succeeded = false;
                    let mut fallback_result = None;
                    let mut fallback_tool_name = None;
                    let mut fallback_args = None;

                    if let Some(fallback) = fallback_map.get(&call_id)
                        && let Some((res, f_name, f_args)) = self
                            .try_execute_fallback(fallback, runtime, agent_prefix, &name)
                            .await
                    {
                        fallback_succeeded = true;
                        fallback_result = Some(res);
                        fallback_tool_name = Some(f_name);
                        fallback_args = Some(f_args);
                    }

                    if let (true, Some(res), Some(f_name), Some(f_args)) = (
                        fallback_succeeded,
                        fallback_result,
                        fallback_tool_name,
                        fallback_args,
                    ) {
                        self.apply_fallback_success(
                            runtime,
                            event_recorder,
                            agent_prefix,
                            &call_id,
                            &f_name,
                            &f_args,
                            res,
                            &tool_call_item,
                            is_gemini,
                        )?;
                    } else {
                        let category = e.category;
                        let should_halt =
                            apply_tool_failure_halt_policy(&mut runtime.state, category);
                        complete_tool_invocation(
                            runtime,
                            event_recorder,
                            &call_id,
                            &name,
                            &args,
                            &tool_call_item,
                            ToolCallStatus::Failed,
                        );
                        let mut failure_ctx = ToolFailureContext {
                            agent_prefix,
                            session_state: &mut runtime.state,
                            event_recorder,
                            tool_call_id: &call_id,
                            call_item_id: Some(tool_call_item.call_item_id.as_str()),
                            is_gemini,
                        };
                        self.record_tool_failure(
                            &mut failure_ctx,
                            &name,
                            &e,
                            Some(call_id.as_str()),
                        );
                        if should_halt {
                            halt_turn = true;
                            break;
                        }
                    }
                }
            }
        }

        Ok(halt_turn)
    }

    async fn execute_prepared_sequential_tool_call(
        &mut self,
        call: PreparedRunnerToolCall,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
    ) -> Result<bool> {
        let name = call.prepared.canonical_name.clone();
        let args = call.prepared.effective_args.clone();

        if !self.quiet {
            info!(agent = %self.agent_type, tool = %name, "Calling tool");
        }

        let tool_call_item =
            resolve_tool_call_item(runtime, event_recorder, &name, &args, &call.tool_call_id);
        event_recorder.tool_output_started(&tool_call_item.call_item_id, Some(&call.tool_call_id));
        let circuit_before = snapshot_circuit_diagnostics(self, &name);

        self.throttle_repeated_tool(&name).await;

        let mut guard = ToolExecutionGuard::new(
            &name,
            &call.tool_call_id,
            runtime.state.error_recovery.clone(),
        );
        match self.execute_prepared_tool_internal(&call.prepared).await {
            Ok(result) => {
                guard.mark_completed();
                record_circuit_transition(
                    self,
                    &runtime.state.error_recovery,
                    &name,
                    circuit_before,
                );
                apply_tool_success(
                    self,
                    runtime,
                    event_recorder,
                    agent_prefix,
                    &call.tool_call_id,
                    &name,
                    &args,
                    &tool_call_item,
                    result,
                    is_gemini,
                );
                Ok(false)
            }
            Err(e) => {
                // Try fallback first before declaring failure
                let mut fallback_succeeded = false;
                let mut fallback_result = None;
                let mut fallback_tool_name = None;
                let mut fallback_args = None;

                if let Some(fallback) = &call.prepared.fallback_recommendation
                    && let Some((res, f_name, f_args)) = self
                        .try_execute_fallback(fallback, runtime, agent_prefix, &name)
                        .await
                {
                    fallback_succeeded = true;
                    fallback_result = Some(res);
                    fallback_tool_name = Some(f_name);
                    fallback_args = Some(f_args);
                }

                if let (true, Some(res), Some(f_name), Some(f_args)) = (
                    fallback_succeeded,
                    fallback_result,
                    fallback_tool_name,
                    fallback_args,
                ) {
                    guard.mark_completed();
                    record_circuit_transition(
                        self,
                        &runtime.state.error_recovery,
                        &f_name,
                        circuit_before,
                    );
                    self.apply_fallback_success(
                        runtime,
                        event_recorder,
                        agent_prefix,
                        &call.tool_call_id,
                        &f_name,
                        &f_args,
                        res,
                        &tool_call_item,
                        is_gemini,
                    )?;
                    Ok(false)
                } else {
                    // Main failure and fallback failed / not present
                    guard.mark_completed();
                    record_circuit_transition(
                        self,
                        &runtime.state.error_recovery,
                        &name,
                        circuit_before,
                    );
                    let category = e.category;
                    let should_halt = apply_tool_failure_halt_policy(&mut runtime.state, category);

                    complete_tool_invocation(
                        runtime,
                        event_recorder,
                        &call.tool_call_id,
                        &name,
                        &args,
                        &tool_call_item,
                        ToolCallStatus::Failed,
                    );
                    let mut failure_ctx = ToolFailureContext {
                        agent_prefix,
                        session_state: &mut runtime.state,
                        event_recorder,
                        tool_call_id: &call.tool_call_id,
                        call_item_id: Some(tool_call_item.call_item_id.as_str()),
                        is_gemini,
                    };
                    self.record_tool_failure(
                        &mut failure_ctx,
                        &name,
                        &e,
                        Some(call.tool_call_id.as_str()),
                    );

                    Ok(should_halt)
                }
            }
        }
    }

    pub(super) async fn execute_tool_call_batches(
        &mut self,
        tool_calls: Vec<ToolCall>,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
        previous_response_chain_present: bool,
    ) -> Result<()> {
        let mut pending: VecDeque<ToolCall> = tool_calls.into();
        let mut deferred: Option<PreparedRunnerToolCall> = None;

        while deferred.is_some() || !pending.is_empty() {
            if matches!(
                runtime.poll_tool_control().await,
                RuntimeControl::StopRequested
            ) {
                runtime.complete_open_tool_calls(ToolCallStatus::Failed);
                let lifecycle_events = runtime.take_emitted_events();
                event_recorder.record_thread_events(lifecycle_events.clone());
                emit_failed_tool_outputs_for_completed_invocations(
                    event_recorder,
                    &lifecycle_events,
                    "Tool execution interrupted by steering signal.",
                );
                warn!(agent = %agent_prefix, "Stopped by steering signal");
                return Ok(());
            }

            let first_call = if let Some(call) = deferred.take() {
                call
            } else {
                let Some(call) = pending.pop_front() else {
                    break;
                };
                match self
                    .admit_runner_tool_call(call, runtime, event_recorder, agent_prefix, is_gemini)
                    .await?
                {
                    RunnerCallAdmission::Prepared(call) => *call,
                    RunnerCallAdmission::Rejected => continue,
                    RunnerCallAdmission::StopTurn => return Ok(()),
                }
            };

            let mut batch_calls = vec![first_call];
            let allow_parallel = batch_calls[0].prepared.can_parallelize();

            if allow_parallel {
                while let Some(next_call) = pending.pop_front() {
                    match self
                        .admit_runner_tool_call(
                            next_call,
                            runtime,
                            event_recorder,
                            agent_prefix,
                            is_gemini,
                        )
                        .await?
                    {
                        RunnerCallAdmission::Prepared(call) if call.prepared.can_parallelize() => {
                            batch_calls.push(*call);
                        }
                        RunnerCallAdmission::Prepared(call) => {
                            deferred = Some(*call);
                            break;
                        }
                        RunnerCallAdmission::Rejected => {}
                        RunnerCallAdmission::StopTurn => return Ok(()),
                    }
                }
            }

            let allow_parallel_batch = allow_parallel && batch_calls.len() > 1;
            let planned_batches = align_prepared_batches(batch_calls, allow_parallel_batch);
            for batch in planned_batches {
                self.emit_tool_batch(
                    &self.get_selected_model(),
                    runtime.state.stats.turns_executed,
                    batch.calls.len(),
                    matches!(batch.kind, PreparedToolBatchKind::ParallelReadonly),
                    previous_response_chain_present,
                );

                let halted = match batch.kind {
                    PreparedToolBatchKind::ParallelReadonly => {
                        self.execute_prepared_parallel_tool_calls(
                            batch.calls,
                            runtime,
                            event_recorder,
                            agent_prefix,
                            is_gemini,
                        )
                        .await?
                    }
                    PreparedToolBatchKind::Sequential => {
                        let mut halted = false;
                        for call in batch.calls {
                            if self
                                .execute_prepared_sequential_tool_call(
                                    call,
                                    runtime,
                                    event_recorder,
                                    agent_prefix,
                                    is_gemini,
                                )
                                .await?
                            {
                                halted = true;
                                break;
                            }
                        }
                        halted
                    }
                };
                if halted {
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    /// Execute multiple tool calls in parallel. Only safe for read-only operations.
    #[cfg_attr(not(test), expect(dead_code))]
    pub(super) async fn execute_parallel_tool_calls(
        &mut self,
        tool_calls: Vec<ToolCall>,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
    ) -> Result<()> {
        let mut prepared_calls = Vec::with_capacity(tool_calls.len());
        for call in tool_calls {
            match self
                .admit_runner_tool_call(call, runtime, event_recorder, agent_prefix, is_gemini)
                .await?
            {
                RunnerCallAdmission::Prepared(call) => prepared_calls.push(*call),
                RunnerCallAdmission::Rejected => {}
                RunnerCallAdmission::StopTurn => return Ok(()),
            }
        }

        for batch in align_prepared_batches(prepared_calls, true) {
            match batch.kind {
                PreparedToolBatchKind::ParallelReadonly => {
                    let _ = self
                        .execute_prepared_parallel_tool_calls(
                            batch.calls,
                            runtime,
                            event_recorder,
                            agent_prefix,
                            is_gemini,
                        )
                        .await?;
                }
                PreparedToolBatchKind::Sequential => {
                    for call in batch.calls {
                        let _ = self
                            .execute_prepared_sequential_tool_call(
                                call,
                                runtime,
                                event_recorder,
                                agent_prefix,
                                is_gemini,
                            )
                            .await?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Execute multiple tool calls sequentially.
    #[cfg_attr(not(test), expect(dead_code))]
    pub(super) async fn execute_sequential_tool_calls(
        &mut self,
        tool_calls: Vec<ToolCall>,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
    ) -> Result<()> {
        for call in tool_calls {
            if matches!(
                runtime.poll_tool_control().await,
                RuntimeControl::StopRequested
            ) {
                runtime.complete_open_tool_calls(ToolCallStatus::Failed);
                let lifecycle_events = runtime.take_emitted_events();
                event_recorder.record_thread_events(lifecycle_events.clone());
                emit_failed_tool_outputs_for_completed_invocations(
                    event_recorder,
                    &lifecycle_events,
                    "Tool execution interrupted by steering signal.",
                );
                warn!(agent = %agent_prefix, "Stopped by steering signal");
                return Ok(());
            }

            let admitted = self
                .admit_runner_tool_call(call, runtime, event_recorder, agent_prefix, is_gemini)
                .await?;
            let RunnerCallAdmission::Prepared(call) = admitted else {
                if matches!(admitted, RunnerCallAdmission::StopTurn) {
                    break;
                }
                continue;
            };

            if self
                .execute_prepared_sequential_tool_call(
                    *call,
                    runtime,
                    event_recorder,
                    agent_prefix,
                    is_gemini,
                )
                .await?
            {
                break;
            }
        }

        Ok(())
    }

    /// Apply the side effects of a successful fallback tool invocation:
    /// reduce the result, push it into the conversation, finalize the
    /// invocation record, and emit the corresponding event recorder
    /// notifications. Shared between the parallel and sequential execution
    /// paths to keep fallback handling behaviorally identical.
    #[allow(clippy::too_many_arguments)]
    fn apply_fallback_success(
        &self,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        call_id: &str,
        tool_name: &str,
        tool_args: &serde_json::Value,
        result: serde_json::Value,
        tool_call_item: &ToolCallItemRef,
        is_gemini: bool,
    ) -> Result<()> {
        if !self.quiet {
            info!(
                agent = %agent_prefix,
                tool = %tool_name,
                "Fallback tool executed successfully"
            );
        }

        let optimized_result = reduce_tool_result(tool_name, result);

        self.update_last_paths_from_args(tool_name, tool_args, &mut runtime.state);

        runtime.state.push_tool_result(
            call_id.to_string(),
            tool_name,
            &optimized_result,
            is_gemini,
        );
        complete_tool_invocation(
            runtime,
            event_recorder,
            call_id,
            tool_name,
            tool_args,
            tool_call_item,
            ToolCallStatus::Completed,
        );
        event_recorder.tool_output_started(&tool_call_item.call_item_id, Some(call_id));
        finish_successful_tool_output(
            event_recorder,
            &tool_call_item.call_item_id,
            call_id,
            &optimized_result,
        );

        Ok(())
    }
}
