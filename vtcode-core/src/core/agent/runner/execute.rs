use super::AgentRunner;
use super::constants::IDLE_TURN_LIMIT;
use super::continuation::{
    CompletionAssessment, ContinuationController, VerificationResult, is_review_like_task,
};
use super::helpers::detect_textual_exec_tool_call;
use crate::config::constants::tools;
use crate::config::models::{ModelId, Provider as ModelProvider};
use crate::config::types::{ReasoningEffortLevel, SystemPromptMode, VerbosityLevel};
use crate::core::agent::completion::{check_completion_indicators, check_for_response_loop};
use crate::core::agent::conversation::{
    build_conversation, build_messages_from_conversation, compose_system_instruction,
    conversation_from_messages,
};
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::session::controller::AgentSessionController;
use crate::core::agent::steering::SteeringMessage;
use crate::core::agent::task::{ContextItem, Task, TaskOutcome, TaskResults};
use crate::exec::events::HarnessEventKind;
use crate::llm::provider::{LLMRequest, Message, ToolCall};
use crate::llm::providers::gemini::wire::Part;
use crate::prompts::PromptContext;
use crate::prompts::system::compose_system_instruction_text;
use crate::utils::colors::style;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;
use tracing::warn;

fn record_terminal_turn_event(event_recorder: &mut ExecEventRecorder, outcome: &TaskOutcome) {
    if outcome.is_success() {
        event_recorder.turn_completed();
    } else {
        event_recorder.turn_failed(&outcome.description());
    }
}

fn tool_loop_limit_reached(loop_count: usize, max_tool_loops: usize) -> bool {
    max_tool_loops > 0 && loop_count >= max_tool_loops
}

fn summarize_verification_output(result: &serde_json::Value) -> String {
    result
        .get("output")
        .and_then(serde_json::Value::as_str)
        .or_else(|| result.get("stderr").and_then(serde_json::Value::as_str))
        .or_else(|| result.get("stdout").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| {
            let truncated = text
                .lines()
                .take(20)
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
            if truncated.len() < text.len() {
                format!("{truncated}\n...")
            } else {
                truncated
            }
        })
        .unwrap_or_default()
}

impl AgentRunner {
    async fn run_verification_commands(
        &self,
        commands: &[String],
        event_recorder: &mut ExecEventRecorder,
    ) -> Result<Vec<VerificationResult>> {
        let mut results = Vec::with_capacity(commands.len());
        for command in commands {
            let command_event = event_recorder.command_started(command);
            let payload = json!({
                "action": "run",
                "command": command,
                "workdir": self._workspace.display().to_string(),
                "yield_time_ms": 1000,
            });
            let result = self
                .tool_registry
                .execute_harness_unified_exec(payload)
                .await?;
            let exit_code = result
                .get("exit_code")
                .and_then(serde_json::Value::as_i64)
                .map(|value| value as i32);
            let success = exit_code.unwrap_or(0) == 0;
            let output = summarize_verification_output(&result);
            event_recorder.command_finished(
                &command_event,
                if success {
                    crate::exec::events::CommandExecutionStatus::Completed
                } else {
                    crate::exec::events::CommandExecutionStatus::Failed
                },
                exit_code,
                &output,
            );
            results.push(VerificationResult {
                command: command.clone(),
                success,
                exit_code,
                output,
            });
            if !success {
                break;
            }
        }
        Ok(results)
    }

