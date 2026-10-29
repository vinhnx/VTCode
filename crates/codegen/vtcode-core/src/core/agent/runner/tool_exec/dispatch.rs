//! Parallel/sequential dispatch and batch orchestration for runner tool calls.

use super::super::AgentRunner;
use super::super::tool_access::PreparedToolExecution;
use super::super::tool_execution_guard::ToolExecutionGuard;
use super::super::tool_rejection::emit_failed_tool_outputs_for_completed_invocations;
use super::super::tool_types::{PreparedRunnerToolCall, RunnerCallAdmission};
use super::admission::align_prepared_batches;
use super::outcome::{
    apply_tool_failure_halt_policy, apply_tool_success, complete_tool_invocation, record_circuit_transition,
    record_tool_execution_observation, resolve_tool_call_item, snapshot_circuit_diagnostics,
};
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::harness_kernel::{FallbackRecommendation, PreparedToolBatchKind};
use crate::core::agent::runtime::{AgentRuntime, RuntimeControl};
use crate::exec::events::ToolCallStatus;
use crate::llm::provider::ToolCall;
use crate::tools::registry::{ToolErrorType, ToolExecutionError};
use anyhow::Result;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{info, warn};
use vtcode_commons::ErrorCategory;

impl AgentRunner {
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
        let fallback_map: std::collections::HashMap<String, FallbackRecommendation> = prepared_calls
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
            let PreparedRunnerToolCall { tool_call_id, prepared } = call;
            let name = prepared.canonical_name.clone();
            let args = prepared.effective_args.clone();
            let tool_call_item = resolve_tool_call_item(runtime, event_recorder, &name, &args, &tool_call_id);
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
                                PreparedToolExecution {
                                    result: Err(ToolExecutionError::new(
                                        "parallel_tool_semaphore",
                                        ToolErrorType::ExecutionError,
                                        "parallel tool semaphore closed",
                                    )),
                                    attempts: 1,
                                    total_duration: Duration::ZERO,
                                    last_error_category: Some(ErrorCategory::ExecutionError),
                                },
                                circuit_before,
                            );
                        }
                    }
                } else {
                    None
                };
                runner.throttle_repeated_tool(&name).await;
                let execution = runner.execute_prepared_tool_internal(&prepared).await;
                (name, tool_call_id, args, tool_call_item, execution, circuit_before)
            });
        }

        let results = join_all(futures).await;
        let mut halt_turn = false;
        for (name, call_id, args, tool_call_item, execution, circuit_before) in results {
            record_tool_execution_observation(&mut runtime.state, &name, &execution);
            record_circuit_transition(self, &runtime.state.error_recovery, &name, circuit_before);
            match execution.result {
                Ok(result) => {
                    event_recorder.tool_output_started(&tool_call_item.call_item_id, Some(&call_id));
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
                    // Try fallback first before declaring failure.
                    let fallback_outcome = if let Some(fallback) = fallback_map.get(&call_id) {
                        self.try_execute_fallback(fallback, runtime, agent_prefix, &name).await
                    } else {
                        None
                    };

                    if let Some((res, f_name, f_args)) = fallback_outcome {
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
                        let should_halt = apply_tool_failure_halt_policy(&mut runtime.state, category);
                        complete_tool_invocation(
                            runtime,
                            event_recorder,
                            &call_id,
                            &name,
                            &args,
                            &tool_call_item,
                            ToolCallStatus::Failed,
                            None,
                        );
                        let mut failure_ctx = super::super::tool_dispatch_common::build_tool_failure_context(
                            agent_prefix,
                            &mut runtime.state,
                            event_recorder,
                            &call_id,
                            Some(tool_call_item.call_item_id.as_str()),
                            is_gemini,
                        );
                        self.record_tool_failure(&mut failure_ctx, &name, &e, Some(call_id.as_str()));
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

        let tool_call_item = resolve_tool_call_item(runtime, event_recorder, &name, &args, &call.tool_call_id);
        event_recorder.tool_output_started(&tool_call_item.call_item_id, Some(&call.tool_call_id));
        let circuit_before = snapshot_circuit_diagnostics(self, &name);

        self.throttle_repeated_tool(&name).await;

        let mut guard = ToolExecutionGuard::new(&name, &call.tool_call_id, runtime.state.error_recovery.clone());
        let tool_execution = self.execute_prepared_tool_internal(&call.prepared).await;
        record_tool_execution_observation(&mut runtime.state, &name, &tool_execution);
        match tool_execution.result {
            Ok(result) => {
                guard.mark_completed();
                record_circuit_transition(self, &runtime.state.error_recovery, &name, circuit_before);
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
                // Try fallback first before declaring failure.
                let fallback_outcome = if let Some(fallback) = &call.prepared.fallback_recommendation {
                    self.try_execute_fallback(fallback, runtime, agent_prefix, &name).await
                } else {
                    None
                };

                if let Some((res, f_name, f_args)) = fallback_outcome {
                    guard.mark_completed();
                    record_circuit_transition(self, &runtime.state.error_recovery, &f_name, circuit_before);
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
                    record_circuit_transition(self, &runtime.state.error_recovery, &name, circuit_before);
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
                        None,
                    );
                    let mut failure_ctx = super::super::tool_dispatch_common::build_tool_failure_context(
                        agent_prefix,
                        &mut runtime.state,
                        event_recorder,
                        &call.tool_call_id,
                        Some(tool_call_item.call_item_id.as_str()),
                        is_gemini,
                    );
                    self.record_tool_failure(&mut failure_ctx, &name, &e, Some(call.tool_call_id.as_str()));

                    Ok(should_halt)
                }
            }
        }
    }

    pub(in super::super) async fn execute_tool_call_batches(
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
            if matches!(runtime.poll_tool_control().await, RuntimeControl::StopRequested) {
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
                        .admit_runner_tool_call(next_call, runtime, event_recorder, agent_prefix, is_gemini)
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
            let planned_batches = align_prepared_batches(
                batch_calls,
                allow_parallel_batch,
                self.config().agent.harness.max_parallel_tool_calls,
            );
            let configured_limit = self.config().agent.harness.max_parallel_tool_calls;
            let group_count = planned_batches.len();
            let parallel_group_count = planned_batches
                .iter()
                .filter(|batch| matches!(batch.kind, PreparedToolBatchKind::ParallelReadonly))
                .count();
            let admitted_parallel_calls = planned_batches
                .iter()
                .filter(|batch| matches!(batch.kind, PreparedToolBatchKind::ParallelReadonly))
                .map(|batch| batch.calls.len())
                .sum::<usize>();
            let maximum_group_size = planned_batches.iter().map(|batch| batch.calls.len()).max().unwrap_or(0);
            tracing::debug!(
                target: "vtcode.turn.metrics",
                metric = "tool_dispatch_groups",
                configured_limit,
                admitted_parallel_calls,
                group_count,
                parallel_group_count,
                maximum_group_size,
                "turn metric"
            );
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
}
