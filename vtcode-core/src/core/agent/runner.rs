//! Agent runner for executing individual agent instances

use crate::config::VTCodeConfig;
use crate::config::constants::{defaults, tools};
use crate::config::loader::ConfigManager;
use crate::config::models::{ModelId, Provider as ModelProvider};
use crate::config::types::ReasoningEffortLevel;
use crate::core::agent::conversation::{
    build_conversation, build_messages_from_conversation, compose_system_instruction,
};
use crate::core::agent::events::{EventSink, ExecEventRecorder};
pub use crate::core::agent::task::{ContextItem, Task, TaskOutcome, TaskResults};
use crate::core::agent::types::AgentType;
use crate::exec::events::{CommandExecutionStatus, ThreadEvent};
use crate::gemini::{Content, Part, Tool};
use crate::llm::factory::create_provider_for_model;
use crate::llm::provider as uni_provider;
use crate::llm::provider::{FunctionDefinition, LLMRequest, Message, ToolCall, ToolDefinition};
use crate::llm::{AnyClient, make_client};
use crate::mcp::McpClient;
use crate::prompts::system::compose_system_instruction_text;
use crate::tools::{ToolRegistry, build_function_declarations};
use crate::utils::colors::style;
use anyhow::{Result, anyhow};
use futures::StreamExt;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::time::{Duration, timeout};
use tracing::{info, warn};

macro_rules! runner_println {
    ($runner:expr, $($arg:tt)*) => {
        if !$runner.quiet {
            println!($($arg)*);
        }
    };
}

/// Format tool result for display in the TUI.
/// Limits verbose output from web_fetch to avoid overwhelming the terminal.
pub fn format_tool_result_for_display(tool_name: &str, result: &Value) -> String {
    if tool_name == tools::WEB_FETCH {
        // For web_fetch, show minimal info instead of the full content
        if let Some(obj) = result.as_object() {
            if obj.contains_key("error") {
                // Error case - show the error message only
                format!(
                    "Tool {} result: {{\"error\": {}}}",
                    tool_name,
                    obj.get("error")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unknown error".to_string())
                )
            } else {
                // Success case - show status info without the full content
                let status = serde_json::json!({
                    "status": "fetched",
                    "content_length": obj.get("content_length"),
                    "truncated": obj.get("truncated"),
                    "url": obj.get("url")
                });
                format!("Tool {} result: {}", tool_name, status.to_string())
            }
        } else {
            // Fallback if result structure is unexpected
            format!("Tool {} result: {}", tool_name, result.to_string())
        }
    } else {
        // For all other tools, show the full result
        format!("Tool {} result: {}", tool_name, result.to_string())
    }
}

fn record_turn_duration(
    turn_durations: &mut Vec<u128>,
    recorded: &mut bool,
    start: &std::time::Instant,
) {
    if !*recorded {
        turn_durations.push(start.elapsed().as_millis());
        *recorded = true;
    }
}

struct ProviderResponseSummary {
    response: crate::llm::provider::LLMResponse,
    content: String,
    reasoning: Option<String>,
    agent_message_streamed: bool,
    used_streaming_fallback: bool,
    reasoning_recorded: bool,
}

struct TaskRunState {
    conversation: Vec<Content>,
    conversation_messages: Vec<Message>,
    created_contexts: Vec<String>,
    modified_files: Vec<String>,
    executed_commands: Vec<String>,
    warnings: Vec<String>,
    has_completed: bool,
    completion_outcome: TaskOutcome,
    turns_executed: usize,
    turn_durations_ms: Vec<u128>,
    max_tool_loops: usize,
    consecutive_tool_loops: usize,
    max_tool_loop_streak: usize,
    tool_loop_limit_hit: bool,
}

impl TaskRunState {
    fn new(
        conversation: Vec<Content>,
        conversation_messages: Vec<Message>,
        max_tool_loops: usize,
    ) -> Self {
        Self {
            conversation,
            conversation_messages,
            created_contexts: Vec::new(),
            modified_files: Vec::new(),
            executed_commands: Vec::new(),
            warnings: Vec::new(),
            has_completed: false,
            completion_outcome: TaskOutcome::Unknown,
            turns_executed: 0,
            turn_durations_ms: Vec::new(),
            max_tool_loops,
            consecutive_tool_loops: 0,
            max_tool_loop_streak: 0,
            tool_loop_limit_hit: false,
        }
    }

    fn record_turn(&mut self, start: &std::time::Instant, recorded: &mut bool) {
        record_turn_duration(&mut self.turn_durations_ms, recorded, start);
    }

    fn finalize_outcome(&mut self, max_turns: usize) {
        if self.completion_outcome == TaskOutcome::Unknown {
            if self.has_completed {
                self.completion_outcome = TaskOutcome::Success;
            } else if self.tool_loop_limit_hit {
                self.completion_outcome = TaskOutcome::ToolLoopLimitReached;
            } else if self.turns_executed >= max_turns {
                self.completion_outcome = TaskOutcome::TurnLimitReached;
            }
        }
    }

    fn register_tool_loop(&mut self) -> usize {
        self.consecutive_tool_loops += 1;
        if self.consecutive_tool_loops > self.max_tool_loop_streak {
            self.max_tool_loop_streak = self.consecutive_tool_loops;
        }
        self.consecutive_tool_loops
    }

    fn reset_tool_loop_guard(&mut self) {
        self.consecutive_tool_loops = 0;
    }

    fn mark_tool_loop_limit_hit(&mut self) {
        self.tool_loop_limit_hit = true;
        self.completion_outcome = TaskOutcome::ToolLoopLimitReached;
    }

