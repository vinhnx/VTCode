use super::AgentRunner;
use super::constants::{IDLE_TURN_LIMIT, ROLE_MODEL};
use super::helpers::detect_textual_run_pty_cmd;
use crate::config::constants::tools;
use crate::config::models::{ModelId, Provider as ModelProvider};
use crate::config::types::{ReasoningEffortLevel, SystemPromptMode, VerbosityLevel};
use crate::core::agent::completion::{check_completion_indicators, check_for_response_loop};
use crate::core::agent::conversation::{
    build_conversation, build_messages_from_conversation, compose_system_instruction,
};
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::session::controller::AgentSessionController;
use crate::core::agent::steering::SteeringMessage;
use crate::core::agent::task::{ContextItem, Task, TaskOutcome, TaskResults};
use crate::gemini::{Content, Part};
use crate::llm::provider::{LLMRequest, Message, ToolCall};
use crate::prompts::system::compose_system_instruction_text;
use crate::utils::colors::style;
use anyhow::Result;
use parking_lot::Mutex;
use std::sync::Arc;
use tracing::warn;

impl AgentRunner {
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
            let mut event_recorder =
                ExecEventRecorder::new(self.session_id.clone(), self.event_sink.clone());
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

            let system_prompt = if is_simple_task {
                // One-time clone for simple tasks to override prompt mode (not per-turn)
                let mut config = self.config().clone();
                config.agent.system_prompt_mode = SystemPromptMode::Minimal;
                compose_system_instruction_text(self._workspace.as_path(), Some(&config), None)
                    .await
            } else {
                self.system_prompt.clone()
            };

            // Prepare conversation with task context
            let system_instruction =
                Arc::new(compose_system_instruction(&system_prompt, task, contexts));
            let conversation = build_conversation(task, contexts);

            // Build available tools for this agent
            let tools = Arc::new(self.build_universal_tools().await?);

            // Maintain a mirrored conversation history for providers that expect
            // OpenAI/Anthropic style message roles.
            let conversation_messages =
                build_messages_from_conversation(&system_instruction, &conversation);

            // Track execution results
            // Determine loop guards via cached configuration
            let max_tool_loops = self.config().tools.max_tool_loops.max(1);
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

            let mut controller = AgentSessionController::new(
                session_state,
                None, // Unified event sink can be added later
                Some(
                    self.event_sink
                        .clone()
                        .unwrap_or_else(|| Arc::new(Mutex::new(Box::new(|_| {})))),
                ),
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

            // Agent execution loop uses max_turns for conversation flow
            for turn in 0..self.max_turns {
                // Check for steering messages before starting the turn
                if let Some(msg) = self.check_steering() {
                    match msg {
                        SteeringMessage::Stop => {
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
                                } else if let Some(SteeringMessage::Stop) = self.check_steering() {
                                    controller.state.outcome = TaskOutcome::Cancelled;
                                    break;
                                }
                            }
                            if matches!(controller.state.outcome, TaskOutcome::Cancelled) {
                                break;
                            }
                        }
                        SteeringMessage::Resume => {} // Already running
                        SteeringMessage::InjectInput(input) => {
                            self.runner_println(format_args!(
                                "{} {}: {}",
                                agent_prefix,
                                style("Injected Input").cyan().bold(),
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
                    response.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty()),
                );

                if !response.content_text().trim().is_empty() {
                    event_recorder.agent_message(&response.content_string());
                    Self::print_compact_response(
                        &self.agent_type,
                        &response.content_string(),
                        self.quiet,
                    );
                    self.runner_println(format_args!(
                        "{} {} {}",
                        agent_prefix,
                        style("(ASSISTANT)").green().bold(),
                        response.content_text().trim()
                    ));
                }

                let mut effective_tool_calls = response.tool_calls.clone();
                
                // HP-4: Detect textual commands in empty/near-empty responses if no structured tool calls
                if effective_tool_calls.is_none()
                    && response.content_text().len() < 150
                    && let Some(args_value) = detect_textual_run_pty_cmd(response.content_text())
                {
                    let tc = ToolCall::function(
                        format!("call_text_{}", turn), 
                        "run_pty_command".to_string(), 
                        args_value.to_string()
                    );
                    effective_tool_calls = Some(vec![tc]);
                }

                let is_gemini = matches!(provider_kind, ModelProvider::Gemini);

                if !controller.state.is_completed && !response.content_text().is_empty() {
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
                        controller.state.is_completed = true;
                        controller.state.outcome = TaskOutcome::Success;
                        break;
                    }
                }

                if let Some(tool_calls) = effective_tool_calls.as_ref().filter(|tc| !tc.is_empty()).cloned() {
                    self.handle_tool_calls(
                        tool_calls,
                        &mut controller.state,
                        &mut event_recorder,
                        &agent_prefix,
                        is_gemini,
                    )
                    .await?;
                }

                let had_tool_call = response.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty())
                    || (effective_tool_calls.as_ref().is_some() && effective_tool_calls.as_ref().unwrap().iter().any(|tc| tc.function.as_ref().map(|f| f.name.as_str()) == Some("run_pty_command")));
                if had_tool_call {
                    let loops = controller.state.register_tool_loop();
                    if loops >= controller.state.constraints.max_tool_loops {
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
                    if !controller.state.is_completed {
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

                let should_continue = had_tool_call
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
            let summary = self.generate_task_summary(
                task,
                &controller.state.modified_files,
                &controller.state.executed_commands,
                &controller.state.warnings,
                &controller.state.conversation,
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

            if !controller.state.outcome.is_success() {
                event_recorder.turn_failed(&controller.state.outcome.description());
            }

            event_recorder.turn_completed();
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
