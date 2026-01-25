//! Agent runner for executing individual agent instances

use crate::config::VTCodeConfig;
use crate::config::constants::{defaults, tools};
use crate::config::loader::ConfigManager;
use crate::config::models::{ModelId, Provider as ModelProvider};
use crate::config::types::{ReasoningEffortLevel, SystemPromptMode, VerbosityLevel};
use crate::core::agent::completion::{check_completion_indicators, check_for_response_loop};
use crate::core::agent::conversation::{
    build_conversation, build_messages_from_conversation, compose_system_instruction,
};
use crate::core::agent::events::{EventSink, ExecEventRecorder};
use crate::core::agent::state::{ApiFailureTracker, TaskRunState};
pub use crate::core::agent::task::{ContextItem, Task, TaskOutcome, TaskResults};
use crate::core::agent::types::AgentType;
use crate::core::context_optimizer::ContextOptimizer;
use crate::core::loop_detector::LoopDetector;
use crate::exec::events::ThreadEvent;
use crate::gemini::{Content, Part, Tool};
use crate::llm::factory::create_provider_for_model;
use crate::llm::provider as uni_provider;
use crate::llm::provider::{LLMRequest, Message, ToolCall};
use crate::llm::{AnyClient, make_client};
use crate::prompts::system::compose_system_instruction_text;
use crate::tools::{ToolRegistry, build_function_declarations};

use crate::utils::colors::style;
use constants::{IDLE_TURN_LIMIT, ROLE_MODEL};
use helpers::detect_textual_run_pty_cmd;
use anyhow::{Result, anyhow};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{info, warn};

mod constants;
mod config_helpers;
mod helpers;
mod optimizer;
mod output;
mod summary;
mod summarize;
mod telemetry;
mod tool_access;
mod tool_args;
mod tool_exec;
mod types;
mod provider_response;
mod validation;
mod workspace_config;

macro_rules! runner_println {
    ($runner:expr, $($arg:tt)*) => {
        if !$runner.quiet {
            println!($($arg)*);
        }
    };
}

#[cfg(test)]
mod tests;

/// Individual agent runner for executing specialized agent tasks
pub struct AgentRunner {
    /// Agent type and configuration
    agent_type: AgentType,
    /// LLM client for this agent
    client: AnyClient,
    /// Unified provider client (OpenAI/Anthropic/Gemini) for tool-calling
    provider_client: Box<dyn uni_provider::LLMProvider>,
    /// Tool registry with restricted access
    tool_registry: ToolRegistry,
    /// System prompt content
    system_prompt: String,
    /// Session information
    session_id: String,
    /// Workspace path
    _workspace: PathBuf,
    /// Cached vtcode configuration
    config: Arc<VTCodeConfig>,
    /// Model identifier
    model: String,
    /// API key (for provider client construction in future flows)
    _api_key: String,
    /// Reasoning effort level for models that support it
    reasoning_effort: Option<ReasoningEffortLevel>,
    /// Verbosity level for output text
    verbosity: Option<VerbosityLevel>,
    /// Suppress stdout output when emitting structured events
    quiet: bool,
    /// Optional sink for streaming structured events
    event_sink: Option<EventSink>,
    /// Maximum number of autonomous turns before halting
    max_turns: usize,
    /// Loop detector to prevent infinite exploration
    loop_detector: RefCell<LoopDetector>,
    /// Cached shell policy patterns to avoid recompilation

    /// API failure tracking for exponential backoff
    failure_tracker: RefCell<ApiFailureTracker>,
    /// Context optimizer for token budget management
    context_optimizer: RefCell<ContextOptimizer>,
    /// Tracks recent streaming failures to avoid repeated double-requests
    streaming_failures: RefCell<u8>,
    /// Records when streaming last failed for cooldown-based re-enablement
    streaming_last_failure: RefCell<Option<Instant>>,
}

impl AgentRunner {
    /// Get the selected model for the current turn.
    fn get_selected_model(&self) -> String {
        self.model.clone()
    }


