use super::AgentRunner;
use super::constants::{LOOP_THROTTLE_BASE_MS, LOOP_THROTTLE_MAX_MS};
use super::types::ToolFailureContext;
use crate::config::constants::tools;
use crate::core::agent::events::{
    ExecEventRecorder, tool_invocation_completed_event, tool_output_payload_from_value,
};
use crate::core::agent::runtime::{AgentRuntime, RuntimeControl};
use crate::exec::events::{ItemCompletedEvent, ThreadEvent, ThreadItemDetails, ToolCallStatus};
use crate::llm::provider::ToolCall;
use anyhow::{Result, anyhow};
use tokio::time::Duration;
use tracing::{error, info, warn};

struct ToolCallItemRef {
    call_item_id: String,
    synthetic_invocation: bool,
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

impl AgentRunner {
    /// Execute multiple tool calls in parallel. Only safe for read-only operations.
    pub(super) async fn execute_parallel_tool_calls(
        &self,
        tool_calls: Vec<ToolCall>,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
    ) -> Result<()> {
        use futures::future::join_all;

        let mut prepared_calls = Vec::with_capacity(tool_calls.len());
        for call in tool_calls {
            let Some(func) = call.function.as_ref() else {
                continue;
            };
            let requested_name = func.name.clone();
            let args = match call.parsed_arguments() {
                Ok(args) => args,
                Err(err) => {
                    let error_msg =
                        format!("Invalid arguments for tool '{}': {}", requested_name, err);
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
                    continue;
                }
            };
            let args = self.normalize_tool_args(&requested_name, &args, &mut runtime.state);
            let name = match self.validate_and_normalize_tool_name(&requested_name, &args) {
                Ok(name) => name,
                Err(err) => {
                    let error_msg =
                        format!("Invalid arguments for tool '{}': {}", requested_name, err);
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
                    continue;
                }
            };
            if self.check_for_loop(&name, &args, &mut runtime.state) {
                return Ok(());
            }
            prepared_calls.push((call, name, args));
        }

        let total_calls = prepared_calls.len();
        info!(
            agent = %self.agent_type,
            count = total_calls,
            "Executing parallel tool calls"
        );

        let mut futures = Vec::with_capacity(prepared_calls.len());
        for (call, name, args) in prepared_calls {
            let call_id = call.id.clone();

            if !self.is_valid_tool(&name).await {
                let detail = format!("Tool execution denied: {name}");
                if !self.quiet {
                    warn!(agent = %agent_prefix, tool = %name, message = %detail);
                }
                runtime.state.warnings.push(detail.clone());
                runtime
                    .state
                    .push_tool_error(call_id.clone(), &name, detail.clone(), is_gemini);
                reject_tool_call(
                    runtime,
                    event_recorder,
                    &name,
                    Some(&args),
                    &call_id,
                    &detail,
                );
                continue;
            }

            let tool_call_item =
                resolve_tool_call_item(runtime, event_recorder, &name, &args, &call_id);
            let runner = self;
            let args_clone = args.clone();
            futures.push(async move {
                let result = runner
                    .execute_tool_internal(&name, &args_clone)
                    .await
                    .map_err(|e| anyhow!("Tool '{}' failed: {}", name, e));
                (name, call_id, args_clone, tool_call_item, result)
            });
        }

        let results = join_all(futures).await;
        let mut halt_turn = false;
        for (name, call_id, args, tool_call_item, result) in results {
            match result {
                Ok(result) => {
                    if !self.quiet {
                        info!(agent = %agent_prefix, tool = %name, "Tool executed successfully");
                    }

                    let optimized_result = self.optimize_tool_result(&name, result).await;
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
                    let error_msg = format!("Error executing {}: {}", name, e);
                    error!(agent = %agent_prefix, tool = %name, error = %e, "Tool execution failed");
                    let err_lower = error_msg.to_lowercase();
                    if err_lower.contains("rate limit") {
                        runtime.state.warnings.push(
                            "Tool was rate limited; halting further tool calls this turn.".into(),
                        );
                        runtime.state.mark_tool_loop_limit_hit();
                        halt_turn = true;
                    } else if err_lower.contains("denied by policy")
                        || err_lower.contains("not permitted while full-auto")
                    {
                        runtime.state.warnings.push(
                            "Tool denied by policy; halting further tool calls this turn.".into(),
                        );
                        halt_turn = true;
                    }
                    runtime
                        .state
                        .push_tool_error(call_id.clone(), &name, error_msg, is_gemini);
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
                        &e.to_string(),
                        None,
                    );
                    if halt_turn {
                        break;
                    }
                }
            }
        }
        if halt_turn {
            tokio::time::sleep(Duration::from_millis(250)).await;
            return Ok(());
        }
        Ok(())
    }

    /// Execute multiple tool calls sequentially.
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

            let requested_name = match call.function.as_ref() {
                Some(func) => func.name.clone(),
                None => continue,
            };
            let args = match call.parsed_arguments() {
                Ok(args) => args,
                Err(err) => {
                    let error_msg =
                        format!("Invalid arguments for tool '{}': {}", requested_name, err);
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
                    continue;
                }
            };
            let args = self.normalize_tool_args(&requested_name, &args, &mut runtime.state);
            let name = match self.validate_and_normalize_tool_name(&requested_name, &args) {
                Ok(name) => name,
                Err(err) => {
                    let error_msg =
                        format!("Invalid arguments for tool '{}': {}", requested_name, err);
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
                    continue;
                }
            };