    /// Execute a task with this agent
    pub async fn execute_task(
        &mut self,
        task: &Task,
        contexts: &[ContextItem],
    ) -> Result<TaskResults> {
        // Align harness context with runner session/task for structured telemetry
        self.tool_registry
            .set_harness_session(self.session_id.clone());
        self.tool_registry.set_harness_task(Some(task.id.clone()));

        // Ensure the tool registry is ready before entering the turn loop to avoid per-turn reinit.
        if let Err(err) = self.tool_registry.initialize_async().await {
            warn!(
                error = %err,
                "Tool registry initialization failed at task start"
            );
        }

        let result = {
            // Agent execution status
            let agent_prefix = format!("[{}]", self.agent_type);
            // OPTIMIZATION: Avoid cloning session_id repeatedly by using reference
            let mut event_recorder = ExecEventRecorder::new(
                self.session_id.clone(),
                self.event_sink.clone(),
                Some(self.thread_handle.clone()),
            );
            event_recorder.turn_started();
            self.runner_println(format_args!(
                "{} {}",
                agent_prefix,
                self.create_progress_message("thinking", None)
            ));

            self.runner_println(format_args!(
                "{} Executing {} task: {}",
                style("[AGENT]").magenta().bold().on_black(),
                self.agent_type,
                task.title
            ));

            let run_started_at = std::time::Instant::now();
            let is_simple_task = Self::is_simple_task(task, contexts);
            let tools = Arc::new(self.build_universal_tools().await?);

            let system_prompt = if is_simple_task {
                // One-time clone for simple tasks to override prompt mode (not per-turn)
                let mut config = self.config().clone();
                config.agent.system_prompt_mode = SystemPromptMode::Minimal;
                let mut prompt_context = PromptContext::from_workspace_tools(
                    self._workspace.as_path(),
                    tools.iter().map(|tool| tool.function_name().to_string()),
                );
                prompt_context.load_available_skills();
                compose_system_instruction_text(
                    self._workspace.as_path(),
                    Some(&config),
                    Some(&prompt_context),
                )
                .await
            } else {
                self.system_prompt.clone()
            };

            // Prepare conversation with task context
            let system_instruction =
                Arc::new(compose_system_instruction(&system_prompt, task, contexts));
            let mut conversation = conversation_from_messages(&self.bootstrap_messages);
            conversation.extend(build_conversation(task, contexts));

            // Maintain a mirrored conversation history for providers that expect
            // OpenAI/Anthropic style message roles.
            let conversation_messages =
                build_messages_from_conversation(&system_instruction, &conversation);

            // Track execution results
            // Determine loop guards via cached configuration
            let max_tool_loops = self.config().tools.max_tool_loops;
            let preserve_recent_turns = self.config().context.preserve_recent_turns;
            let max_context_tokens = self.config().context.max_context_tokens;

            let mut session_state = AgentSessionState::new(
                self.session_id.clone(),
                self.max_turns,
                max_tool_loops,
                max_context_tokens,
            );
            session_state.conversation = conversation;
            session_state.messages = conversation_messages;
            session_state.last_processed_message_idx = session_state.conversation.len();

            let mut controller = AgentSessionController::new(
                session_state,
                None, // Unified event sink can be added later
            );

            if let Err(err) = self.tool_registry.initialize_async().await {
                warn!(
                    error = %err,
                    "Tool registry initialization failed at task start"
                );
                controller
                    .state
                    .warnings
                    .push(format!("Tool registry init failed: {err}"));
            }

            let mut continuation_controller = ContinuationController::new(
                self._workspace.clone(),
                self.tool_registry.plan_mode_state(),
                self.config().agent.harness.continuation_policy.clone(),
                self.tool_registry
                    .current_full_auto_allowlist()
                    .await
                    .is_some(),
                self.tool_registry.is_plan_mode(),
                is_review_like_task(task),
            );
            continuation_controller.prepare(task).await?;

            // Agent execution loop uses max_turns for conversation flow
            for turn in 0..self.max_turns {
                // Check for steering messages before starting the turn
                if let Some(msg) = self.check_steering() {
                    match msg {
                        SteeringMessage::SteerStop => {
                            self.runner_println(format_args!(
                                "{} {}",
                                agent_prefix,
                                style("Stopped by steering signal.").red().bold()
                            ));
                            controller.state.outcome = TaskOutcome::Cancelled;
                            break;
                        }
                        SteeringMessage::Pause => {
                            self.runner_println(format_args!(
                                "{} {}",
                                agent_prefix,
                                style("Paused by steering signal. Waiting for Resume...")
                                    .yellow()
                                    .bold()
                            ));
                            // Wait for resume
                            loop {
                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                if let Some(SteeringMessage::Resume) = self.check_steering() {
                                    self.runner_println(format_args!(
                                        "{} {}",
                                        agent_prefix,
                                        style("Resumed by steering signal.").green().bold()
                                    ));
                                    break;
                                } else if let Some(SteeringMessage::SteerStop) =
                                    self.check_steering()
                                {
                                    controller.state.outcome = TaskOutcome::Cancelled;
                                    break;
                                }
                            }
                            if matches!(controller.state.outcome, TaskOutcome::Cancelled) {
                                break;
                            }
                        }
                        SteeringMessage::Resume => {} // Already running
                        SteeringMessage::FollowUpInput(input) => {
                            self.runner_println(format_args!(
                                "{} {}: {}",
                                agent_prefix,
                                style("Follow-up Input").cyan().bold(),
                                input
                            ));
                            controller.state.add_user_message(input);
                        }
                    }
                }

                // Check context utilization before each turn
                let utilization = controller.state.utilization();
                if utilization > 0.90 {
                    // At 90%+ utilization, warn and consider stopping
                    warn!("Context at {:.1}% - approaching limit", utilization * 100.0);
                    controller.state.warnings.push(format!(
                        "Token budget at {}% - approaching context limit",
                        (utilization * 100.0) as u32
                    ));
                    // Continue but warn - actual compaction handled by ContextOptimizer internally
                }

                if controller.state.is_completed {
                    controller.state.outcome = TaskOutcome::Success;
                    break;
                }

                controller.state.stats.turns_executed = turn + 1;
                let turn_started_at = std::time::Instant::now();
                let mut turn_recorded = false;

                self.runner_println(format_args!(
                    "{} {} is processing turn {}...",
                    agent_prefix,
                    style("(PROC)").cyan().bold(),
                    turn + 1
                ));

                let turn_model = self.get_selected_model();
                let turn_reasoning = if is_simple_task {
                    Some(ReasoningEffortLevel::Minimal)
                } else {
                    self.reasoning_effort
                };
                let turn_verbosity = if is_simple_task {
                    Some(VerbosityLevel::Low)
                } else {
                    self.verbosity
                };
                let max_tokens = if is_simple_task {
                    Some(800)
                } else {
                    Some(2000)
                };

                // Context compaction before the request
                self.summarize_conversation_if_needed(
                    &system_instruction,
                    &mut controller.state,
                    preserve_recent_turns,
                    utilization,
                );

                let parallel_tool_config = if self.model.len() < 20 {
                    None
                } else if self
                    .provider_client
                    .supports_parallel_tool_config(&turn_model)
                {
                    Some(Box::new(
                        crate::llm::provider::ParallelToolConfig::anthropic_optimized(),
                    ))
                } else {
                    None
                };

                let provider_kind = turn_model
                    .parse::<ModelId>()
                    .map(|m| m.provider())
                    .unwrap_or(ModelProvider::Gemini);

                // Optimize: Only rebuild messages for Gemini incrementally from last processed index
                if matches!(provider_kind, ModelProvider::Gemini)
                    && controller.state.conversation.len()
                        > controller.state.last_processed_message_idx
                {
                    // Incremental append instead of full rebuild
                    for content in &controller.state.conversation
                        [controller.state.last_processed_message_idx..]
                    {
                        let mut text = String::new();
                        for part in &content.parts {
                            if let Part::Text {
                                text: part_text, ..
                            } = part
                            {
                                if !text.is_empty() {
                                    text.push('\n');
                                }
                                text.push_str(part_text);
                            }
                        }
                        let message = match content.role.as_str() {
                            "model" => Message::assistant(text),
                            _ => Message::user(text),
                        };
                        controller.state.messages.push(message);
                    }
                    controller.state.last_processed_message_idx =
                        controller.state.conversation.len();
                }

                let reasoning_effort =
                    if self.provider_client.supports_reasoning_effort(&turn_model) {
                        turn_reasoning
                    } else {
                        None
                    };
                let temperature = if reasoning_effort.is_some()
                    && matches!(
                        provider_kind,
                        ModelProvider::Anthropic | ModelProvider::Minimax
                    ) {
                    None
                } else {
                    Some(0.7)
                };
                let request = LLMRequest {
                    messages: controller.state.messages.clone(),
                    system_prompt: Some(Arc::clone(&system_instruction)),
                    tools: Some(Arc::clone(&tools)),
                    model: turn_model.clone(),
                    max_tokens,
                    temperature,
                    stream: self.provider_client.supports_streaming(),
                    parallel_tool_config,
                    reasoning_effort,
                    verbosity: turn_verbosity,
                    ..Default::default()
                };

                let mut steering_captured = self.steering_receiver.lock().take();

                let (response, _content, _reasoning) = controller
                    .run_turn(
                        &mut self.provider_client,
                        request,
                        &mut steering_captured,
                        Some(std::time::Duration::from_secs(60)), // Standard timeout
                    )
                    .await?;

                // Put steering back for next turn
                if let Some(rx) = steering_captured {
                    *self.steering_receiver.lock() = Some(rx);
                }

                self.runner_println(format_args!(
                    "{} {} {} received response, processing...",
                    agent_prefix,
                    self.agent_type,
                    style("(RECV)").green().bold()
                ));

                if let Some(reasoning) = response.reasoning.as_ref() {
                    event_recorder.reasoning(reasoning);
                }

                self.warn_on_empty_response(
                    &agent_prefix,
                    response.content.as_deref().unwrap_or(""),
                    response
                        .tool_calls
                        .as_ref()
                        .is_some_and(|tc| !tc.is_empty()),
                );

                let response_text = response.content_string();
                if !response_text.trim().is_empty() {
                    event_recorder.agent_message(&response_text);
                    self.emit_final_assistant_message(&self.agent_type, &response_text);
                }

                let mut effective_tool_calls = response.tool_calls.clone();
                let mut forced_continuation = false;

                // HP-4: Detect textual commands in empty/near-empty responses if no structured tool calls
                if effective_tool_calls.is_none()
                    && response.content_text().len() < 150
                    && let Some(args_value) = detect_textual_exec_tool_call(response.content_text())
                {
                    let tc = ToolCall::function(
                        format!("call_text_{}", turn),
                        tools::UNIFIED_EXEC.to_string(),
                        args_value.to_string(),
                    );
                    effective_tool_calls = Some(vec![tc]);
                }

                let is_gemini = matches!(provider_kind, ModelProvider::Gemini);

                if !controller.state.is_completed
                    && effective_tool_calls
                        .as_ref()
                        .is_none_or(|tool_calls| tool_calls.is_empty())
                    && !response.content_text().is_empty()
                {
                    if check_for_response_loop(response.content_text(), &mut controller.state) {
                        self.runner_println(format_args!(
                            "[{}] {}",
                            self.agent_type,
                            style(
                                "Repetitive assistant response detected. Breaking potential loop."
                            )
                            .red()
                            .bold()
                        ));
                        controller.state.outcome = TaskOutcome::LoopDetected;
                        controller
                            .state
                            .record_turn(&turn_started_at, &mut turn_recorded);
                        break;
                    }

                    if check_completion_indicators(response.content_text()) {
                        self.runner_println(format_args!(
                            "[{}] {}",
                            self.agent_type,
                            style("Completion indicator detected.").green().bold()
                        ));
                        match continuation_controller
                            .assess_completion(task, &controller.state)
                            .await?
                        {
                            CompletionAssessment::Accept => {
                                controller.state.is_completed = true;
                                controller.state.outcome = TaskOutcome::Success;
                                break;
                            }
                            CompletionAssessment::SkipAccept { reason } => {
                                event_recorder.harness_event(
                                    HarnessEventKind::ContinuationSkipped,
                                    Some(reason),
                                    None,
                                    None,
                                );
                                controller.state.is_completed = true;
                                controller.state.outcome = TaskOutcome::Success;
                                break;
                            }
                            CompletionAssessment::Continue { reason, prompt } => {
                                event_recorder.harness_event(
                                    HarnessEventKind::ContinuationStarted,
                                    Some(reason),
                                    None,
                                    None,
                                );
                                controller.state.add_user_message(prompt);
                                forced_continuation = true;
                            }
                            CompletionAssessment::Verify { commands } => {
                                event_recorder.harness_event(
                                    HarnessEventKind::VerificationStarted,
                                    Some(format!("Running verification: {}", commands.join(", "))),
                                    commands.first().cloned(),
                                    None,
                                );
                                let verification_results = self
                                    .run_verification_commands(&commands, &mut event_recorder)
                                    .await?;
                                if let Some(failure) =
                                    verification_results.iter().find(|result| !result.success)
                                {
                                    event_recorder.harness_event(
                                        HarnessEventKind::VerificationFailed,
                                        Some(format!(
                                            "{}{}",
                                            match failure.exit_code {
                                                Some(code) => format!(
                                                    "Verification failed: {} (exit code {}).",
                                                    failure.command, code
                                                ),
                                                None => format!(
                                                    "Verification failed: {}.",
                                                    failure.command
                                                ),
                                            },
                                            if failure.output.trim().is_empty() {
                                                String::new()
                                            } else {
                                                format!("\n{}", failure.output.trim())
                                            }
                                        )),
                                        Some(failure.command.clone()),
                                        failure.exit_code,
                                    );
                                } else {
                                    event_recorder.harness_event(
                                        HarnessEventKind::VerificationPassed,
                                        Some(format!(
                                            "Verification passed: {}",
                                            commands.join(", ")
                                        )),
                                        commands.last().cloned(),
                                        Some(0),
                                    );
                                }

                                match continuation_controller
                                    .after_verification(&verification_results)
                                    .await?
                                {
                                    CompletionAssessment::Accept
                                    | CompletionAssessment::SkipAccept { .. } => {
                                        controller.state.is_completed = true;
                                        controller.state.outcome = TaskOutcome::Success;
                                        break;
                                    }
                                    CompletionAssessment::Continue { reason, prompt } => {
                                        event_recorder.harness_event(
                                            HarnessEventKind::ContinuationStarted,
                                            Some(reason),
                                            None,
                                            None,
                                        );
                                        controller.state.add_user_message(prompt);
                                        forced_continuation = true;
                                    }
                                    CompletionAssessment::Verify { .. } => {}
                                }
                            }
                        }
                    }
                }

                if let Some(tool_calls) = effective_tool_calls
                    .as_ref()
                    .filter(|tc| !tc.is_empty())
                    .cloned()
                {
                    self.handle_tool_calls(
                        tool_calls,
                        &mut controller.state,
                        &mut event_recorder,
                        &agent_prefix,
                        is_gemini,
                    )
                    .await?;
                }

                let had_effective_shell_tool_call =
                    effective_tool_calls.as_ref().is_some_and(|calls| {
                        calls.iter().any(|tc| {
                            tc.function.as_ref().map(|f| f.name.as_str())
                                == Some(tools::UNIFIED_EXEC)
                        })
                    });
                let had_tool_call = response
                    .tool_calls
                    .as_ref()
                    .is_some_and(|tc| !tc.is_empty())
                    || had_effective_shell_tool_call;
                if had_tool_call {
                    let loops = controller.state.register_tool_loop();
                    if tool_loop_limit_reached(loops, controller.state.constraints.max_tool_loops) {
                        let warning_message = format!(
                            "Reached tool-call limit of {} iterations; pausing autonomous loop",
                            controller.state.constraints.max_tool_loops
                        );
                        self.record_warning(
                            &agent_prefix,
                            &mut controller.state,
                            &mut event_recorder,
                            warning_message,
                        );
                        controller.state.mark_tool_loop_limit_hit();
                        controller
                            .state
                            .record_turn(&turn_started_at, &mut turn_recorded);
                        break;
                    }
                    controller.state.consecutive_idle_turns = 0;
                } else {
                    controller.state.reset_tool_loop_guard();
                    if forced_continuation {
                        controller.state.consecutive_idle_turns = 0;
                    } else if !controller.state.is_completed {
                        controller.state.consecutive_idle_turns =
                            controller.state.consecutive_idle_turns.saturating_add(1);
                        if controller.state.consecutive_idle_turns >= IDLE_TURN_LIMIT {
                            let warning_message = format!(
                                "No tool calls or completion for {} consecutive turns; halting to avoid idle loop",
                                controller.state.consecutive_idle_turns
                            );
                            self.record_warning(
                                &agent_prefix,
                                &mut controller.state,
                                &mut event_recorder,
                                warning_message,
                            );
                            controller.state.outcome = TaskOutcome::StoppedNoAction;
                            controller
                                .state
                                .record_turn(&turn_started_at, &mut turn_recorded);
                            break;
                        }
                    }
                }

                let should_continue = forced_continuation
                    || had_tool_call
                    || (!controller.state.is_completed && (turn + 1) < self.max_turns);

                // Record turn duration for the successfully completed turn
                controller
                    .state
                    .record_turn(&turn_started_at, &mut turn_recorded);

                if !should_continue {
                    if controller.state.is_completed {
                        controller.state.outcome = TaskOutcome::Success;
                    } else if (turn + 1) >= self.max_turns {
                        controller.state.outcome =
                            TaskOutcome::turn_limit_reached(self.max_turns, turn + 1);
                    } else {
                        controller.state.outcome = TaskOutcome::StoppedNoAction;
                    }
                    break;
                }
            }

            controller.state.finalize_outcome(self.max_turns);

            let total_duration_ms = run_started_at.elapsed().as_millis();

            // Agent execution completed
            self.runner_println(format_args!("{} Done", agent_prefix));

            // Generate meaningful summary based on agent actions
            let average_turn_duration_ms = if controller.state.turn_count > 0 {
                Some(controller.state.turn_total_ms as f64 / controller.state.turn_count as f64)
            } else {
                None
            };

            let max_turn_duration_ms = if controller.state.turn_count > 0 {
                Some(controller.state.turn_max_ms)
            } else {
                None
            };

            let outcome = controller.state.outcome.clone(); // Clone to avoid moving
            self.thread_handle
                .replace_messages(controller.state.messages.clone());
            let summary = self.generate_task_summary(
                task,
                &controller.state.modified_files,
                &controller.state.executed_commands,
                &controller.state.warnings,
                &controller.state.messages,
                controller.state.stats.turns_executed,
                controller.state.max_tool_loop_streak,
                max_tool_loops,
                outcome,
                total_duration_ms,
                average_turn_duration_ms,
                max_turn_duration_ms,
            );

            if !summary.trim().is_empty() {
                // Record summary as agent message for event stream
                event_recorder.agent_message(&summary);
                // Also display summary prominently for immediate visibility in TUI transcript
                self.runner_println(format_args!(
                    "\n{} Agent Task Summary\n{}",
                    style("[Task]").cyan().bold(),
                    summary
                ));
            }

            record_terminal_turn_event(&mut event_recorder, &controller.state.outcome);
            let thread_events = event_recorder.into_events();

            // Return task results
            Ok(controller
                .state
                .into_results(summary, thread_events, total_duration_ms))
        };

        self.tool_registry.set_harness_task(None);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{record_terminal_turn_event, tool_loop_limit_reached};
    use crate::core::agent::events::ExecEventRecorder;
    use crate::core::agent::task::TaskOutcome;
    use crate::exec::events::ThreadEvent;

    #[test]
    fn failed_outcome_emits_only_turn_failed() {
        let mut recorder = ExecEventRecorder::new("thread", None, None);
        recorder.turn_started();

        record_terminal_turn_event(
            &mut recorder,
            &TaskOutcome::Failed {
                reason: "boom".to_string(),
            },
        );

        let events = recorder.into_events();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ThreadEvent::TurnFailed(_)))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ThreadEvent::TurnCompleted(_)))
                .count(),
            0
        );
    }

    #[test]
    fn successful_outcome_emits_only_turn_completed() {
        let mut recorder = ExecEventRecorder::new("thread", None, None);
        recorder.turn_started();

        record_terminal_turn_event(&mut recorder, &TaskOutcome::Success);

        let events = recorder.into_events();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ThreadEvent::TurnCompleted(_)))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ThreadEvent::TurnFailed(_)))
                .count(),
            0
        );
    }

    #[test]
    fn disabled_tool_loop_limit_never_trips() {
        assert!(!tool_loop_limit_reached(1, 0));
        assert!(!tool_loop_limit_reached(32, 0));
    }
}
