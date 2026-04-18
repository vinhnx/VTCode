use super::AgentRunner;
use super::constants::IDLE_TURN_LIMIT;
use super::continuation::{
    CompletionAssessment, ContinuationController, VerificationResult, is_review_like_task,
};
use super::helpers::detect_textual_exec_tool_call;
use super::orchestration::EvaluatorGateOutcome;
use crate::config::build_openai_prompt_cache_key;
use crate::config::constants::tools;
use crate::config::models::{ModelId, Provider as ModelProvider};
use crate::config::types::{ReasoningEffortLevel, SystemPromptMode, VerbosityLevel};
use crate::core::agent::blocked_handoff::write_blocked_handoff;
use crate::core::agent::completion::{check_completion_indicators, check_for_response_loop};
use crate::core::agent::conversation::{
    build_conversation, build_messages_from_conversation, conversation_from_messages,
};
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::harness_artifacts::existing_harness_artifact_paths;
use crate::core::agent::harness_kernel::{HarnessRequestPlanInput, build_harness_request_plan};
use crate::core::agent::runtime::{AgentRuntime, RuntimeControl};
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::task::{ContextItem, Task, TaskOutcome, TaskResults};
use crate::exec::events::HarnessEventKind;
use crate::llm::model_resolver::ModelResolver;
use crate::llm::provider::{
    FinishReason, Message, ToolCall, prepare_responses_continuation_request,
    supports_responses_chaining,
};
use crate::llm::providers::gemini::wire::Part;
use crate::project_doc::build_instruction_appendix;
use crate::prompts::PromptContext;
use crate::prompts::system::compose_system_instruction_text;
use crate::utils::colors::style;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, warn};

fn record_terminal_turn_event(
    event_recorder: &mut ExecEventRecorder,
    outcome: &TaskOutcome,
    usage: vtcode_exec_events::Usage,
) {
    if outcome.is_success() {
        event_recorder.record_thread_event(vtcode_exec_events::ThreadEvent::TurnCompleted(
            vtcode_exec_events::TurnCompletedEvent { usage },
        ));
    } else {
        event_recorder.record_thread_event(vtcode_exec_events::ThreadEvent::TurnFailed(
            vtcode_exec_events::TurnFailedEvent {
                message: outcome.description(),
                usage: Some(usage),
            },
        ));
    }
}

fn tool_loop_limit_reached(loop_count: usize, max_tool_loops: usize) -> bool {
    max_tool_loops > 0 && loop_count >= max_tool_loops
}

