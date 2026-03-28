use super::AgentRunner;
use super::constants::{LOOP_THROTTLE_BASE_MS, LOOP_THROTTLE_MAX_MS};
use super::tool_execution_guard::ToolExecutionGuard;
use super::types::ToolFailureContext;
use crate::core::agent::events::{
    ExecEventRecorder, tool_invocation_completed_event, tool_output_payload_from_value,
};
use crate::core::agent::harness_kernel::{PreparedToolBatchKind, PreparedToolCall};
use crate::core::agent::runtime::{AgentRuntime, RuntimeControl};
use crate::exec::events::{ItemCompletedEvent, ThreadEvent, ThreadItemDetails, ToolCallStatus};
use crate::llm::provider::ToolCall;
use anyhow::Result;
use std::collections::{HashSet, VecDeque};
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
    error_recovery: &std::sync::Arc<
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

fn apply_tool_failure_halt_policy(
    session_state: &mut crate::core::agent::session::AgentSessionState,
    category: ErrorCategory,
) -> bool {
    if matches!(category, ErrorCategory::RateLimit) {
        session_state
            .warnings
            .push("Tool was rate limited; halting further tool calls this turn.".into());
        session_state.mark_tool_loop_limit_hit();
        return true;
    }

    if matches!(
        category,
        ErrorCategory::PolicyViolation | ErrorCategory::PlanModeViolation
    ) {
        session_state
            .warnings
            .push("Tool denied by policy; halting further tool calls this turn.".into());
        return true;
    }

    false
}

fn align_prepared_batches(
    calls: Vec<PreparedRunnerToolCall>,
    allow_parallel: bool,
) -> Vec<PreparedRunnerToolBatch> {
    if !allow_parallel {
        return calls
            .into_iter()
            .map(|call| PreparedRunnerToolBatch {
                kind: PreparedToolBatchKind::Sequential,
                calls: vec![call],
            })
            .collect();
    }

    let mut batches = Vec::new();
    let mut parallel_calls = Vec::new();
    let mut parallel_tool_names = HashSet::new();

    for call in calls {
        if !call.prepared.can_parallelize() {
            push_prepared_batch(
                &mut batches,
                &mut parallel_calls,
                PreparedToolBatchKind::ParallelReadonly,
            );
            parallel_tool_names.clear();
            batches.push(PreparedRunnerToolBatch {
                kind: PreparedToolBatchKind::Sequential,
                calls: vec![call],
            });
            continue;
        }

        if !parallel_calls.is_empty()
            && parallel_tool_names.contains(call.prepared.canonical_name.as_str())
        {
            push_prepared_batch(
                &mut batches,
                &mut parallel_calls,
                PreparedToolBatchKind::ParallelReadonly,
            );
            parallel_tool_names.clear();
        }

        parallel_tool_names.insert(call.prepared.canonical_name.clone());
        parallel_calls.push(call);
    }

    push_prepared_batch(
        &mut batches,
        &mut parallel_calls,
        PreparedToolBatchKind::ParallelReadonly,
    );
    batches
}

fn push_prepared_batch(
    batches: &mut Vec<PreparedRunnerToolBatch>,
    calls: &mut Vec<PreparedRunnerToolCall>,
    kind: PreparedToolBatchKind,
) {
    if calls.is_empty() {
        return;
    }

    let batch_kind = if matches!(kind, PreparedToolBatchKind::ParallelReadonly) && calls.len() == 1
    {
        PreparedToolBatchKind::Sequential
    } else {
        kind
    };

    batches.push(PreparedRunnerToolBatch {
        kind: batch_kind,
        calls: std::mem::take(calls),
    });
}