    /// Create a new agent runner
    pub async fn new(
        agent_type: AgentType,
        model: ModelId,
        api_key: String,
        workspace: PathBuf,
        session_id: String,
        reasoning_effort: Option<ReasoningEffortLevel>,
        verbosity: Option<VerbosityLevel>,
    ) -> Result<Self> {
        // Create client based on model
        let client: AnyClient = make_client(api_key.clone(), model)?;

        // Create unified provider client for tool calling
        let provider_client = create_provider_for_model(model.as_str(), api_key.clone(), None)
            .map_err(|e| anyhow!("Failed to create provider client: {}", e))?;

        // Load configuration once to seed system prompt and runtime policies
        let (config_value, system_prompt) = match ConfigManager::load_from_workspace(&workspace) {
            Ok(manager) => {
                let cfg = manager.config().clone();
                let prompt =
                    compose_system_instruction_text(workspace.as_path(), Some(&cfg), None).await;
                (cfg, prompt)
            }
            Err(err) => {
                warn!("Failed to load vtcode configuration for system prompt composition: {err:#}");
                let cfg = VTCodeConfig::default();
                let prompt = compose_system_instruction_text(workspace.as_path(), None, None).await;
                (cfg, prompt)
            }
        };

        let max_repeated_tool_calls = config_value.tools.max_repeated_tool_calls.max(1);
        let config = Arc::new(config_value);
        let tool_registry = ToolRegistry::new(workspace.clone()).await;
        tool_registry.set_harness_session(session_id.clone());
        tool_registry.set_agent_type(agent_type.to_string());
        tool_registry.apply_timeout_policy(&config.timeouts);
        let loop_detector = LoopDetector::with_max_repeated_calls(max_repeated_tool_calls);

        Ok(Self {
            agent_type,
            client,
            provider_client,
            tool_registry,
            system_prompt,
            session_id,
            _workspace: workspace,
            config,
            model: model.to_string(),
            _api_key: api_key,
            reasoning_effort,
            verbosity,
            quiet: false,
            event_sink: None,
            max_turns: defaults::DEFAULT_FULL_AUTO_MAX_TURNS,
            loop_detector: RefCell::new(loop_detector),
            failure_tracker: RefCell::new(ApiFailureTracker::new()),
            context_optimizer: RefCell::new(ContextOptimizer::new()),
            streaming_failures: RefCell::new(0),
            streaming_last_failure: RefCell::new(None),
        })
    }