    fn into_results(
        self,
        summary: String,
        thread_events: Vec<ThreadEvent>,
        total_duration_ms: u128,
    ) -> TaskResults {
        let total_turn_duration_ms: u128 = self.turn_durations_ms.iter().sum();
        let average_turn_duration_ms = if !self.turn_durations_ms.is_empty() {
            Some(total_turn_duration_ms as f64 / self.turn_durations_ms.len() as f64)
        } else {
            None
        };
        let max_turn_duration_ms = self.turn_durations_ms.iter().copied().max();
        let completion_outcome = self.completion_outcome;

        TaskResults {
            created_contexts: self.created_contexts,
            modified_files: self.modified_files,
            executed_commands: self.executed_commands,
            summary,
            warnings: self.warnings,
            thread_events,
            outcome: completion_outcome,
            turns_executed: self.turns_executed,
            total_duration_ms,
            average_turn_duration_ms,
            max_turn_duration_ms,
            turn_durations_ms: self.turn_durations_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_turn_duration_records_once() {
        let mut durations = Vec::new();
        let mut recorded = false;
        let start = std::time::Instant::now();

        record_turn_duration(&mut durations, &mut recorded, &start);
        record_turn_duration(&mut durations, &mut recorded, &start);

        assert_eq!(durations.len(), 1);
    }

    #[test]
    fn finalize_outcome_marks_success() {
        let mut state = TaskRunState::new(Vec::new(), Vec::new(), 5);
        state.has_completed = true;
        state.turns_executed = 2;

        state.finalize_outcome(4);

        assert_eq!(state.completion_outcome, TaskOutcome::Success);
    }

    #[test]
    fn finalize_outcome_turn_limit() {
        let mut state = TaskRunState::new(Vec::new(), Vec::new(), 5);
        state.turns_executed = 6;

        state.finalize_outcome(6);

        assert_eq!(state.completion_outcome, TaskOutcome::TurnLimitReached);
    }

    #[test]
    fn finalize_outcome_tool_loop_limit() {
        let mut state = TaskRunState::new(Vec::new(), Vec::new(), 2);
        state.turns_executed = 2;
        state.tool_loop_limit_hit = true;

        state.finalize_outcome(10);

        assert_eq!(state.completion_outcome, TaskOutcome::ToolLoopLimitReached);
    }

    #[test]
    fn into_results_computes_metrics() {
        let mut state = TaskRunState::new(Vec::new(), Vec::new(), 5);
        state.turn_durations_ms = vec![100, 200, 300];
        state.turns_executed = 3;
        state.completion_outcome = TaskOutcome::Success;
        state.modified_files = vec!["file.rs".to_string()];
        state.executed_commands = vec!["write_file".to_string()];
        state.warnings = vec!["warning".to_string()];

        let total_duration_ms = 1_000u128;
        let results = state.into_results("summary".to_string(), Vec::new(), total_duration_ms);

        assert_eq!(results.outcome, TaskOutcome::Success);
        assert_eq!(results.turns_executed, 3);
        assert_eq!(results.total_duration_ms, total_duration_ms);
        assert_eq!(results.max_turn_duration_ms, Some(300));
        assert_eq!(results.average_turn_duration_ms, Some(200.0));
        assert_eq!(results.modified_files, vec!["file.rs".to_string()]);
        assert_eq!(results.executed_commands, vec!["write_file".to_string()]);
        assert_eq!(results.summary, "summary");
        assert_eq!(results.warnings, vec!["warning".to_string()]);
    }
}

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
    /// Model identifier
    model: String,
    /// API key (for provider client construction in future flows)
    _api_key: String,
    /// Reasoning effort level for models that support it
    reasoning_effort: Option<ReasoningEffortLevel>,
    /// Suppress stdout output when emitting structured events
    quiet: bool,
    /// Optional sink for streaming structured events
    event_sink: Option<EventSink>,
    /// Maximum number of autonomous turns before halting
    max_turns: usize,
}

impl AgentRunner {
    fn print_compact_response(agent: AgentType, text: &str, quiet: bool) {
        if quiet {
            return;
        }
        use crate::utils::colors::style;
        const MAX_CHARS: usize = 1200;
        const HEAD_CHARS: usize = 800;
        const TAIL_CHARS: usize = 200;
        let clean = text.trim();
        if clean.chars().count() <= MAX_CHARS {
            println!(
                "{} [{}]: {}",
                style("[RESPONSE]").cyan().bold(),
                agent,
                clean
            );
            return;
        }
        let mut out = String::new();
        let mut count = 0;
        for ch in clean.chars() {
            if count >= HEAD_CHARS {
                break;
            }
            out.push(ch);
            count += 1;
        }
        out.push_str("\n…\n");
        // tail
        let total = clean.chars().count();
        let start_tail = total.saturating_sub(TAIL_CHARS);
        let tail: String = clean.chars().skip(start_tail).collect();
        out.push_str(&tail);
        println!("{} [{}]: {}", style("[RESPONSE]").cyan().bold(), agent, out);
        println!(
            "{} truncated long response ({} chars).",
            style("[NOTE]").dim(),
            total
        );
    }
    /// Create informative progress message based on operation type
    fn create_progress_message(&self, operation: &str, details: Option<&str>) -> String {
        match operation {
            "thinking" => "Analyzing request and planning approach...".to_string(),
            "processing" => format!("Processing turn with {} model", self.client.model_id()),
            "tool_call" => {
                if let Some(tool) = details {
                    format!("Executing {} tool for task completion", tool)
                } else {
                    "Executing tool to gather information".to_string()
                }
            }
            "file_read" => {
                if let Some(file) = details {
                    format!("Reading {} to understand structure", file)
                } else {
                    "Reading file to analyze content".to_string()
                }
            }
            "file_write" => {
                if let Some(file) = details {
                    format!("Writing changes to {}", file)
                } else {
                    "Writing file with requested changes".to_string()
                }
            }
            "search" => {
                if let Some(pattern) = details {
                    format!("Searching codebase for '{}'", pattern)
                } else {
                    "Searching codebase for relevant information".to_string()
                }
            }
            "terminal" => {
                if let Some(cmd) = details {
                    format!(
                        "Running terminal command: {}",
                        cmd.split(' ').next().unwrap_or(cmd)
                    )
                } else {
                    "Executing terminal command".to_string()
                }
            }
            "completed" => "Task completed successfully!".to_string(),
            "error" => {
                if let Some(err) = details {
                    format!("Error encountered: {}", err)
                } else {
                    "An error occurred during execution".to_string()
                }
            }
            _ => format!("{}...", operation),
        }
    }

    async fn collect_provider_response(
        &mut self,
        request: &LLMRequest,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        warnings: &mut Vec<String>,
        turn_index: usize,
    ) -> Result<ProviderResponseSummary> {
        let supports_streaming = self.provider_client.supports_streaming();
        let mut agent_message_streamed = false;
        let mut used_streaming_fallback = false;
        let mut reasoning_recorded = false;
        let mut aggregated_text = String::new();
        let mut aggregated_reasoning = String::new();
        let mut streaming_response: Option<crate::llm::provider::LLMResponse> = None;

        if supports_streaming {
            match self.provider_client.stream(request.clone()).await {
                Ok(mut stream) => {
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(crate::llm::provider::LLMStreamEvent::Token { delta }) => {
                                if delta.is_empty() {
                                    continue;
                                }
                                aggregated_text.push_str(&delta);
                                if event_recorder.agent_message_stream_update(&aggregated_text) {
                                    agent_message_streamed = true;
                                }
                            }
                            Ok(crate::llm::provider::LLMStreamEvent::Reasoning { delta }) => {
                                aggregated_reasoning.push_str(&delta);
                            }
                            Ok(crate::llm::provider::LLMStreamEvent::Completed { response }) => {
                                streaming_response = Some(response);
                                break;
                            }
                            Err(err) => {
                                runner_println!(
                                    self,
                                    "{} {} Streaming error: {}",
                                    agent_prefix,
                                    style("(WARN)").yellow().bold(),
                                    err
                                );
                                let warning = format!("Streaming response interrupted: {}", err);
                                warnings.push(warning.clone());
                                event_recorder.warning(&warning);
                                if agent_message_streamed {
                                    event_recorder.agent_message_stream_complete();
                                }
                                used_streaming_fallback = agent_message_streamed;
                                break;
                            }
                        }
                    }
                }
                Err(err) => {
                    runner_println!(
                        self,
                        "{} {} Streaming fallback: {}",
                        agent_prefix,
                        style("(WARN)").yellow().bold(),
                        err
                    );
                    let warning = format!("Streaming request failed: {}", err);
                    warnings.push(warning.clone());
                    event_recorder.warning(&warning);
                    used_streaming_fallback = agent_message_streamed;
                }
            }
        }

        if let Some(mut response) = streaming_response {
            let response_text = response.content.clone().unwrap_or_default();
            if !response_text.is_empty() {
                aggregated_text = response_text.clone();
            }

            if !aggregated_text.trim().is_empty() {
                if event_recorder.agent_message_stream_update(&aggregated_text) {
                    agent_message_streamed = true;
                }
                if agent_message_streamed {
                    event_recorder.agent_message_stream_complete();
                }
            } else if agent_message_streamed {
                event_recorder.agent_message_stream_complete();
            }

            if !aggregated_reasoning.trim().is_empty() {
                event_recorder.reasoning(&aggregated_reasoning);
                reasoning_recorded = true;
                response.reasoning = Some(aggregated_reasoning.clone());
            } else if let Some(reasoning) = response.reasoning.clone() {
                event_recorder.reasoning(&reasoning);
                reasoning_recorded = true;
            }

            let reasoning = response.reasoning.clone();
            return Ok(ProviderResponseSummary {
                response,
                content: aggregated_text,
                reasoning,
                agent_message_streamed,
                used_streaming_fallback,
                reasoning_recorded,
            });
        }

        if agent_message_streamed {
            event_recorder.agent_message_stream_complete();
            used_streaming_fallback = true;
        }

        let mut fallback_request = request.clone();
        fallback_request.stream = false;

        let response = self
            .provider_client
            .generate(fallback_request)
            .await
            .map_err(|e| {
                runner_println!(
                    self,
                    "{} {} Failed",
                    agent_prefix,
                    style("(ERROR)").red().bold().on_black()
                );
                anyhow!(
                    "Agent {} execution failed at turn {}: {}",
                    self.agent_type,
                    turn_index,
                    e
                )
            })?;

        let content = response.content.clone().unwrap_or_default();
        let reasoning = response.reasoning.clone();