fn emit_blocked_handoff_events(
    event_recorder: &mut ExecEventRecorder,
    current_path: &std::path::Path,
    archive_path: &std::path::Path,
) {
    for path in [current_path, archive_path] {
        event_recorder.harness_event(
            HarnessEventKind::BlockedHandoffWritten,
            Some("Blocked handoff written".to_string()),
            None,
            Some(path.display().to_string()),
            None,
        );
    }
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

fn prepare_responses_request_messages(
    session_state: &mut AgentSessionState,
    provider_name: &str,
    provider_supports_responses_compaction: bool,
    model: &str,
    messages: Vec<Message>,
) -> (Vec<Message>, Option<String>) {
    let prepared = prepare_responses_continuation_request(
        provider_name,
        provider_supports_responses_compaction,
        messages,
        session_state.previous_response_chain_for(provider_name, model),
    );
    if prepared.clear_stale_chain {
        session_state.clear_previous_response_chain_for(provider_name, model);
    }

    (prepared.messages, prepared.previous_response_id)
}

fn stop_reason_from_finish_reason(finish_reason: &FinishReason) -> String {
    match finish_reason {
        FinishReason::Stop => "end_turn".to_string(),
        FinishReason::Length => "max_tokens".to_string(),
        FinishReason::ToolCalls => "tool_calls".to_string(),
        FinishReason::ContentFilter => "content_filter".to_string(),
        FinishReason::Pause => "pause_turn".to_string(),
        FinishReason::Refusal => "refusal".to_string(),
        FinishReason::Error(message) => message.clone(),
    }
}

fn estimate_session_cost_usd(
    provider: &str,
    model: &str,
    usage: &vtcode_exec_events::Usage,
) -> Option<f64> {
    let usage = crate::llm::provider::Usage {
        prompt_tokens: u32::try_from(usage.input_tokens).unwrap_or(u32::MAX),
        completion_tokens: u32::try_from(usage.output_tokens).unwrap_or(u32::MAX),
        total_tokens: u32::try_from(usage.input_tokens.saturating_add(usage.output_tokens))
            .unwrap_or(u32::MAX),
        cached_prompt_tokens: Some(u32::try_from(usage.cached_input_tokens).unwrap_or(u32::MAX)),
        cache_creation_tokens: Some(u32::try_from(usage.cache_creation_tokens).unwrap_or(u32::MAX)),
        cache_read_tokens: Some(u32::try_from(usage.cached_input_tokens).unwrap_or(u32::MAX)),
    };
    let resolved = ModelResolver::resolve(Some(provider), model, &[], None)?;
    let pricing = resolved.pricing()?;
    ModelResolver::estimate_cost(pricing, &usage)
}

impl AgentRunner {
    async fn resolve_completion_acceptance(
        &mut self,
        effective_task: &Task,
        session_state: &mut AgentSessionState,
        event_recorder: &mut ExecEventRecorder,
        orchestration_enabled: bool,
        verification_results: &[VerificationResult],
        revision_rounds_used: &mut usize,
        max_revision_rounds: usize,
        should_write_blocked_handoff: &mut bool,
    ) -> Result<bool> {
        if !orchestration_enabled {
            session_state.is_completed = true;
            session_state.outcome = TaskOutcome::Success;
            return Ok(true);
        }

        match self
            .apply_evaluator_gate(
                effective_task,
                session_state,
                event_recorder,
                verification_results,
                revision_rounds_used,
                max_revision_rounds,
            )
            .await?
        {
            EvaluatorGateOutcome::Accept => {
                session_state.is_completed = true;
                session_state.outcome = TaskOutcome::Success;
                Ok(true)
            }
            EvaluatorGateOutcome::Continue { prompt } => {
                session_state.add_user_message(prompt);
                Ok(false)
            }
            EvaluatorGateOutcome::Exhausted { reason } => {
                session_state.outcome = TaskOutcome::Failed { reason };
                *should_write_blocked_handoff = true;
                Ok(true)
            }
        }
    }

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

        let steering_receiver = self.steering_receiver.lock().take();
        let runtime_setup = {
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
            let tool_snapshot = self.build_universal_tool_snapshot().await?;
            let request_tools = tool_snapshot.snapshot.clone();
            let prompt_tools = request_tools
                .clone()
                .unwrap_or_else(|| Arc::new(Vec::new()));

            let system_prompt = if is_simple_task {
                // One-time clone for simple tasks to override prompt mode (not per-turn)
                let mut config = self.config().clone();
                config.agent.system_prompt_mode = SystemPromptMode::Minimal;
                let mut prompt_context = PromptContext::from_workspace_tools(
                    self._workspace.as_path(),
                    prompt_tools
                        .iter()
                        .map(|tool| tool.function_name().to_string()),
                );
                prompt_context.load_available_skills();
                let mut prompt = compose_system_instruction_text(
                    self._workspace.as_path(),
                    Some(&config),
                    Some(&prompt_context),
                )
                .await;
                let mut appendix_config = config.agent.clone();
                if !config.memories_enabled() {
                    appendix_config.persistent_memory.enabled = false;
                }
                if let Some(appendix) =
                    build_instruction_appendix(&appendix_config, self._workspace.as_path()).await
                {
                    prompt.push_str("\n\n# INSTRUCTIONS\n");
                    prompt.push_str(&appendix);
                }
                prompt
            } else {
                self.system_prompt.clone()
            };

            // Prepare conversation with task context
            let system_instruction = Arc::new(system_prompt);
            let mut conversation = conversation_from_messages(&self.bootstrap_messages);
            conversation.extend(build_conversation(task, contexts));

            // Maintain a mirrored conversation history for providers that expect
            // OpenAI/Anthropic style message roles.
            let conversation_messages = build_messages_from_conversation(&conversation);

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

            let mut runtime = AgentRuntime::new(session_state, None, steering_receiver);

            if let Err(err) = self.tool_registry.initialize_async().await {
                warn!(
                    error = %err,
                    "Tool registry initialization failed at task start"
                );
                runtime
                    .state
                    .warnings
                    .push(format!("Tool registry init failed: {err}"));
            }

            let review_like = is_review_like_task(task);
            let full_auto_active = self
                .tool_registry
                .current_full_auto_allowlist()
                .await
                .is_some();
            let orchestration_enabled =
                self.harness_plan_build_evaluate_enabled(full_auto_active, review_like);
            let planner_artifacts = if orchestration_enabled {
                Some(self.run_planner_phase(task, &mut event_recorder).await?)
            } else {
                None
            };
            let effective_task = planner_artifacts
                .as_ref()
                .map(|artifacts| self.augment_generator_task(task, artifacts))
                .unwrap_or_else(|| task.clone());

            let mut continuation_controller = ContinuationController::new(
                self._workspace.clone(),
                self.tool_registry.plan_mode_state(),
                self.config().agent.harness.continuation_policy.clone(),
                full_auto_active,
                self.tool_registry.is_plan_mode(),
                review_like,
            );
            continuation_controller.prepare(&effective_task).await?;

            (
                agent_prefix,
                event_recorder,
                run_started_at,
                is_simple_task,
                request_tools,
                tool_snapshot.tool_catalog_hash,
                tool_snapshot.version,
                system_instruction,
                preserve_recent_turns,
                max_tool_loops,
                max_context_tokens,
                runtime,
                continuation_controller,
                effective_task,
                orchestration_enabled,
            )
        };

        let (
            agent_prefix,
            mut event_recorder,
            run_started_at,
            is_simple_task,
            mut request_tools,
            mut tool_catalog_hash,
            mut tool_catalog_version,
            system_instruction,
            preserve_recent_turns,
            max_tool_loops,
            max_context_tokens,
            mut runtime,
            mut continuation_controller,
            effective_task,
            orchestration_enabled,
        ) = runtime_setup;
        let mut cost_warning_emitted = false;
        let max_budget_usd = self.config().agent.harness.max_budget_usd;
        let max_revision_rounds = self.config().agent.harness.max_revision_rounds.max(1);
        let mut revision_rounds_used = 0usize;
        let mut should_write_blocked_handoff = false;

        let result = {
            for turn in 0..self.max_turns {
                if matches!(
                    runtime.poll_turn_control().await,
                    RuntimeControl::StopRequested
                ) {
                    self.runner_println(format_args!(
                        "{} {}",
                        agent_prefix,
                        style("Stopped by steering signal.").red().bold()
                    ));
                    runtime.state.outcome = TaskOutcome::Cancelled;
                    break;
                }

                if let Some(input) = runtime.run_until_idle() {
                    self.runner_println(format_args!(
                        "{} {}: {}",
                        agent_prefix,
                        style("Follow-up Input").cyan().bold(),
                        input
                    ));
                }

                let utilization = runtime.state.utilization();
                if utilization > 0.90 {
                    warn!("Context at {:.1}% - approaching limit", utilization * 100.0);
                    runtime.state.warnings.push(format!(
                        "Token budget at {}% - approaching context limit",
                        (utilization * 100.0) as u32
                    ));
                }

                if runtime.state.is_completed {
                    runtime.state.outcome = TaskOutcome::Success;
                    break;
                }

                self.runner_println(format_args!(
                    "{} {} is processing turn {}...",
                    agent_prefix,
                    style("(PROC)").cyan().bold(),
                    turn + 1
                ));

                let turn_model = self.get_selected_model();
                let provider_name = self.provider_client.name().to_string();
                if std::env::var_os("VTCODE_DEBUG_PROVIDER").is_some() {
                    tracing::debug!(
                        provider_client = self.provider_client.name(),
                        turn_model,
                        "Provider debug turn selection"
                    );
                }
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

                self.summarize_conversation_if_needed(
                    &mut runtime.state,
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
                    .map(|model| model.provider())
                    .unwrap_or(ModelProvider::Gemini);

                if matches!(provider_kind, ModelProvider::Gemini)
                    && runtime.state.conversation.len() > runtime.state.last_processed_message_idx
                {
                    for content in
                        &runtime.state.conversation[runtime.state.last_processed_message_idx..]
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
                        runtime.state.messages.push(message);
                    }
                    runtime.state.last_processed_message_idx = runtime.state.conversation.len();
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

                let current_messages = runtime.state.messages.clone();
                let (request_messages, previous_response_id) = prepare_responses_request_messages(
                    &mut runtime.state,
                    &provider_name,
                    self.provider_client
                        .supports_responses_compaction(&turn_model),
                    &turn_model,
                    current_messages,
                );

                let request = build_harness_request_plan(HarnessRequestPlanInput {
                    messages: request_messages,
                    system_prompt: system_instruction.as_ref().clone(),
                    tools: request_tools.clone(),
                    model: turn_model.clone(),
                    max_tokens,
                    temperature,
                    stream: self.provider_client.supports_streaming(),
                    tool_choice: None,
                    parallel_tool_config,
                    reasoning_effort,
                    verbosity: turn_verbosity,
                    metadata: None,
                    context_management: None,
                    previous_response_id,
                    prompt_cache_key: build_openai_prompt_cache_key(
                        provider_name.eq_ignore_ascii_case("openai")
                            && self.config().prompt_cache.enabled
                            && self.config().prompt_cache.providers.openai.enabled,
                        &self
                            .config()
                            .prompt_cache
                            .providers
                            .openai
                            .prompt_cache_key_mode,
                        Some(&self.session_id),
                    ),
                    prompt_cache_profile: None,
                    tool_catalog_hash,
                })
                .request;
                let previous_response_chain_present = request.previous_response_id.is_some();
                let sent_messages = request.messages.clone();

                let turn_output = runtime
                    .run_turn_once(
                        &mut self.provider_client,
                        request,
                        Some(std::time::Duration::from_secs(60)),
                    )
                    .await?;
                event_recorder.record_thread_events(runtime.take_emitted_events());
                let response = turn_output.response;
                runtime.state.stop_reason =
                    Some(stop_reason_from_finish_reason(&response.finish_reason));
                if supports_responses_chaining(
                    &provider_name,
                    self.provider_client
                        .supports_responses_compaction(&turn_model),
                ) {
                    runtime.state.set_previous_response_chain(
                        &provider_name,
                        &turn_model,
                        response.request_id.as_deref(),
                        &sent_messages,
                    );
                }
                match estimate_session_cost_usd(
                    self.config().agent.provider.as_str(),
                    &turn_model,
                    &runtime.state.stats.total_usage,
                ) {
                    Some(total_cost_usd) => {
                        runtime.state.total_cost_usd = Some(total_cost_usd);
                        if let Some(max_budget_usd) = max_budget_usd
                            && total_cost_usd > max_budget_usd
                        {
                            runtime.state.outcome =
                                TaskOutcome::budget_limit_reached(max_budget_usd, total_cost_usd);
                            break;
                        }
                    }
                    None => {
                        runtime.state.total_cost_usd = None;
                        if max_budget_usd.is_some() && !cost_warning_emitted {
                            cost_warning_emitted = true;
                            let warning_message = format!(
                                "Budget enforcement disabled for model `{turn_model}` because pricing metadata is unavailable"
                            );
                            warn!(
                                provider = %self.config().agent.provider,
                                model = %turn_model,
                                "Budget enforcement disabled because pricing metadata is unavailable"
                            );
                            runtime.state.warnings.push(warning_message);
                        }
                    }
                }
                self.runner_println(format_args!(
                    "{} {} {} received response, processing...",
                    agent_prefix,
                    self.agent_type,
                    style("(RECV)").green().bold()
                ));

                self.warn_on_empty_response(
                    &agent_prefix,
                    response.content.as_deref().unwrap_or(""),
                    response
                        .tool_calls
                        .as_ref()
                        .is_some_and(|tool_calls| !tool_calls.is_empty()),
                );

                let response_text = response.content_string();
                if !response_text.trim().is_empty() {
                    self.emit_final_assistant_message(&self.agent_type, &response_text);
                }

                let mut effective_tool_calls = response.tool_calls.clone();
                let mut forced_continuation = false;

                if effective_tool_calls.is_none()
                    && response.content_text().len() < 150
                    && let Some(args_value) = detect_textual_exec_tool_call(response.content_text())
                {
                    effective_tool_calls = Some(vec![ToolCall::function(
                        format!("call_text_{}", turn),
                        tools::UNIFIED_EXEC.to_string(),
                        args_value.to_string(),
                    )]);
                }

                let is_gemini = matches!(provider_kind, ModelProvider::Gemini);

                if !runtime.state.is_completed
                    && effective_tool_calls
                        .as_ref()
                        .is_none_or(|tool_calls| tool_calls.is_empty())
                    && !response.content_text().is_empty()
                {
                    if check_for_response_loop(response.content_text(), &mut runtime.state) {
                        self.runner_println(format_args!(
                            "[{}] {}",
                            self.agent_type,
                            style(
                                "Repetitive assistant response detected. Breaking potential loop."
                            )
                            .red()
                            .bold()
                        ));
                        runtime.state.outcome = TaskOutcome::LoopDetected;
                        break;
                    }

                    if check_completion_indicators(response.content_text()) {
                        self.runner_println(format_args!(
                            "[{}] {}",
                            self.agent_type,
                            style("Completion indicator detected.").green().bold()
                        ));
                        match continuation_controller
                            .assess_completion(&effective_task, &runtime.state)
                            .await?
                        {
                            CompletionAssessment::Accept => {
                                if self
                                    .resolve_completion_acceptance(
                                        &effective_task,
                                        &mut runtime.state,
                                        &mut event_recorder,
                                        orchestration_enabled,
                                        &[],
                                        &mut revision_rounds_used,
                                        max_revision_rounds,
                                        &mut should_write_blocked_handoff,
                                    )
                                    .await?
                                {
                                    break;
                                }
                                forced_continuation = true;
                            }
                            CompletionAssessment::SkipAccept { reason } => {
                                event_recorder.harness_event(
                                    HarnessEventKind::ContinuationSkipped,
                                    Some(reason),
                                    None,
                                    None,
                                    None,
                                );
                                runtime.state.is_completed = true;
                                runtime.state.outcome = TaskOutcome::Success;
                                break;
                            }
                            CompletionAssessment::Continue { reason, prompt } => {
                                event_recorder.harness_event(
                                    HarnessEventKind::ContinuationStarted,
                                    Some(reason),
                                    None,
                                    None,
                                    None,
                                );
                                runtime.state.add_user_message(prompt);
                                forced_continuation = true;
                            }
                            CompletionAssessment::Verify { commands } => {
                                event_recorder.harness_event(
                                    HarnessEventKind::VerificationStarted,
                                    Some(format!("Running verification: {}", commands.join(", "))),
                                    commands.first().cloned(),
                                    None,
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
                                        None,
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
                                        None,
                                        Some(0),
                                    );
                                }

                                match continuation_controller
                                    .after_verification(&verification_results)
                                    .await?
                                {
                                    CompletionAssessment::Accept
                                    | CompletionAssessment::SkipAccept { .. } => {
                                        if self
                                            .resolve_completion_acceptance(
                                                &effective_task,
                                                &mut runtime.state,
                                                &mut event_recorder,
                                                orchestration_enabled,
                                                &verification_results,
                                                &mut revision_rounds_used,
                                                max_revision_rounds,
                                                &mut should_write_blocked_handoff,
                                            )
                                            .await?
                                        {
                                            break;
                                        }
                                        forced_continuation = true;
                                    }
                                    CompletionAssessment::Continue { reason, prompt } => {
                                        event_recorder.harness_event(
                                            HarnessEventKind::ContinuationStarted,
                                            Some(reason),
                                            None,
                                            None,
                                            None,
                                        );
                                        runtime.state.add_user_message(prompt);
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
                    .filter(|tool_calls| !tool_calls.is_empty())
                    .cloned()
                {
                    self.handle_tool_calls(
                        tool_calls,
                        &mut runtime,
                        &mut event_recorder,
                        &agent_prefix,
                        is_gemini,
                        previous_response_chain_present,
                    )
                    .await?;
                    event_recorder.record_thread_events(runtime.take_emitted_events());
                }

                // Refresh tool definitions if the catalog was mutated during tool
                // execution (e.g. tools.load / tools.unload / skill activation).
                // The version is bumped by `note_explicit_refresh` on every
                // register_tool / unregister_tool call; we only re-snapshot when
                // it has actually changed.
                {
                    let current_version = self.tool_registry.tool_catalog_state().current_version();
                    if current_version != tool_catalog_version {
                        debug!(
                            old_version = tool_catalog_version,
                            new_version = current_version,
                            "Tool catalog changed mid-task; refreshing tool snapshot"
                        );
                        let refreshed = self.build_universal_tool_snapshot().await?;
                        request_tools = refreshed.snapshot.clone();
                        tool_catalog_hash = refreshed.tool_catalog_hash;
                        tool_catalog_version = refreshed.version;
                    }
                }

                let had_effective_shell_tool_call =
                    effective_tool_calls.as_ref().is_some_and(|calls| {
                        calls.iter().any(|call| {
                            call.function
                                .as_ref()
                                .map(|function| function.name.as_str())
                                == Some(tools::UNIFIED_EXEC)
                        })
                    });
                let had_tool_call = response
                    .tool_calls
                    .as_ref()
                    .is_some_and(|tool_calls| !tool_calls.is_empty())
                    || had_effective_shell_tool_call;

                if had_tool_call {
                    let loops = runtime.state.register_tool_loop();
                    if tool_loop_limit_reached(loops, runtime.state.constraints.max_tool_loops) {
                        let warning_message = format!(
                            "Reached tool-call limit of {} iterations; pausing autonomous loop",
                            runtime.state.constraints.max_tool_loops
                        );
                        self.record_warning(
                            &agent_prefix,
                            &mut runtime.state,
                            &mut event_recorder,
                            warning_message,
                        );
                        runtime.state.mark_tool_loop_limit_hit();
                        break;
                    }
                    runtime.state.consecutive_idle_turns = 0;
                } else {
                    runtime.state.reset_tool_loop_guard();
                    if forced_continuation {
                        runtime.state.consecutive_idle_turns = 0;
                    } else if !runtime.state.is_completed {
                        runtime.state.consecutive_idle_turns =
                            runtime.state.consecutive_idle_turns.saturating_add(1);
                        if runtime.state.consecutive_idle_turns >= IDLE_TURN_LIMIT {
                            let warning_message = format!(
                                "No tool calls or completion for {} consecutive turns; halting to avoid idle loop",
                                runtime.state.consecutive_idle_turns
                            );
                            self.record_warning(
                                &agent_prefix,
                                &mut runtime.state,
                                &mut event_recorder,
                                warning_message,
                            );
                            runtime.state.outcome = TaskOutcome::StoppedNoAction;
                            break;
                        }
                    }
                }

                let should_continue = forced_continuation
                    || had_tool_call
                    || runtime.has_pending_follow_up_inputs()
                    || (!runtime.state.is_completed && (turn + 1) < self.max_turns);

                if !should_continue {
                    if runtime.state.is_completed {
                        runtime.state.outcome = TaskOutcome::Success;
                    } else if (turn + 1) >= self.max_turns {
                        runtime.state.outcome =
                            TaskOutcome::turn_limit_reached(self.max_turns, turn + 1);
                    } else {
                        runtime.state.outcome = TaskOutcome::StoppedNoAction;
                    }
                    break;
                }
            }

            runtime.state.finalize_outcome(self.max_turns);

            let total_duration_ms = run_started_at.elapsed().as_millis();

            // Agent execution completed
            self.runner_println(format_args!("{} Done", agent_prefix));

            // Generate meaningful summary based on agent actions
            let average_turn_duration_ms = if runtime.state.turn_count > 0 {
                Some(runtime.state.turn_total_ms as f64 / runtime.state.turn_count as f64)
            } else {
                None
            };

            let max_turn_duration_ms = if runtime.state.turn_count > 0 {
                Some(runtime.state.turn_max_ms)
            } else {
                None
            };

            let outcome = runtime.state.outcome.clone();
            self.thread_handle
                .replace_messages(runtime.state.messages.clone());
            let summary = self.generate_task_summary(
                &effective_task,
                &runtime.state.modified_files,
                &runtime.state.executed_commands,
                &runtime.state.warnings,
                &runtime.state.messages,
                runtime.state.stats.turns_executed,
                runtime.state.max_tool_loop_streak,
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

            let runtime_agent_config = self.core_agent_config();
            if let Err(err) = crate::persistent_memory::finalize_persistent_memory(
                &runtime_agent_config,
                Some(self.config()),
                &runtime.state.messages,
            )
            .await
            {
                warn!(
                    error = %err,
                    session_id = %self.session_id,
                    "Failed to update persistent memory"
                );
            }

            if runtime.state.outcome.is_hard_block() || should_write_blocked_handoff {
                let relevant_paths = existing_harness_artifact_paths(&self._workspace);
                match write_blocked_handoff(
                    &self._workspace,
                    &self.session_id,
                    runtime.state.outcome.code(),
                    &runtime.state.outcome.description(),
                    &relevant_paths,
                ) {
                    Ok(artifacts) => emit_blocked_handoff_events(
                        &mut event_recorder,
                        &artifacts.current_path,
                        &artifacts.archive_path,
                    ),
                    Err(err) => warn!(
                        error = %err,
                        session_id = %self.session_id,
                        "Failed to persist blocked handoff"
                    ),
                }
            }

            let total_usage = runtime.state.stats.total_usage.clone();
            record_terminal_turn_event(
                &mut event_recorder,
                &runtime.state.outcome,
                total_usage.clone(),
            );
            event_recorder.thread_completed(
                &self.session_id,
                runtime.state.outcome.thread_completion_subtype(),
                runtime.state.outcome.code(),
                runtime
                    .state
                    .outcome
                    .is_success()
                    .then_some(summary.as_str()),
                runtime.state.stop_reason.as_deref(),
                total_usage,
                runtime
                    .state
                    .total_cost_usd
                    .and_then(serde_json::Number::from_f64),
                runtime.state.stats.turns_executed,
            );
            let thread_events = event_recorder.into_events();
            let steering_receiver = runtime.take_steering_receiver();
            let state = std::mem::replace(
                &mut runtime.state,
                AgentSessionState::new(
                    self.session_id.clone(),
                    self.max_turns,
                    max_tool_loops,
                    max_context_tokens,
                ),
            );

            Ok((
                state.into_results(summary, thread_events, total_duration_ms),
                steering_receiver,
            ))
        };

        let result = match result {
            Ok((task_results, steering_receiver)) => {
                *self.steering_receiver.lock() = steering_receiver;
                Ok(task_results)
            }
            Err(err) => {
                *self.steering_receiver.lock() = runtime.take_steering_receiver();
                Err(err)
            }
        };

        self.tool_registry.set_harness_task(None);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{
        prepare_responses_request_messages, record_terminal_turn_event, tool_loop_limit_reached,
    };
    use crate::core::agent::events::ExecEventRecorder;
    use crate::core::agent::session::AgentSessionState;
    use crate::core::agent::task::TaskOutcome;
    use crate::exec::events::ThreadEvent;
    use crate::llm::provider::Message;

    #[test]
    fn failed_outcome_emits_only_turn_failed() {
        let mut recorder = ExecEventRecorder::new("thread", None, None);
        recorder.turn_started();

        record_terminal_turn_event(
            &mut recorder,
            &TaskOutcome::Failed {
                reason: "boom".to_string(),
            },
            Default::default(),
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

        record_terminal_turn_event(&mut recorder, &TaskOutcome::Success, Default::default());

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

    #[test]
    fn openai_prepare_responses_request_messages_uses_incremental_suffix() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        let prior_messages = vec![Message::user("hello".to_string())];
        let current_messages = vec![
            Message::user("hello".to_string()),
            Message::user("continue".to_string()),
        ];
        state.set_previous_response_chain("openai", "gpt-5.4", Some("resp_123"), &prior_messages);

        let (request_messages, previous_response_id) = prepare_responses_request_messages(
            &mut state,
            "openai",
            false,
            "gpt-5.4",
            current_messages,
        );

        assert_eq!(previous_response_id.as_deref(), Some("resp_123"));
        assert_eq!(
            request_messages,
            vec![Message::user("continue".to_string())]
        );
    }

    #[test]
    fn gemini_prepare_responses_request_messages_keeps_full_history() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        let prior_messages = vec![Message::user("hello".to_string())];
        let current_messages = vec![
            Message::user("hello".to_string()),
            Message::user("continue".to_string()),
        ];
        state.set_previous_response_chain(
            "gemini",
            "gemini-2.5-pro",
            Some("resp_123"),
            &prior_messages,
        );

        let (request_messages, previous_response_id) = prepare_responses_request_messages(
            &mut state,
            "gemini",
            false,
            "gemini-2.5-pro",
            current_messages.clone(),
        );

        assert_eq!(previous_response_id.as_deref(), Some("resp_123"));
        assert_eq!(request_messages, current_messages);
    }

    #[test]
    fn compatible_prepare_responses_request_messages_uses_incremental_suffix() {
        let mut state = AgentSessionState::new("session".to_string(), 4, 4, 16_000);
        let prior_messages = vec![Message::user("hello".to_string())];
        let current_messages = vec![
            Message::user("hello".to_string()),
            Message::user("continue".to_string()),
        ];
        state.set_previous_response_chain("mycorp", "gpt-5.4", Some("resp_123"), &prior_messages);

        let (request_messages, previous_response_id) = prepare_responses_request_messages(
            &mut state,
            "mycorp",
            true,
            "gpt-5.4",
            current_messages,
        );

        assert_eq!(previous_response_id.as_deref(), Some("resp_123"));
        assert_eq!(
            request_messages,
            vec![Message::user("continue".to_string())]
        );
    }
}