            if self.check_for_loop(&name, &args, &mut runtime.state) {
                break;
            }

            if !self.quiet {
                info!(
                    agent = %self.agent_type,
                    tool = %name,
                    "Calling tool"
                );
            }

            if !self.is_valid_tool(&name).await {
                let detail = format!("Tool execution denied: {name}");
                if !self.quiet {
                    warn!(agent = %agent_prefix, tool = %name, message = %detail);
                }
                runtime.state.warnings.push(detail.clone());
                runtime
                    .state
                    .push_tool_error(call.id.clone(), &name, detail.clone(), is_gemini);
                reject_tool_call(
                    runtime,
                    event_recorder,
                    &name,
                    Some(&args),
                    &call.id,
                    &detail,
                );
                continue;
            }

            let tool_call_item =
                resolve_tool_call_item(runtime, event_recorder, &name, &args, &call.id);
            event_recorder.tool_output_started(&tool_call_item.call_item_id, Some(&call.id));

            let repeat_count = self.loop_detector.lock().get_call_count(&name);
            if repeat_count > 1 {
                let delay_ms =
                    (LOOP_THROTTLE_BASE_MS * repeat_count as u64).min(LOOP_THROTTLE_MAX_MS);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }

            // Use internal execution since is_valid_tool was already called above
            match self.execute_tool_internal(&name, &args).await {
                Ok(result) => {
                    if !self.quiet {
                        info!(agent = %agent_prefix, tool = %name, "Tool executed successfully");
                    }

                    let optimized_result = self.optimize_tool_result(&name, result).await;
                    let tool_result = serde_json::to_string(&optimized_result)?;

                    self.update_last_paths_from_args(&name, &args, &mut runtime.state);

                    runtime
                        .state
                        .push_tool_result(call.id.clone(), &name, tool_result, is_gemini);
                    complete_tool_invocation(
                        runtime,
                        event_recorder,
                        &call.id,
                        &name,
                        &args,
                        &tool_call_item,
                        ToolCallStatus::Completed,
                    );
                    finish_successful_tool_output(
                        event_recorder,
                        &tool_call_item.call_item_id,
                        &call.id,
                        &optimized_result,
                    );

                    if name == tools::WRITE_FILE
                        && let Some(filepath) = args.get("path").and_then(|p| p.as_str())
                    {
                        runtime.state.modified_files.push(filepath.to_owned());
                        event_recorder.file_change_completed(filepath);
                    }
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    let err_lower = err_msg.to_lowercase();
                    if err_lower.contains("rate limit") {
                        runtime.state.warnings.push(
                            "Tool was rate limited; halting further tool calls this turn.".into(),
                        );
                        runtime.state.mark_tool_loop_limit_hit();
                        complete_tool_invocation(
                            runtime,
                            event_recorder,
                            &call.id,
                            &name,
                            &args,
                            &tool_call_item,
                            ToolCallStatus::Failed,
                        );
                        let mut failure_ctx = ToolFailureContext {
                            agent_prefix,
                            session_state: &mut runtime.state,
                            event_recorder,
                            tool_call_id: &call.id,
                            call_item_id: Some(tool_call_item.call_item_id.as_str()),
                            is_gemini,
                        };
                        self.record_tool_failure(
                            &mut failure_ctx,
                            &name,
                            &e,
                            Some(call.id.as_str()),
                        );
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        break;
                    } else if err_lower.contains("denied by policy")
                        || err_lower.contains("not permitted while full-auto")
                    {
                        runtime.state.warnings.push(
                            "Tool denied by policy; halting further tool calls this turn.".into(),
                        );
                        complete_tool_invocation(
                            runtime,
                            event_recorder,
                            &call.id,
                            &name,
                            &args,
                            &tool_call_item,
                            ToolCallStatus::Failed,
                        );
                        let mut failure_ctx = ToolFailureContext {
                            agent_prefix,
                            session_state: &mut runtime.state,
                            event_recorder,
                            tool_call_id: &call.id,
                            call_item_id: Some(tool_call_item.call_item_id.as_str()),
                            is_gemini,
                        };
                        self.record_tool_failure(
                            &mut failure_ctx,
                            &name,
                            &e,
                            Some(call.id.as_str()),
                        );
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        break;
                    } else {
                        complete_tool_invocation(
                            runtime,
                            event_recorder,
                            &call.id,
                            &name,
                            &args,
                            &tool_call_item,
                            ToolCallStatus::Failed,
                        );
                        let mut failure_ctx = ToolFailureContext {
                            agent_prefix,
                            session_state: &mut runtime.state,
                            event_recorder,
                            tool_call_id: &call.id,
                            call_item_id: Some(tool_call_item.call_item_id.as_str()),
                            is_gemini,
                        };
                        self.record_tool_failure(
                            &mut failure_ctx,
                            &name,
                            &e,
                            Some(call.id.as_str()),
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