        Ok(ProviderResponseSummary {
            response,
            content,
            reasoning,
            agent_message_streamed,
            used_streaming_fallback,
            reasoning_recorded,
        })
    }

    /// Create a new agent runner
    pub async fn new(
        agent_type: AgentType,
        model: ModelId,
        api_key: String,
        workspace: PathBuf,
        session_id: String,
        reasoning_effort: Option<ReasoningEffortLevel>,
    ) -> Result<Self> {
        // Create client based on model
        let client: AnyClient = make_client(api_key.clone(), model.clone());

        // Create unified provider client for tool calling
        let provider_client = create_provider_for_model(model.as_str(), api_key.clone(), None)
            .map_err(|e| anyhow!("Failed to create provider client: {}", e))?;

        // Create system prompt for single agent, merging configuration and AGENTS.md hierarchy
        let system_prompt = match ConfigManager::load_from_workspace(&workspace) {
            Ok(manager) => {
                compose_system_instruction_text(workspace.as_path(), Some(manager.config())).await
            }
            Err(err) => {
                warn!("Failed to load vtcode configuration for system prompt composition: {err:#}");
                compose_system_instruction_text(workspace.as_path(), None).await
            }
        };

        Ok(Self {
            agent_type,
            client,
            provider_client,
            tool_registry: ToolRegistry::new(workspace.clone()).await,
            system_prompt,
            session_id,
            _workspace: workspace,
            model: model.as_str().to_string(),
            _api_key: api_key,
            reasoning_effort,
            quiet: false,
            event_sink: None,
            max_turns: defaults::DEFAULT_FULL_AUTO_MAX_TURNS,
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

    /// Apply workspace configuration to the tool registry, including tool policies and MCP setup.
    pub async fn apply_workspace_configuration(&mut self, vt_cfg: &VTCodeConfig) -> Result<()> {
        self.tool_registry.initialize_async().await?;

        self.tool_registry.apply_commands_config(&vt_cfg.commands);

        if let Err(err) = self
            .tool_registry
            .apply_config_policies(&vt_cfg.tools)
            .await
        {
            eprintln!(
                "Warning: Failed to apply tool policies from config: {}",
                err
            );
        }

        self.max_turns = vt_cfg.automation.full_auto.max_turns.max(1);

        if vt_cfg.mcp.enabled {
            let mut mcp_client = McpClient::new(vt_cfg.mcp.clone());

            // Validate configuration before initializing
            if let Err(e) = crate::mcp::validate_mcp_config(&vt_cfg.mcp) {
                warn!("MCP configuration validation error: {e}");
            }
            match timeout(Duration::from_secs(30), mcp_client.initialize()).await {
                Ok(Ok(())) => {
                    let mcp_client = Arc::new(mcp_client);
                    self.tool_registry.set_mcp_client(Arc::clone(&mcp_client));
                    if let Err(err) = self.tool_registry.refresh_mcp_tools().await {
                        warn!("Failed to refresh MCP tools: {}", err);
                    }
                }
                Ok(Err(err)) => {
                    warn!("MCP client initialization failed: {}", err);
                }
                Err(_) => {
                    warn!("MCP client initialization timed out after 30 seconds");
                }
            }
        }

        Ok(())
    }

    /// Execute a task with this agent
    pub async fn execute_task(
        &mut self,
        task: &Task,
        contexts: &[ContextItem],
    ) -> Result<TaskResults> {
        // Agent execution status
        let agent_prefix = format!("[{}]", self.agent_type);
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

        // Prepare conversation with task context
        let system_instruction = compose_system_instruction(&self.system_prompt, task, contexts);
        let conversation = build_conversation(task, contexts);

        // Build available tools for this agent
        let gemini_tools = self.build_agent_tools()?;

        // Maintain a mirrored conversation history for providers that expect
        // OpenAI/Anthropic style message roles.
        let conversation_messages =
            build_messages_from_conversation(&system_instruction, &conversation);

        // Convert Gemini tools to universal ToolDefinition format
        let tools: Vec<ToolDefinition> = gemini_tools
            .into_iter()
            .flat_map(|tool| tool.function_declarations)
            .map(|decl| ToolDefinition {
                tool_type: "function".to_string(),
                function: FunctionDefinition {
                    name: decl.name,
                    description: decl.description,
                    parameters: crate::llm::providers::gemini::sanitize_function_parameters(
                        decl.parameters,
                    ),
                },
            })
            .collect();

        // Track execution results
        // Determine max loops via configuration
        let cfg = ConfigManager::load()
            .or_else(|_| ConfigManager::load_from_workspace("."))
            .or_else(|_| ConfigManager::load_from_file("vtcode.toml"))
            .map(|cm| cm.config().clone())
            .unwrap_or_default();
        let max_tool_loops = cfg.tools.max_tool_loops.max(1);

        let mut task_state = TaskRunState::new(conversation, conversation_messages, max_tool_loops);

        // Agent execution loop uses max_turns for conversation flow
        for turn in 0..self.max_turns {
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

            let parallel_tool_config = if self
                .provider_client
                .supports_parallel_tool_config(&self.model)
            {
                Some(crate::llm::provider::ParallelToolConfig::anthropic_optimized())
            } else {
                None
            };

            let provider_kind = self
                .model
                .parse::<ModelId>()
                .map(|m| m.provider())
                .unwrap_or(ModelProvider::Gemini);

            let request_messages = if matches!(provider_kind, ModelProvider::Gemini) {
                let rebuilt =
                    build_messages_from_conversation(&system_instruction, &task_state.conversation);
                task_state.conversation_messages = rebuilt.clone();
                rebuilt
            } else {
                task_state.conversation_messages.clone()
            };

            let supports_streaming = self.provider_client.supports_streaming();

            // NOTE: Do NOT perform complex MessageContent introspection here.
            // WebFetch already returns a `next_action_hint` telling the model to analyze
            // `content` with `prompt`. The router-level model selection can be extended
            // separately to map such follow-ups to a small/fast model.
            let effective_model = self.model.clone();

            let request = LLMRequest {
                messages: request_messages,
                system_prompt: Some(system_instruction.clone()),
                tools: Some(tools.clone()),
                model: effective_model,
                max_tokens: Some(2000),
                temperature: Some(0.7),
                stream: supports_streaming,
                tool_choice: None,
                parallel_tool_calls: None,
                parallel_tool_config,
                reasoning_effort: if self.provider_client.supports_reasoning_effort(&self.model) {
                    self.reasoning_effort
                } else {
                    None
                },
            };

            // Use provider-specific client for OpenAI/Anthropic (and generic support for others)
            // Prepare for provider-specific vs Gemini handling
            #[allow(unused_assignments)]
            let mut response_opt: Option<crate::llm::types::LLMResponse> = None;

            if matches!(
                provider_kind,
                ModelProvider::OpenAI | ModelProvider::Anthropic | ModelProvider::DeepSeek
            ) {
                let ProviderResponseSummary {
                    response,
                    content: response_text,
                    reasoning,
                    agent_message_streamed,
                    used_streaming_fallback,
                    reasoning_recorded,
                } = self
                    .collect_provider_response(
                        &request,
                        &mut event_recorder,
                        &agent_prefix,
                        &mut task_state.warnings,
                        turn,
                    )
                    .await?;
                let resp = response;

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

                let mut had_tool_call = false;

                if !reasoning_recorded {
                    if let Some(reasoning) = reasoning.as_ref() {
                        event_recorder.reasoning(reasoning);
                    }
                }
                {
                    runner_println!(
                        self,
                        "{} {} received empty response with no tool calls",
                        agent_prefix,
                        style("(WARN)").yellow().bold()
                    );
                }

                const LOOP_DETECTED_MESSAGE: &str = "A potential loop was detected";
                if response_text.contains(LOOP_DETECTED_MESSAGE) {
                    if !response_text.trim().is_empty() {
                        Self::print_compact_response(self.agent_type, &response_text, self.quiet);
                        if agent_message_streamed {
                            if used_streaming_fallback {
                                event_recorder.agent_message(&response_text);
                            }
                        } else {
                            event_recorder.agent_message(&response_text);
                        }
                        task_state.conversation.push(Content {
                            role: "model".to_string(),
                            parts: vec![Part::Text {
                                text: response_text.clone(),
                            }],
                        });
                        task_state.conversation_messages.push(
                            Message::assistant(response_text.clone())
                                .with_reasoning(reasoning.clone()),
                        );
                    }

                    let warning_message =
                        "Provider halted execution after detecting a potential tool loop";
                    runner_println!(
                        self,
                        "{} {}",
                        agent_prefix,
                        format!("{} {}", style("(WARN)").yellow().bold(), warning_message)
                    );
                    task_state.warnings.push(warning_message.to_string());
                    event_recorder.warning(warning_message);
                    task_state.mark_tool_loop_limit_hit();
                    task_state.record_turn(&turn_started_at, &mut turn_recorded);
                    break;
                }

                let mut effective_tool_calls = resp.tool_calls.clone();

                if effective_tool_calls
                    .as_ref()
                    .map_or(true, |calls| calls.is_empty())
                {
                    if let Some(args_value) = resp
                        .content
                        .as_ref()
                        .and_then(|text| detect_textual_run_terminal_cmd(text))
                    {
                        let call_id = format!(
                            "textual_call_{}_{}",
                            turn,
                            task_state.conversation_messages.len()
                        );
                        let args_json = serde_json::to_string(&args_value)?;
                        effective_tool_calls = Some(vec![ToolCall::function(
                            call_id,
                            tools::RUN_COMMAND.to_string(),
                            args_json,
                        )]);
                    }
                }

                if let Some(tool_calls) = effective_tool_calls.as_ref() {
                    if !tool_calls.is_empty() {
                        had_tool_call = true;
                        let tool_calls_vec = tool_calls.clone();

                        task_state.conversation_messages.push(
                            Message::assistant_with_tools(
                                response_text.clone(),
                                tool_calls_vec.clone(),
                            )
                            .with_reasoning(reasoning.clone()),
                        );

                        for call in tool_calls_vec {
                            let name = call.function.name.clone();

                            runner_println!(
                                self,
                                "{} [{}] Calling tool: {}",
                                style("[TOOL_CALL]").blue().bold(),
                                self.agent_type,
                                name
                            );

                            let args = call
                                .parsed_arguments()
                                .unwrap_or_else(|_| serde_json::json!({}));

                            let command_event = event_recorder.command_started(&name);

                            match self.execute_tool(&name, &args).await {
                                Ok(result) => {
                                    runner_println!(
                                        self,
                                        "{} {}",
                                        agent_prefix,
                                        format!(
                                            "{} {} tool executed successfully",
                                            style("(OK)").green(),
                                            name
                                        )
                                    );

                                    let tool_result = serde_json::to_string(&result)?;
                                    // For display: use limited version to avoid overwhelming TUI
                                    let display_text =
                                        format_tool_result_for_display(&name, &result);
                                    task_state.conversation.push(Content {
                                        role: "user".to_string(),
                                        parts: vec![Part::Text { text: display_text }],
                                    });
                                    // For LLM: use full result
                                    task_state
                                        .conversation_messages
                                        .push(Message::tool_response(
                                            call.id.clone(),
                                            tool_result.clone(),
                                        ));

                                    task_state.executed_commands.push(name.to_string());
                                    event_recorder.command_finished(
                                        &command_event,
                                        CommandExecutionStatus::Completed,
                                        None,
                                        "",
                                    );

                                    if name == tools::WRITE_FILE {
                                        if let Some(filepath) =
                                            args.get("path").and_then(|p| p.as_str())
                                        {
                                            task_state.modified_files.push(filepath.to_string());
                                            event_recorder.file_change_completed(filepath);
                                        }
                                    }
                                }
                                Err(e) => {
                                    runner_println!(
                                        self,
                                        "{} {}",
                                        agent_prefix,
                                        format!(
                                            "{} {} tool failed: {}",
                                            style("(ERR)").red(),
                                            name,
                                            e
                                        )
                                    );
                                    let warning_message = format!("Tool {} failed: {}", name, e);
                                    task_state.warnings.push(warning_message.clone());
                                    event_recorder.command_finished(
                                        &command_event,
                                        CommandExecutionStatus::Failed,
                                        None,
                                        &warning_message,
                                    );
                                    event_recorder.warning(&warning_message);
                                    task_state.conversation.push(Content {
                                        role: "user".to_string(),
                                        parts: vec![Part::Text {
                                            text: format!("Tool {} failed: {}", name, e),
                                        }],
                                    });
                                    let error_payload =
                                        serde_json::json!({ "error": e.to_string() }).to_string();
                                    task_state
                                        .conversation_messages
                                        .push(Message::tool_response(
                                            call.id.clone(),
                                            error_payload,
                                        ));
                                }
                            }
                        }
                    }
                }

                if !had_tool_call {
                    if !response_text.trim().is_empty() {
                        Self::print_compact_response(self.agent_type, &response_text, self.quiet);
                        if agent_message_streamed {
                            if used_streaming_fallback {
                                event_recorder.agent_message(&response_text);
                            }
                        } else {
                            event_recorder.agent_message(&response_text);
                        }
                        task_state.conversation.push(Content {
                            role: "model".to_string(),
                            parts: vec![Part::Text {
                                text: response_text.clone(),
                            }],
                        });
                        task_state.conversation_messages.push(
                            Message::assistant(response_text.clone())
                                .with_reasoning(reasoning.clone()),
                        );
                    }
                }

                if !task_state.has_completed {
                    let response_lower = response_text.to_lowercase();
                    let completion_indicators = [
                        "task completed",
                        "task done",
                        "finished",
                        "complete",
                        "summary",
                        "i have successfully",
                        "i've completed",
                        "i have finished",
                        "task accomplished",
                        "mission accomplished",
                        "objective achieved",
                        "work is done",
                        "all done",
                        "completed successfully",
                        "task execution complete",
                        "operation finished",
                    ];
                    let is_completed = completion_indicators
                        .iter()
                        .any(|&indicator| response_lower.contains(indicator));
                    let has_explicit_completion = response_lower.contains("the task is complete")
                        || response_lower.contains("task has been completed")
                        || response_lower.contains("i am done")
                        || response_lower.contains("that's all")
                        || response_lower.contains("no more actions needed");
                    if is_completed || has_explicit_completion {
                        task_state.has_completed = true;
                        runner_println!(
                            self,
                            "{} {}",
                            agent_prefix,
                            format!(
                                "{} {} completed task successfully",
                                self.agent_type,
                                style("(SUCCESS)").green().bold()
                            )
                        );
                    }
                }

                let mut tool_loop_limit_triggered = false;
                if had_tool_call {
                    let loops = task_state.register_tool_loop();
                    if loops >= task_state.max_tool_loops {
                        let warning_message = format!(
                            "Reached tool-call limit of {} iterations; pausing autonomous loop",
                            task_state.max_tool_loops
                        );
                        runner_println!(
                            self,
                            "{} {}",
                            agent_prefix,
                            format!("{} {}", style("(WARN)").yellow().bold(), warning_message)
                        );
                        task_state.warnings.push(warning_message.clone());
                        event_recorder.warning(&warning_message);
                        task_state.mark_tool_loop_limit_hit();
                        task_state.record_turn(&turn_started_at, &mut turn_recorded);
                        tool_loop_limit_triggered = true;
                    }
                } else {
                    task_state.reset_tool_loop_guard();
                }

                if tool_loop_limit_triggered {
                    break;
                }

                let should_continue =
                    had_tool_call || (!task_state.has_completed && (turn + 1) < self.max_turns);
                if !should_continue {
                    if task_state.has_completed {
                        task_state.completion_outcome = TaskOutcome::Success;
                        runner_println!(
                            self,
                            "{} {}",
                            agent_prefix,
                            format!(
                                "{} {} finished - task completed",
                                self.agent_type,
                                style("(SUCCESS)").green().bold()
                            )
                        );
                    } else if (turn + 1) >= self.max_turns {
                        task_state.completion_outcome = TaskOutcome::TurnLimitReached;
                        runner_println!(
                            self,
                            "{} {}",
                            agent_prefix,
                            format!(
                                "{} {} finished - maximum turns reached",
                                self.agent_type,
                                style("(TIME)").yellow().bold()
                            )
                        );
                    } else {
                        task_state.completion_outcome = TaskOutcome::StoppedNoAction;
                        runner_println!(
                            self,
                            "{} {}",
                            agent_prefix,
                            format!(
                                "{} {} finished",
                                self.agent_type,
                                style("(FINISH)").blue().bold()
                            )
                        );
                    }
                    task_state.record_turn(&turn_started_at, &mut turn_recorded);
                    break;
                }

                task_state.record_turn(&turn_started_at, &mut turn_recorded);
                continue;
            } else {
                // Gemini path (existing flow)
                let response = self
                    .client
                    .generate(&serde_json::to_string(&request)?)
                    .await
                    .map_err(|e| {
                        runner_println!(
                            self,
                            "{} {} Failed",
                            agent_prefix,
                            style("(ERROR)").red().bold().on_black()
                        );
                        anyhow!(
                            "Agent {} execution failed at turn {}: {}",
                            self.agent_type,
                            turn,
                            e
                        )
                    })?;
                response_opt = Some(response);
            }

            // For Gemini path: use original response handling
            let response = response_opt.expect("response should be set for Gemini path");

            // Update progress for successful response
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

            // Use response content directly
            if !response.content.is_empty() {
                // Try to parse the response as JSON to check for tool calls
                let mut had_tool_call = false;

                // Try to parse as a tool call response
                if let Ok(tool_call_response) = serde_json::from_str::<Value>(&response.content) {
                    // Check for standard tool_calls format
                    if let Some(tool_calls) = tool_call_response
                        .get("tool_calls")
                        .and_then(|tc| tc.as_array())
                    {
                        had_tool_call = true;

                        // Process each tool call
                        for tool_call in tool_calls {
                            if let Some(function) = tool_call.get("function") {
                                if let (Some(name), Some(arguments)) = (
                                    function.get("name").and_then(|n| n.as_str()),
                                    function.get("arguments"),
                                ) {
                                    runner_println!(
                                        self,
                                        "{} [{}] Calling tool: {}",
                                        style("[TOOL_CALL]").blue().bold(),
                                        self.agent_type,
                                        name
                                    );

                                    // Execute the tool
                                    let command_event = event_recorder.command_started(name);
                                    match self.execute_tool(name, &arguments.clone()).await {
                                        Ok(result) => {
                                            runner_println!(
                                                self,
                                                "{} {}",
                                                agent_prefix,
                                                format!(
                                                    "{} {} tool executed successfully",
                                                    style("(OK)").green(),
                                                    name
                                                )
                                            );

                                            // Add tool result to conversation
                                            // For display: use limited version to avoid overwhelming TUI
                                            let display_text =
                                                format_tool_result_for_display(name, &result);
                                            task_state.conversation.push(Content {
                                                role: "user".to_string(), // Gemini API only accepts "user" and "model"
                                                parts: vec![Part::Text { text: display_text }],
                                            });

                                            // Track what the agent did
                                            task_state.executed_commands.push(name.to_string());
                                            event_recorder.command_finished(
                                                &command_event,
                                                CommandExecutionStatus::Completed,
                                                None,
                                                "",
                                            );

                                            // Special handling for certain tools
                                            if name == tools::WRITE_FILE {
                                                if let Some(filepath) =
                                                    arguments.get("path").and_then(|p| p.as_str())
                                                {
                                                    task_state
                                                        .modified_files
                                                        .push(filepath.to_string());
                                                    event_recorder.file_change_completed(filepath);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            runner_println!(
                                                self,
                                                "{} {}",
                                                agent_prefix,
                                                format!(
                                                    "{} {} tool failed: {}",
                                                    style("(ERR)").red(),
                                                    name,
                                                    e
                                                )
                                            );
                                            let warning_message =
                                                format!("Tool {} failed: {}", name, e);
                                            task_state.warnings.push(warning_message.clone());
                                            event_recorder.command_finished(
                                                &command_event,
                                                CommandExecutionStatus::Failed,
                                                None,
                                                &warning_message,
                                            );
                                            event_recorder.warning(&warning_message);
                                            task_state.conversation.push(Content {
                                                role: "user".to_string(), // Gemini API only accepts "user" and "model"
                                                parts: vec![Part::Text {
                                                    text: format!("Tool {} failed: {}", name, e),
                                                }],
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Check for Gemini functionCall format
                    else if let Some(function_call) = tool_call_response.get("functionCall") {
                        had_tool_call = true;

                        if let (Some(name), Some(args)) = (
                            function_call.get("name").and_then(|n| n.as_str()),
                            function_call.get("args"),
                        ) {
                            runner_println!(
                                self,
                                "{} [{}] Calling tool: {}",
                                style("[TOOL_CALL]").blue().bold(),
                                self.agent_type,
                                name
                            );

                            // Execute the tool
                            let command_event = event_recorder.command_started(name);
                            match self.execute_tool(name, args).await {
                                Ok(result) => {
                                    runner_println!(
                                        self,
                                        "{} {}",
                                        agent_prefix,
                                        format!(
                                            "{} {} tool executed successfully",
                                            style("(OK)").green(),
                                            name
                                        )
                                    );

                                    // Add tool result to conversation
                                    // For display: use limited version to avoid overwhelming TUI
                                    let display_text =
                                        format_tool_result_for_display(name, &result);
                                    task_state.conversation.push(Content {
                                        role: "user".to_string(), // Gemini API only accepts "user" and "model"
                                        parts: vec![Part::Text { text: display_text }],
                                    });

                                    // Track what the agent did
                                    task_state.executed_commands.push(name.to_string());
                                    event_recorder.command_finished(
                                        &command_event,
                                        CommandExecutionStatus::Completed,
                                        None,
                                        "",
                                    );

                                    // Special handling for certain tools
                                    if name == tools::WRITE_FILE {
                                        if let Some(filepath) =
                                            args.get("path").and_then(|p| p.as_str())
                                        {
                                            task_state.modified_files.push(filepath.to_string());
                                            event_recorder.file_change_completed(filepath);
                                        }
                                    }
                                }
                                Err(e) => {
                                    runner_println!(
                                        self,
                                        "{} {}",
                                        agent_prefix,
                                        format!(
                                            "{} {} tool failed: {}",
                                            style("(ERR)").red().bold(),
                                            name,
                                            e
                                        )
                                    );
                                    let warning_message = format!("Tool {} failed: {}", name, e);
                                    task_state.warnings.push(warning_message.clone());
                                    event_recorder.command_finished(
                                        &command_event,
                                        CommandExecutionStatus::Failed,
                                        None,
                                        &warning_message,
                                    );
                                    event_recorder.warning(&warning_message);
                                    task_state.conversation.push(Content {
                                        role: "user".to_string(), // Gemini API only accepts "user" and "model"
                                        parts: vec![Part::Text {
                                            text: format!("Tool {} failed: {}", name, e),
                                        }],
                                    });
                                }
                            }
                        }
                    }
                    // Check for tool_code format (what agents are actually producing)
                    else if let Some(tool_code) = tool_call_response
                        .get("tool_code")
                        .and_then(|tc| tc.as_str())
                    {
                        had_tool_call = true;

                        runner_println!(
                            self,
                            "{} [{}] Executing tool code: {}",
                            style("[TOOL_EXEC]").cyan().bold().on_black(),
                            self.agent_type,
                            tool_code
                        );

                        // Try to parse the tool_code as a function call
                        // This is a simplified parser for the format: function_name(args)
                        if let Some((func_name, args_str)) = parse_tool_code(tool_code) {
                            runner_println!(
                                self,
                                "{} [{}] Parsed tool: {} with args: {}",
                                style("[TOOL_PARSE]").yellow().bold().on_black(),
                                self.agent_type,
                                func_name,
                                args_str
                            );

                            // Parse arguments as JSON
                            match serde_json::from_str::<Value>(&args_str) {
                                Ok(arguments) => {
                                    // Execute the tool
                                    let command_event = event_recorder.command_started(&func_name);
                                    match self.execute_tool(&func_name, &arguments).await {
                                        Ok(result) => {
                                            runner_println!(
                                                self,
                                                "{} {}",
                                                agent_prefix,
                                                format!(
                                                    "{} {} tool executed successfully",
                                                    style("(OK)").green(),
                                                    func_name
                                                )
                                            );

                                            // Add tool result to conversation
                                            // For display: use limited version to avoid overwhelming TUI
                                            let display_text =
                                                format_tool_result_for_display(&func_name, &result);
                                            task_state.conversation.push(Content {
                                                role: "user".to_string(), // Gemini API only accepts "user" and "model"
                                                parts: vec![Part::Text { text: display_text }],
                                            });

                                            // Track what the agent did
                                            task_state
                                                .executed_commands
                                                .push(func_name.to_string());
                                            event_recorder.command_finished(
                                                &command_event,
                                                CommandExecutionStatus::Completed,
                                                None,
                                                "",
                                            );

                                            // Special handling for certain tools
                                            if func_name == tools::WRITE_FILE {
                                                if let Some(filepath) =
                                                    arguments.get("path").and_then(|p| p.as_str())
                                                {
                                                    task_state
                                                        .modified_files
                                                        .push(filepath.to_string());
                                                    event_recorder.file_change_completed(filepath);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            runner_println!(
                                                self,
                                                "{} {}",
                                                agent_prefix,
                                                format!(
                                                    "{} {} tool failed: {}",
                                                    style("(ERROR)").red().bold(),
                                                    func_name,
                                                    e
                                                )
                                            );
                                            let warning_message =
                                                format!("Tool {} failed: {}", func_name, e);
                                            task_state.warnings.push(warning_message.clone());
                                            event_recorder.command_finished(
                                                &command_event,
                                                CommandExecutionStatus::Failed,
                                                None,
                                                &warning_message,
                                            );
                                            event_recorder.warning(&warning_message);
                                            task_state.conversation.push(Content {
                                                role: "user".to_string(), // Gemini API only accepts "user" and "model"
                                                parts: vec![Part::Text {
                                                    text: format!(
                                                        "Tool {} failed: {}",
                                                        func_name, e
                                                    ),
                                                }],
                                            });
                                        }
                                    }
                                }
                                Err(e) => {
                                    let error_msg = format!(
                                        "Failed to parse tool arguments '{}': {}",
                                        args_str, e
                                    );
                                    event_recorder.warning(&error_msg);
                                    task_state.warnings.push(error_msg.clone());
                                    task_state.conversation.push(Content {
                                        role: "user".to_string(), // Gemini API only accepts "user" and "model"
                                        parts: vec![Part::Text { text: error_msg }],
                                    });
                                }
                            }
                        } else {
                            let error_msg = format!("Failed to parse tool code: {}", tool_code);
                            event_recorder.warning(&error_msg);
                            task_state.warnings.push(error_msg.clone());
                            task_state.conversation.push(Content {
                                role: "user".to_string(), // Gemini API only accepts "user" and "model"
                                parts: vec![Part::Text { text: error_msg }],
                            });
                        }
                    }
                    // Check for tool_name format (alternative format)
                    else if let Some(tool_name) = tool_call_response
                        .get("tool_name")
                        .and_then(|tn| tn.as_str())
                    {
                        had_tool_call = true;

                        runner_println!(
                            self,
                            "{} [{}] Calling tool: {}",
                            style("[TOOL_CALL]").blue().bold().on_black(),
                            self.agent_type,
                            tool_name
                        );

                        if let Some(parameters) = tool_call_response.get("parameters") {
                            // Execute the tool
                            let command_event = event_recorder.command_started(tool_name);
                            match self.execute_tool(tool_name, parameters).await {
                                Ok(result) => {
                                    runner_println!(
                                        self,
                                        "{} {}",
                                        agent_prefix,
                                        format!(
                                            "{} {} tool executed successfully",
                                            style("(SUCCESS)").green().bold(),
                                            tool_name
                                        )
                                    );

                                    // Add tool result to conversation
                                    // For display: use limited version to avoid overwhelming TUI
                                    let display_text =
                                        format_tool_result_for_display(tool_name, &result);
                                    task_state.conversation.push(Content {
                                        role: "user".to_string(), // Gemini API only accepts "user" and "model"
                                        parts: vec![Part::Text { text: display_text }],
                                    });

                                    // Track what the agent did
                                    task_state.executed_commands.push(tool_name.to_string());
                                    event_recorder.command_finished(
                                        &command_event,
                                        CommandExecutionStatus::Completed,
                                        None,
                                        "",
                                    );

                                    // Special handling for certain tools
                                    if tool_name == tools::WRITE_FILE {
                                        if let Some(filepath) =
                                            parameters.get("path").and_then(|p| p.as_str())
                                        {
                                            task_state.modified_files.push(filepath.to_string());
                                            event_recorder.file_change_completed(filepath);
                                        }
                                    }
                                }
                                Err(e) => {
                                    runner_println!(
                                        self,
                                        "{} {}",
                                        agent_prefix,
                                        format!(
                                            "{} {} tool failed: {}",
                                            style("(ERROR)").red().bold(),
                                            tool_name,
                                            e
                                        )
                                    );
                                    let warning_message =
                                        format!("Tool {} failed: {}", tool_name, e);
                                    task_state.warnings.push(warning_message.clone());
                                    event_recorder.command_finished(
                                        &command_event,
                                        CommandExecutionStatus::Failed,
                                        None,
                                        &warning_message,
                                    );
                                    event_recorder.warning(&warning_message);
                                    task_state.conversation.push(Content {
                                        role: "user".to_string(), // Gemini API only accepts "user" and "model"
                                        parts: vec![Part::Text {
                                            text: format!("Tool {} failed: {}", tool_name, e),
                                        }],
                                    });
                                }
                            }
                        }
                    } else {
                        // Regular content response
                        Self::print_compact_response(
                            self.agent_type,
                            response.content.trim(),
                            self.quiet,
                        );
                        event_recorder.agent_message(response.content.trim());
                        task_state.conversation.push(Content {
                            role: "model".to_string(),
                            parts: vec![Part::Text {
                                text: response.content.clone(),
                            }],
                        });
                    }
                } else {
                    // Regular text response
                    Self::print_compact_response(
                        self.agent_type,
                        response.content.trim(),
                        self.quiet,
                    );
                    event_recorder.agent_message(response.content.trim());
                    task_state.conversation.push(Content {
                        role: "model".to_string(),
                        parts: vec![Part::Text {
                            text: response.content.clone(),
                        }],
                    });
                }

                // Check for task completion indicators in the response
                if !task_state.has_completed {
                    let response_lower = response.content.to_lowercase();

                    // More comprehensive completion detection
                    let completion_indicators = [
                        "task completed",
                        "task done",
                        "finished",
                        "complete",
                        "summary",
                        "i have successfully",
                        "i've completed",
                        "i have finished",
                        "task accomplished",
                        "mission accomplished",
                        "objective achieved",
                        "work is done",
                        "all done",
                        "completed successfully",
                        "task execution complete",
                        "operation finished",
                    ];

                    // Check if any completion indicator is present
                    let is_completed = completion_indicators
                        .iter()
                        .any(|&indicator| response_lower.contains(indicator));

                    // Also check for explicit completion statements
                    let has_explicit_completion = response_lower.contains("the task is complete")
                        || response_lower.contains("task has been completed")
                        || response_lower.contains("i am done")
                        || response_lower.contains("that's all")
                        || response_lower.contains("no more actions needed");

                    if is_completed || has_explicit_completion {
                        task_state.has_completed = true;
                        runner_println!(
                            self,
                            "{} {}",
                            agent_prefix,
                            format!(
                                "{} {} completed task successfully",
                                self.agent_type,
                                style("(SUCCESS)").green().bold()
                            )
                        );
                    }
                }

                let mut tool_loop_limit_triggered = false;
                if had_tool_call {
                    let loops = task_state.register_tool_loop();
                    if loops >= task_state.max_tool_loops {
                        let warning_message = format!(
                            "Reached tool-call limit of {} iterations; pausing autonomous loop",
                            task_state.max_tool_loops
                        );
                        runner_println!(
                            self,
                            "{} {}",
                            agent_prefix,
                            format!("{} {}", style("(WARN)").yellow().bold(), warning_message)
                        );
                        task_state.warnings.push(warning_message.clone());
                        event_recorder.warning(&warning_message);
                        task_state.mark_tool_loop_limit_hit();
                        task_state.record_turn(&turn_started_at, &mut turn_recorded);
                        tool_loop_limit_triggered = true;
                    }
                } else {
                    task_state.reset_tool_loop_guard();
                }

                if tool_loop_limit_triggered {
                    break;
                }

                // Improved loop termination logic
                // Continue if: we had tool calls, task is not completed, and we haven't exceeded max turns
                let should_continue =
                    had_tool_call || (!task_state.has_completed && (turn + 1) < self.max_turns);

                if !should_continue {
                    if task_state.has_completed {
                        task_state.completion_outcome = TaskOutcome::Success;
                        runner_println!(
                            self,
                            "{} {}",
                            agent_prefix,
                            format!(
                                "{} {} finished - task completed",
                                self.agent_type,
                                style("(SUCCESS)").green().bold()
                            )
                        );
                    } else if (turn + 1) >= self.max_turns {
                        task_state.completion_outcome = TaskOutcome::TurnLimitReached;
                        runner_println!(
                            self,
                            "{} {}",
                            agent_prefix,
                            format!(
                                "{} {} finished - maximum turns reached",
                                self.agent_type,
                                style("(TIME)").yellow().bold()
                            )
                        );
                    } else {
                        task_state.completion_outcome = TaskOutcome::StoppedNoAction;
                        runner_println!(
                            self,
                            "{} {}",
                            agent_prefix,
                            format!(
                                "{} {} finished - no more actions needed",
                                self.agent_type,
                                style("(FINISH)").blue().bold()
                            )
                        );
                    }
                    task_state.record_turn(&turn_started_at, &mut turn_recorded);
                    break;
                }
            } else {
                // Empty response - check if we should continue or if task is actually complete
                if task_state.has_completed {
                    task_state.record_turn(&turn_started_at, &mut turn_recorded);
                    task_state.completion_outcome = TaskOutcome::Success;
                    runner_println!(
                        self,
                        "{} {}",
                        agent_prefix,
                        format!(
                            "{} {} finished - task was completed earlier",
                            self.agent_type,
                            style("(SUCCESS)").green().bold()
                        )
                    );
                    break;
                } else if (turn + 1) >= self.max_turns {
                    task_state.record_turn(&turn_started_at, &mut turn_recorded);
                    task_state.completion_outcome = TaskOutcome::TurnLimitReached;
                    runner_println!(
                        self,
                        "{} {}",
                        agent_prefix,
                        format!(
                            "{} {} finished - maximum turns reached with empty response",
                            self.agent_type,
                            style("(TIME)").yellow().bold()
                        )
                    );
                    break;
                } else {
                    // Empty response but task not complete - this might indicate an issue
                    runner_println!(
                        self,
                        "{} {}",
                        agent_prefix,
                        format!(
                            "{} {} received empty response, continuing...",
                            self.agent_type,
                            style("(EMPTY)").yellow()
                        )
                    );
                    // Don't break here, let the loop continue to give the agent another chance
                }
            }

            if !turn_recorded {
                task_state.record_turn(&turn_started_at, &mut turn_recorded);
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

        let summary = self.generate_task_summary(
            task,
            &task_state.modified_files,
            &task_state.executed_commands,
            &task_state.warnings,
            &task_state.conversation,
            task_state.turns_executed,
            task_state.max_tool_loop_streak,
            max_tool_loops,
            task_state.completion_outcome,
            total_duration_ms,
            average_turn_duration_ms,
            max_turn_duration_ms,
        );

        if !summary.trim().is_empty() {
            event_recorder.agent_message(&summary);
        }

        if !task_state.completion_outcome.is_success() {
            event_recorder.turn_failed(task_state.completion_outcome.description());
        }

        event_recorder.turn_completed();
        let thread_events = event_recorder.into_events();

        // Return task results
        Ok(task_state.into_results(summary, thread_events, total_duration_ms))
    }

    /// Build available tools for this agent type
    fn build_agent_tools(&self) -> Result<Vec<Tool>> {
        use crate::llm::providers::gemini::sanitize_function_parameters;

        // Build function declarations based on available tools
        let declarations = build_function_declarations();

        // Filter tools based on agent type and permissions
        let allowed_tools: Vec<Tool> = declarations
            .into_iter()
            .filter(|decl| self.is_tool_allowed(&decl.name))
            .map(|decl| Tool {
                function_declarations: vec![crate::gemini::FunctionDeclaration {
                    name: decl.name,
                    description: decl.description,
                    parameters: sanitize_function_parameters(decl.parameters),
                }],
            })
            .collect();

        Ok(allowed_tools)
    }

    /// Check if a tool is allowed for this agent
    fn is_tool_allowed(&self, tool_name: &str) -> bool {
        if let Ok(policy_manager) = self.tool_registry.policy_manager() {
            match policy_manager.get_policy(tool_name) {
                crate::tool_policy::ToolPolicy::Allow | crate::tool_policy::ToolPolicy::Prompt => {
                    true
                }
                crate::tool_policy::ToolPolicy::Deny => false,
            }
        } else {
            true
        }
    }

    /// Execute a tool by name with given arguments
    async fn execute_tool(&self, tool_name: &str, args: &Value) -> Result<Value> {
        // Enforce per-agent shell policies for RUN_COMMAND
        let is_shell = tool_name == tools::RUN_COMMAND;
        if is_shell {
            let cfg = ConfigManager::load()
                .or_else(|_| ConfigManager::load_from_workspace("."))
                .or_else(|_| ConfigManager::load_from_file("vtcode.toml"))
                .map(|cm| cm.config().clone())
                .unwrap_or_default();

            let cmd_text = if let Some(cmd_val) = args.get("command") {
                if cmd_val.is_array() {
                    cmd_val
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    cmd_val.as_str().unwrap_or("").to_string()
                }
            } else {
                String::new()
            };

            let agent_prefix = format!(
                "VTCODE_{}_COMMANDS_",
                self.agent_type.to_string().to_uppercase()
            );

            let mut deny_regex = cfg.commands.deny_regex.clone();
            if let Ok(extra) = std::env::var(format!("{}DENY_REGEX", agent_prefix)) {
                deny_regex.extend(extra.split(',').map(|s| s.trim().to_string()));
            }
            for pat in &deny_regex {
                if regex::Regex::new(pat)
                    .ok()
                    .map(|re| re.is_match(&cmd_text))
                    .unwrap_or(false)
                {
                    return Err(anyhow!("Shell command denied by regex: {}", pat));
                }
            }

            let mut deny_glob = cfg.commands.deny_glob.clone();
            if let Ok(extra) = std::env::var(format!("{}DENY_GLOB", agent_prefix)) {
                deny_glob.extend(extra.split(',').map(|s| s.trim().to_string()));
            }
            for pat in &deny_glob {
                let re = format!("^{}$", regex::escape(pat).replace(r"\*", ".*"));
                if regex::Regex::new(&re)
                    .ok()
                    .map(|re| re.is_match(&cmd_text))
                    .unwrap_or(false)
                {
                    return Err(anyhow!("Shell command denied by glob: {}", pat));
                }
            }
            info!(target = "policy", agent = ?self.agent_type, tool = tool_name, cmd = %cmd_text, "shell_policy_checked");
        }
        // Clone the tool registry for this execution
        let mut registry = self.tool_registry.clone();

        // Initialize async components
        registry.initialize_async().await?;

        // Try with simple adaptive retry (up to 2 retries)
        let mut delay = std::time::Duration::from_millis(200);
        for attempt in 0..3 {
            match registry.execute_tool(tool_name, args.clone()).await {
                Ok(result) => return Ok(result),
                Err(_e) if attempt < 2 => {
                    tokio::time::sleep(delay).await;
                    delay = delay.saturating_mul(2);
                    continue;
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Tool '{}' not found or failed to execute: {}",
                        tool_name,
                        e
                    ));
                }
            }
        }
        unreachable!()
    }

    /// Generate a meaningful summary of the task execution
    fn generate_task_summary(
        &self,
        task: &Task,
        modified_files: &[String],
        executed_commands: &[String],
        warnings: &[String],
        conversation: &[Content],
        turns_executed: usize,
        peak_tool_loops: usize,
        max_tool_loops: usize,
        outcome: TaskOutcome,
        total_duration_ms: u128,
        average_turn_duration_ms: Option<f64>,
        max_turn_duration_ms: Option<u128>,
    ) -> String {
        let mut summary = Vec::new();

        summary.push(format!("Task: {}", task.title));
        if !task.description.trim().is_empty() {
            summary.push(format!("Description: {}", task.description.trim()));
        }
        summary.push(format!("Agent Type: {:?}", self.agent_type));
        summary.push(format!("Session: {}", self.session_id));

        let reasoning_label = self
            .reasoning_effort
            .map(|effort| effort.to_string())
            .unwrap_or_else(|| "default".to_string());

        summary.push(format!(
            "Model: {} (provider: {}, reasoning: {})",
            self.client.model_id(),
            self.provider_client.name(),
            reasoning_label
        ));

        let tool_loops_used = peak_tool_loops;
        summary.push(format!(
            "Turns: {} used / {} max | Tool loops: {} used / {} max",
            turns_executed, self.max_turns, tool_loops_used, max_tool_loops
        ));

        let mut duration_line = format!("Duration: {} ms", total_duration_ms);
        let mut duration_metrics = Vec::new();
        if let Some(avg) = average_turn_duration_ms {
            duration_metrics.push(format!("avg {:.1} ms/turn", avg));
        }
        if let Some(max_turn) = max_turn_duration_ms {
            duration_metrics.push(format!("max {} ms", max_turn));
        }
        if !duration_metrics.is_empty() {
            duration_line.push_str(" (");
            duration_line.push_str(&duration_metrics.join(", "));
            duration_line.push(')');
        }
        summary.push(duration_line);

        let mut resolved_outcome = outcome;
        if matches!(resolved_outcome, TaskOutcome::Unknown) {
            if conversation.last().map_or(false, |c| {
                c.role == "model"
                    && c.parts.iter().any(|p| {
                        p.as_text().map_or(false, |t| {
                            t.contains("completed") || t.contains("done") || t.contains("finished")
                        })
                    })
            }) {
                resolved_outcome = TaskOutcome::Success;
            }
        }

        let mut status_line = format!("Final Status: {}", resolved_outcome.description());
        if !warnings.is_empty() && resolved_outcome.is_success() {
            status_line.push_str(" (with warnings)");
        }
        summary.push(status_line);
        summary.push(format!("Outcome Code: {}", resolved_outcome.code()));

        if !executed_commands.is_empty() {
            summary.push("Executed Commands:".to_string());
            for command in executed_commands {
                summary.push(format!(" - {}", command));
            }
        }

        if !modified_files.is_empty() {
            summary.push("Modified Files:".to_string());
            for file in modified_files {
                summary.push(format!(" - {}", file));
            }
        }

        if !warnings.is_empty() {
            summary.push("Warnings:".to_string());
            for warning in warnings {
                summary.push(format!(" - {}", warning));
            }
        }

        summary.join("\n")
    }
}

/// Parse tool code in the format: function_name(arg1=value1, arg2=value2)
fn parse_tool_code(tool_code: &str) -> Option<(String, String)> {
    // Remove any markdown code blocks
    let code = tool_code.trim();
    let code = if code.starts_with("```") && code.ends_with("```") {
        code.trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        code
    };

    // Try to match function call pattern: name(args)
    if let Some(open_paren) = code.find('(') {
        if let Some(close_paren) = code.rfind(')') {
            let func_name = code[..open_paren].trim().to_string();
            let args_str = &code[open_paren + 1..close_paren];

            // Convert Python-style arguments to JSON
            let json_args = convert_python_args_to_json(args_str)?;
            return Some((func_name, json_args));
        }
    }

    None
}

/// Convert Python-style function arguments to JSON
fn convert_python_args_to_json(args_str: &str) -> Option<String> {
    if args_str.trim().is_empty() {
        return Some("{}".to_string());
    }

    let mut json_parts = Vec::new();

    for arg in args_str.split(',').map(|s| s.trim()) {
        if arg.is_empty() {
            continue;
        }

        // Handle key=value format
        if let Some(eq_pos) = arg.find('=') {
            let key = arg[..eq_pos].trim().trim_matches('"').trim_matches('\'');
            let value = arg[eq_pos + 1..].trim();

            // Convert value to JSON format
            let json_value = if value.starts_with('"') && value.ends_with('"') {
                value.to_string()
            } else if value.starts_with('\'') && value.ends_with('\'') {
                format!("\"{}\"", value.trim_matches('\''))
            } else if value == "True" || value == "true" {
                "true".to_string()
            } else if value == "False" || value == "false" {
                "false".to_string()
            } else if value == "None" || value == "null" {
                "null".to_string()
            } else if let Ok(num) = value.parse::<f64>() {
                num.to_string()
            } else {
                // Assume it's a string that needs quotes
                format!("\"{}\"", value)
            };

            json_parts.push(format!("\"{}\": {}", key, json_value));
        } else {
            // Handle positional arguments (not supported well, but try)
            return None;
        }
    }

    Some(format!("{{{}}}", json_parts.join(", ")))
}

fn detect_textual_run_terminal_cmd(text: &str) -> Option<Value> {
    const FENCE_PREFIXES: [&str; 2] = ["```tool:run_terminal_cmd", "```run_terminal_cmd"];

    let (start_idx, prefix) = FENCE_PREFIXES
        .iter()
        .filter_map(|candidate| text.find(candidate).map(|idx| (idx, *candidate)))
        .min_by_key(|(idx, _)| *idx)?;

    // Require a fenced block owned by the model to avoid executing echoed examples.
    let mut remainder = &text[start_idx + prefix.len()..];
    if remainder.starts_with('\r') {
        remainder = &remainder[1..];
    }
    remainder = remainder.strip_prefix('\n')?;

    let fence_close = remainder.find("```")?;
    let block = remainder[..fence_close].trim();
    if block.is_empty() {
        return None;
    }

    let parsed = serde_json::from_str::<Value>(block)
        .or_else(|_| json5::from_str::<Value>(block))
        .ok()?;
    parsed.as_object()?;
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_web_fetch_success_result_limits_output() {
        let result = json!({
            "url": "https://example.com",
            "content_length": 50000,
            "truncated": false,
            "preview": "This is a preview of the content...",
            "content": "x".repeat(50000),  // Large content should not be shown
            "prompt": "Summarize this page"
        });

        let display = format_tool_result_for_display(tools::WEB_FETCH, &result);

        // Should contain status info
        assert!(display.contains("fetched"));
        assert!(display.contains("content_length"));

        // Should NOT contain the full content
        assert!(!display.contains(&"x".repeat(1000)));
    }

    #[test]
    fn test_format_web_fetch_error_result() {
        let result = json!({
            "error": "web_fetch: failed to fetch URL 'https://example.com': timeout"
        });

        let display = format_tool_result_for_display(tools::WEB_FETCH, &result);

        // Should contain the error message
        assert!(display.contains("error"));
        assert!(display.contains("timeout"));
    }

    #[test]
    fn test_format_other_tools_show_full_result() {
        let result = json!({
            "status": "ok",
            "data": "some data"
        });

        let display = format_tool_result_for_display("read_file", &result);

        // Should contain the full result for non-web_fetch tools
        assert!(display.contains("ok"));
        assert!(display.contains("some data"));
    }

    #[test]
    fn test_web_fetch_tool_name() {
        // Verify the constant exists and is correct
        assert_eq!(tools::WEB_FETCH, "web_fetch");
    }
}