    /// Enable or disable console output for this runner.
    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
    }

    /// Attach a callback that will be invoked for each structured event as it is recorded.
    pub fn set_event_handler<F>(&mut self, handler: F)
    where
        F: FnMut(&ThreadEvent) + Send + 'static,
    {
        self.event_sink = Some(Arc::new(Mutex::new(Box::new(handler))));
    }

    /// Remove any previously registered structured event callback.
    pub fn clear_event_handler(&mut self) {
        self.event_sink = None;
    }

    /// Enable full-auto execution with the provided allow-list.
    pub async fn enable_full_auto(&mut self, allowed_tools: &[String]) {
        self.tool_registry
            .enable_full_auto_mode(allowed_tools)
            .await;
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
            let mut event_recorder =
                ExecEventRecorder::new(self.session_id.clone(), self.event_sink.clone());
            event_recorder.turn_started();
            runner_println!(
                self,
                "{} {}",
                agent_prefix,
                self.create_progress_message("thinking", None)
            );

            runner_println!(
                self,
                "{} Executing {} task: {}",
                style("[AGENT]").blue().bold().on_black(),
                self.agent_type,
                task.title
            );

            let run_started_at = std::time::Instant::now();
            let is_simple_task = Self::is_simple_task(task, contexts);

            let system_prompt = if is_simple_task {
                let mut config = self.config().clone();
                config.agent.system_prompt_mode = SystemPromptMode::Minimal;
                compose_system_instruction_text(self._workspace.as_path(), Some(&config), None)
                    .await
            } else {
                self.system_prompt.clone()
            };

            // Prepare conversation with task context
            let system_instruction = compose_system_instruction(&system_prompt, task, contexts);
            let conversation = build_conversation(task, contexts);

            // Build available tools for this agent
            let tools = self.build_universal_tools().await?;

            // Maintain a mirrored conversation history for providers that expect
            // OpenAI/Anthropic style message roles.
            let conversation_messages =
                build_messages_from_conversation(&system_instruction, &conversation);

            // Track execution results
            // Determine loop guards via cached configuration
            let max_tool_loops = self.config().tools.max_tool_loops.max(1);
            let preserve_recent_turns = self.config().context.preserve_recent_turns;
            let max_context_tokens = self.config().context.max_context_tokens;

            let mut task_state = TaskRunState::new(
                conversation,
                conversation_messages,
                max_tool_loops,
                max_context_tokens,
            );
            // Pre-reserve capacity for conversation messages to avoid reallocations
            // Typical: 2-3 messages per turn (user input + assistant response + potential tool calls)
            task_state.conversation_messages.reserve(self.max_turns * 3);

            if let Err(err) = self.tool_registry.initialize_async().await {
                warn!(
                    error = %err,
                    "Tool registry initialization failed at task start"
                );
                task_state
                    .warnings
                    .push(format!("Tool registry init failed: {err}"));
            }

            // Agent execution loop uses max_turns for conversation flow
            for turn in 0..self.max_turns {
                // Check context utilization before each turn
                let utilization = task_state.utilization();
                if utilization > 0.90 {
                    // At 90%+ utilization, warn and consider stopping
                    warn!("Context at {:.1}% - approaching limit", utilization * 100.0);
                    task_state.warnings.push(format!(
                        "Token budget at {}% - approaching context limit",
                        (utilization * 100.0) as u32
                    ));
                    // Continue but warn - actual compaction handled by ContextOptimizer internally
                }

                if task_state.has_completed {
                    task_state.completion_outcome = TaskOutcome::Success;
                    break;
                }

                task_state.turns_executed = turn + 1;
                let turn_started_at = std::time::Instant::now();
                let mut turn_recorded = false;

                runner_println!(
                    self,
                    "{} {} is processing turn {}...",
                    agent_prefix,
                    style("(PROC)").yellow().bold(),
                    turn + 1
                );

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
                    &mut task_state,
                    preserve_recent_turns,
                    utilization,
                );

                let parallel_tool_config = if self.model.len() < 20 {
                    None
                } else if self
                    .provider_client
                    .supports_parallel_tool_config(&turn_model)
                {
                    Some(crate::llm::provider::ParallelToolConfig::anthropic_optimized())
                } else {
                    None
                };

                let provider_kind = turn_model
                    .parse::<ModelId>()
                    .map(|m| m.provider())
                    .unwrap_or(ModelProvider::Gemini);

                // Optimize: Only rebuild messages for Gemini incrementally from last processed index
                if matches!(provider_kind, ModelProvider::Gemini)
                    && task_state.conversation.len() > task_state.last_processed_message_idx
                {
                    // Incremental append instead of full rebuild
                    for content in &task_state.conversation[task_state.last_processed_message_idx..]
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
                        task_state.conversation_messages.push(message);
                    }
                    task_state.last_processed_message_idx = task_state.conversation.len();
                }

                let request = LLMRequest {
                    messages: task_state.conversation_messages.clone(),
                    system_prompt: Some(system_instruction.clone()),
                    tools: Some(tools.clone()),
                    model: turn_model.clone(),
                    max_tokens,
                    temperature: Some(0.7),
                    stream: self.provider_client.supports_streaming(),
                    parallel_tool_config,
                    reasoning_effort: if self.provider_client.supports_reasoning_effort(&turn_model)
                    {
                        turn_reasoning
                    } else {
                        None
                    },
                    verbosity: turn_verbosity,
                    ..Default::default()
                };

                let resp_summary = self
                    .collect_provider_response(
                        &request,
                        &mut event_recorder,
                        &agent_prefix,
                        &mut task_state.warnings,
                        turn,
                    )
                    .await?;

                runner_println!(
                    self,
                    "{} {}",
                    agent_prefix,
                    format!(
                        "{} {} received response, processing...",
                        self.agent_type,
                        style("(RECV)").green().bold()
                    )
                );

                if !resp_summary.reasoning_recorded
                    && let Some(reasoning) = resp_summary.reasoning.as_ref()
                {
                    event_recorder.reasoning(reasoning);
                }

                if resp_summary.content.trim().is_empty()
                    && resp_summary
                        .response
                        .tool_calls
                        .as_ref()
                        .is_none_or(|tc| tc.is_empty())
                {
                    runner_println!(
                        self,
                        "{} {} received empty response with no tool calls",
                        agent_prefix,
                        style("(WARN)").yellow().bold()
                    );
                }

                if !resp_summary.content.trim().is_empty() && !resp_summary.agent_message_streamed {
                    event_recorder.agent_message(&resp_summary.content);
                    Self::print_compact_response(
                        &self.agent_type,
                        &resp_summary.content,
                        self.quiet,
                    );
                    runner_println!(
                        self,
                        "{} {}",
                        agent_prefix,
                        format!(
                            "{} {}",
                            style("(ASSISTANT)").green().bold(),
                            resp_summary.content.trim()
                        )
                    );
                }

                const LOOP_DETECTED_MESSAGE: &str = "A potential loop was detected";
                if resp_summary.content.contains(LOOP_DETECTED_MESSAGE) {
                    let warning_message =
                        "Provider halted execution after detecting a potential tool loop";
                    self.record_warning(
                        &agent_prefix,
                        &mut task_state,
                        &mut event_recorder,
                        warning_message,
                    );
                    task_state.mark_tool_loop_limit_hit();
                    task_state.record_turn(&turn_started_at, &mut turn_recorded);
                    break;
                }

                let mut effective_tool_calls = resp_summary.response.tool_calls.clone();

                if effective_tool_calls
                    .as_ref()
                    .is_none_or(|calls| calls.is_empty())
                    && let Some(args_value) = detect_textual_run_pty_cmd(&resp_summary.content)
                {
                    let call_id = format!(
                        "textual_call_{}_{}",
                        turn,
                        task_state.conversation_messages.len()
                    );
                    effective_tool_calls = Some(vec![ToolCall::function(
                        call_id,
                        tools::RUN_PTY_CMD.to_owned(),
                        serde_json::to_string(&args_value)?,
                    )]);
                }

                let is_gemini = matches!(provider_kind, ModelProvider::Gemini);

                // Build assistant message
                let assistant_msg = if let Some(ref tc) = effective_tool_calls {
                    Message::assistant_with_tools(resp_summary.content.clone(), tc.clone())
                } else {
                    Message::assistant(resp_summary.content.clone())
                }
                .with_reasoning(resp_summary.reasoning.clone());

                task_state.conversation_messages.push(assistant_msg.clone());

                // Legacy conversation sync for Gemini
                if is_gemini {
                    task_state.conversation.push(Content {
                        role: ROLE_MODEL.into(),
                        parts: vec![Part::Text {
                            text: resp_summary.content.clone(),
                            thought_signature: None,
                        }],
                    });
                    task_state.last_processed_message_idx = task_state.conversation.len();
                }

                if let Some(tool_calls) = effective_tool_calls.filter(|tc| !tc.is_empty()) {
                    let can_parallelize = tool_calls.len() > 1
                        && tool_calls.iter().all(|call| {
                            if let Some(func) = &call.function {
                                matches!(
                                    func.name.as_str(),
                                    "list_files" | "read_file" | "grep_file" | "search_tools"
                                )
                            } else {
                                false
                            }
                        });

                    if can_parallelize {
                        self.execute_parallel_tool_calls(
                            tool_calls,
                            &mut task_state,
                            &mut event_recorder,
                            &agent_prefix,
                            is_gemini,
                        )
                        .await?;
                    } else {
                        self.execute_sequential_tool_calls(
                            tool_calls,
                            &mut task_state,
                            &mut event_recorder,
                            &agent_prefix,
                            is_gemini,
                        )
                        .await?;
                    }
                }

                if !task_state.has_completed && !resp_summary.content.is_empty() {
                    if check_for_response_loop(&resp_summary.content, &mut task_state) {
                        runner_println!(
                            self,
                            "[{}] {}",
                            self.agent_type,
                            style(
                                "Repetitive assistant response detected. Breaking potential loop."
                            )
                            .yellow()
                            .bold()
                        );
                        break;
                    }

                    if check_completion_indicators(&resp_summary.content) {
                        runner_println!(
                            self,
                            "[{}] {}",
                            self.agent_type,
                            style("Completion indicator detected.").green().bold()
                        );
                        task_state.has_completed = true;
                        task_state.completion_outcome = TaskOutcome::Success;
                        break;
                    }
                }

                let had_tool_call = assistant_msg
                    .tool_calls
                    .as_ref()
                    .is_some_and(|tc| !tc.is_empty());
                if had_tool_call {
                    let loops = task_state.register_tool_loop();
                    if loops >= task_state.max_tool_loops {
                        let warning_message = format!(
                            "Reached tool-call limit of {} iterations; pausing autonomous loop",
                            task_state.max_tool_loops
                        );
                        self.record_warning(
                            &agent_prefix,
                            &mut task_state,
                            &mut event_recorder,
                            warning_message,
                        );
                        task_state.mark_tool_loop_limit_hit();
                        task_state.record_turn(&turn_started_at, &mut turn_recorded);
                        break;
                    }
                    task_state.consecutive_idle_turns = 0;
                } else {
                    task_state.reset_tool_loop_guard();
                    if !task_state.has_completed {
                        task_state.consecutive_idle_turns =
                            task_state.consecutive_idle_turns.saturating_add(1);
                        if task_state.consecutive_idle_turns >= IDLE_TURN_LIMIT {
                            let warning_message = format!(
                                "No tool calls or completion for {} consecutive turns; halting to avoid idle loop",
                                task_state.consecutive_idle_turns
                            );
                            self.record_warning(
                                &agent_prefix,
                                &mut task_state,
                                &mut event_recorder,
                                warning_message,
                            );
                            task_state.completion_outcome = TaskOutcome::StoppedNoAction;
                            task_state.record_turn(&turn_started_at, &mut turn_recorded);
                            break;
                        }
                    }
                }

                let should_continue =
                    had_tool_call || (!task_state.has_completed && (turn + 1) < self.max_turns);

                // Record turn duration for the successfully completed turn
                task_state.record_turn(&turn_started_at, &mut turn_recorded);

                if !should_continue {
                    if task_state.has_completed {
                        task_state.completion_outcome = TaskOutcome::Success;
                    } else if (turn + 1) >= self.max_turns {
                        task_state.completion_outcome =
                            TaskOutcome::turn_limit_reached(self.max_turns, turn + 1);
                    } else {
                        task_state.completion_outcome = TaskOutcome::StoppedNoAction;
                    }
                    break;
                }
            }

            task_state.finalize_outcome(self.max_turns);

            let total_duration_ms = run_started_at.elapsed().as_millis();

            // Agent execution completed
            runner_println!(self, "{} Done", agent_prefix);

            // Generate meaningful summary based on agent actions
            let average_turn_duration_ms = if !task_state.turn_durations_ms.is_empty() {
                Some(
                    task_state.turn_durations_ms.iter().sum::<u128>() as f64
                        / task_state.turn_durations_ms.len() as f64,
                )
            } else {
                None
            };

            let max_turn_duration_ms = task_state.turn_durations_ms.iter().copied().max();

            let outcome = task_state.completion_outcome.clone(); // Clone to avoid moving
            let summary = self.generate_task_summary(
                task,
                &task_state.modified_files,
                &task_state.executed_commands,
                &task_state.warnings,
                &task_state.conversation,
                task_state.turns_executed,
                task_state.max_tool_loop_streak,
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
                runner_println!(
                    self,
                    "\n{} Agent Task Summary\n{}",
                    style("[📋]").cyan().bold(),
                    summary
                );
            }

            if !task_state.completion_outcome.is_success() {
                event_recorder.turn_failed(&task_state.completion_outcome.description());
            }

            event_recorder.turn_completed();
            let thread_events = event_recorder.into_events();

            // Return task results
            Ok(task_state.into_results(summary, thread_events, total_duration_ms))
        };

        self.tool_registry.set_harness_task(None);
        result
    }

    /// Execute a task with automatic retry on transient failures
    ///
    /// Wraps `execute_task` with retry logic using exponential backoff.
    /// Retries only occur for transient errors (timeouts, network issues, 5xx errors).
    /// Non-retryable errors (auth failures, invalid requests) fail immediately.
    pub async fn execute_task_with_retry(
        &mut self,
        task: &Task,
        contexts: &[ContextItem],
        max_retries: u32,
    ) -> Result<TaskResults> {
        use crate::core::orchestrator_retry::is_retryable_error;
        use tokio::time::{Duration, sleep};

        let mut delay_secs = 2u64;
        let max_delay_secs = 30u64;
        let backoff_multiplier = 2.0f64;

        for attempt in 0..=max_retries {
            info!(
                attempt = attempt + 1,
                max_attempts = max_retries + 1,
                task_id = %task.id,
                "agent task attempt starting"
            );

            match self.execute_task(task, contexts).await {
                Ok(result) => {
                    if attempt > 0 {
                        // Notify user about successful retry
                        runner_println!(
                            self,
                            "{} Task succeeded after {} attempt(s)",
                            style("[✓]").green().bold(),
                            attempt + 1
                        );

                        info!(
                            attempt = attempt + 1,
                            task_id = %task.id,
                            "agent task succeeded after retry"
                        );
                    }
                    return Ok(result);
                }
                Err(err) => {
                    warn!(
                        attempt = attempt + 1,
                        max_attempts = max_retries + 1,
                        task_id = %task.id,
                        error = %err,
                        "agent task attempt failed"
                    );

                    // Check if this error should be retried
                    if !is_retryable_error(&err) {
                        warn!(task_id = %task.id, error = %err, "non-retryable error");
                        return Err(err);
                    }

                    // If this is not the last attempt, wait before retrying
                    if attempt < max_retries {
                        let backoff_duration = Duration::from_secs(delay_secs);

                        // Notify user about retry with visible message
                        runner_println!(
                            self,
                            "{} Task failed (attempt {}/{}), retrying in {}s...",
                            style("[⚠️]").yellow().bold(),
                            attempt + 1,
                            max_retries + 1,
                            delay_secs
                        );

                        info!(
                            delay_secs,
                            next_attempt = attempt + 2,
                            task_id = %task.id,
                            "backing off before retry"
                        );

                        sleep(backoff_duration).await;

                        // Apply exponential backoff with cap
                        delay_secs = std::cmp::min(
                            (delay_secs as f64 * backoff_multiplier) as u64,
                            max_delay_secs,
                        );
                    } else {
                        // Last attempt failed
                        warn!(
                            task_id = %task.id,
                            attempts = max_retries + 1,
                            "agent task failed after all retries"
                        );

                        runner_println!(
                            self,
                            "{} Task failed after {} attempts",
                            style("[❌]").red().bold(),
                            max_retries + 1
                        );

                        return Err(anyhow!(
                            "Agent task '{}' failed after {} attempts: {}",
                            task.id,
                            max_retries + 1,
                            err
                        ));
                    }
                }
            }
        }

        // This should never be reached due to loop logic, but satisfy compiler
        unreachable!("Retry loop should always return within the loop")
    }

    /// Build available tools for this agent type
    async fn build_agent_tools(&self) -> Result<Vec<Tool>> {
        use crate::llm::providers::gemini::sanitize_function_parameters;

        // Build function declarations based on available tools
        let declarations = build_function_declarations();

        // Filter tools based on agent type and permissions
        let mut allowed_tools = Vec::with_capacity(declarations.len());
        for decl in declarations {
            if !self.is_tool_allowed(&decl.name).await {
                continue;
            }

            allowed_tools.push(Tool {
                function_declarations: vec![crate::gemini::FunctionDeclaration {
                    name: decl.name,
                    description: decl.description,
                    parameters: sanitize_function_parameters(decl.parameters),
                }],
            });
        }

        Ok(allowed_tools)
    }

}