impl AgentRunner {
    async fn admit_runner_tool_call(
        &self,
        call: ToolCall,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
    ) -> Result<RunnerCallAdmission> {
        let requested_name = match call.function.as_ref() {
            Some(func) => func.name.clone(),
            None => return Ok(RunnerCallAdmission::Rejected),
        };
        let args = match call.parsed_arguments() {
            Ok(args) => args,
            Err(err) => {
                let error_msg = format!("Invalid arguments for tool '{}': {}", requested_name, err);
                error!(agent = %agent_prefix, tool = %requested_name, error = %err, "Invalid tool arguments");
                reject_tool_call(
                    runtime,
                    event_recorder,
                    &requested_name,
                    None,
                    call.id.as_str(),
                    &error_msg,
                );
                runtime.state.push_tool_error(
                    call.id.clone(),
                    &requested_name,
                    error_msg,
                    is_gemini,
                );
                return Ok(RunnerCallAdmission::Rejected);
            }
        };
        if self
            .resolve_executable_tool_name(&requested_name)
            .await
            .is_none()
        {
            let detail = format!("Tool execution denied: {}", requested_name);
            if !self.quiet {
                warn!(
                    agent = %agent_prefix,
                    tool = %requested_name,
                    message = %detail
                );
            }
            runtime.state.warnings.push(detail.clone());
            runtime.state.push_tool_error(
                call.id.clone(),
                &requested_name,
                detail.clone(),
                is_gemini,
            );
            reject_tool_call(
                runtime,
                event_recorder,
                &requested_name,
                Some(&args),
                &call.id,
                &detail,
            );
            return Ok(RunnerCallAdmission::Rejected);
        }
        let prepared = match self.admit_tool_call(&requested_name, &args, &mut runtime.state) {
            Ok(prepared) => prepared,
            Err(err) => {
                let error_msg = format!("Invalid arguments for tool '{}': {}", requested_name, err);
                error!(agent = %agent_prefix, tool = %requested_name, error = %err, "Tool admission failed");
                reject_tool_call(
                    runtime,
                    event_recorder,
                    &requested_name,
                    Some(&args),
                    call.id.as_str(),
                    &error_msg,
                );
                runtime.state.push_tool_error(
                    call.id.clone(),
                    &requested_name,
                    error_msg,
                    is_gemini,
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
            let detail = format!("Tool execution denied: {}", prepared.canonical_name);
            if !self.quiet {
                warn!(
                    agent = %agent_prefix,
                    tool = %prepared.canonical_name,
                    message = %detail
                );
            }
            runtime.state.warnings.push(detail.clone());
            runtime.state.push_tool_error(
                call.id.clone(),
                &prepared.canonical_name,
                detail.clone(),
                is_gemini,
            );
            reject_tool_call(
                runtime,
                event_recorder,
                &prepared.canonical_name,
                Some(&prepared.effective_args),
                &call.id,
                &detail,
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

        info!(
            agent = %self.agent_type,
            count = prepared_calls.len(),
            "Executing parallel tool calls"
        );

        let mut futures = Vec::with_capacity(prepared_calls.len());
        for call in prepared_calls {
            let name = call.prepared.canonical_name.clone();
            let args = call.prepared.effective_args.clone();
            let tool_call_item =
                resolve_tool_call_item(runtime, event_recorder, &name, &args, &call.tool_call_id);
            let runner = self;
            let prepared = call.prepared.clone();
            let circuit_before = snapshot_circuit_diagnostics(runner, &name);
            futures.push(async move {
                let result = runner.execute_prepared_tool_internal(&prepared).await;
                (
                    name,
                    call.tool_call_id,
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
                    if !self.quiet {
                        info!(agent = %agent_prefix, tool = %name, "Tool executed successfully");
                    }

                    let optimized_result = self.optimize_tool_result(&name, result);
                    let tool_result = serde_json::to_string(&optimized_result)?;

                    self.update_last_paths_from_args(&name, &args, &mut runtime.state);

                    runtime
                        .state
                        .push_tool_result(call_id.clone(), &name, tool_result, is_gemini);
                    complete_tool_invocation(
                        runtime,
                        event_recorder,
                        &call_id,
                        &name,
                        &args,
                        &tool_call_item,
                        ToolCallStatus::Completed,
                    );
                    event_recorder
                        .tool_output_started(&tool_call_item.call_item_id, Some(&call_id));
                    finish_successful_tool_output(
                        event_recorder,
                        &tool_call_item.call_item_id,
                        &call_id,
                        &optimized_result,
                    );
                }
                Err(e) => {
                    let category = e.category;
                    let failure_text = self.user_facing_tool_error_message(&name, &e);
                    error!(
                        agent = %agent_prefix,
                        tool = %name,
                        error = %e.message,
                        category = ?category,
                        retryable = e.retryable,
                        partial_state_possible = e.partial_state_possible,
                        "Tool execution failed"
                    );
                    halt_turn = apply_tool_failure_halt_policy(&mut runtime.state, category);
                    runtime.state.push_tool_error(
                        call_id.clone(),
                        &name,
                        e.to_json_value().to_string(),
                        is_gemini,
                    );
                    complete_tool_invocation(
                        runtime,
                        event_recorder,
                        &call_id,
                        &name,
                        &args,
                        &tool_call_item,
                        ToolCallStatus::Failed,
                    );
                    event_recorder
                        .tool_output_started(&tool_call_item.call_item_id, Some(&call_id));
                    event_recorder.tool_output_finished(
                        &tool_call_item.call_item_id,
                        Some(&call_id),
                        ToolCallStatus::Failed,
                        None,
                        &failure_text,
                        None,
                    );
                    if halt_turn {
                        break;
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

        let repeat_count = self.loop_detector.lock().get_call_count(&name);
        if repeat_count > 1 {
            let delay_ms = (LOOP_THROTTLE_BASE_MS * repeat_count as u64).min(LOOP_THROTTLE_MAX_MS);
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }

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
                if !self.quiet {
                    info!(agent = %agent_prefix, tool = %name, "Tool executed successfully");
                }

                let optimized_result = self.optimize_tool_result(&name, result);
                let tool_result = serde_json::to_string(&optimized_result)?;

                self.update_last_paths_from_args(&name, &args, &mut runtime.state);

                runtime.state.push_tool_result(
                    call.tool_call_id.clone(),
                    &name,
                    tool_result,
                    is_gemini,
                );
                complete_tool_invocation(
                    runtime,
                    event_recorder,
                    &call.tool_call_id,
                    &name,
                    &args,
                    &tool_call_item,
                    ToolCallStatus::Completed,
                );
                finish_successful_tool_output(
                    event_recorder,
                    &tool_call_item.call_item_id,
                    &call.tool_call_id,
                    &optimized_result,
                );
                Ok(false)
            }
            Err(e) => {
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
}
