//! Agent runner for executing individual agent instances

use crate::config::VTCodeConfig;
use crate::config::constants::{defaults, tools};
use crate::config::loader::ConfigManager;
use crate::config::models::{ModelId, Provider as ModelProvider};
use crate::config::types::{ReasoningEffortLevel, VerbosityLevel};
use crate::core::agent::conversation::{
    build_conversation, build_messages_from_conversation, compose_system_instruction,
};
use crate::core::agent::events::{EventSink, ExecEventRecorder};
pub use crate::core::agent::task::{ContextItem, Task, TaskOutcome, TaskResults};
use crate::core::agent::types::AgentType;
use crate::core::loop_detector::LoopDetector;
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
use std::cell::RefCell;
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
    match tool_name {
        tools::WEB_FETCH => {
            // For web_fetch, show minimal info instead of the full content
            if let Some(obj) = result.as_object() {
                if obj.contains_key("error") {
                    format!(
                        "Tool {} result: {{\"error\": {}}}",
                        tool_name,
                        obj.get("error")
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "unknown error".into())
                    )
                } else {
                    let status = serde_json::json!({
                        "status": "fetched",
                        "content_length": obj.get("content_length"),
                        "truncated": obj.get("truncated"),
                        "url": obj.get("url")
                    });
                    format!("Tool {} result: {}", tool_name, status)
                }
            } else {
                format!("Tool {} result: {}", tool_name, result)
            }
        }
        tools::GREP_FILE => {
            // Show max 5 matches, indicate overflow
            if let Some(obj) = result.as_object() {
                if let Some(matches) = obj.get("matches").and_then(|v| v.as_array()) {
                    if matches.len() > 5 {
                        let truncated: Vec<_> = matches.iter().take(5).cloned().collect();
                        let overflow = matches.len() - 5;
                        let summary = serde_json::json!({
                            "matches": truncated,
                            "overflow": format!("[+{} more matches]", overflow),
                            "total": matches.len()
                        });
                        return format!("Tool {} result: {}", tool_name, summary);
                    }
                }
            }
            format!("Tool {} result: {}", tool_name, result)
        }
        tools::LIST_FILES => {
            // Summarize if 50+ items
            if let Some(obj) = result.as_object() {
                if let Some(files) = obj.get("files").and_then(|v| v.as_array()) {
                    if files.len() > 50 {
                        let sample: Vec<_> = files.iter().take(5).cloned().collect();
                        let summary = serde_json::json!({
                            "total_files": files.len(),
                            "sample": sample,
                            "note": format!("Showing 5 of {} files", files.len())
                        });
                        return format!("Tool {} result: {}", tool_name, summary);
                    }
                }
            }
            format!("Tool {} result: {}", tool_name, result)
        }
        tools::RUN_PTY_CMD | "shell" => {
            // Extract errors + 2 context lines for build output
            if let Some(obj) = result.as_object() {
                if let Some(stdout) = obj.get("stdout").and_then(|v| v.as_str()) {
                    if stdout.len() > 2000 && (stdout.contains("error") || stdout.contains("Error"))
                    {
                        let lines: Vec<&str> = stdout.lines().collect();
                        let mut extracted = Vec::new();
                        for (i, line) in lines.iter().enumerate() {
                            if line.to_lowercase().contains("error") {
                                let start = i.saturating_sub(2);
                                let end = (i + 3).min(lines.len());
                                extracted.extend_from_slice(&lines[start..end]);
                                extracted.push("...");
                            }
                        }
                        if !extracted.is_empty() {
                            let compact = serde_json::json!({
                                "exit_code": obj.get("exit_code"),
                                "errors": extracted.join("\n"),
                                "note": "Showing error lines + context only"
                            });
                            return format!("Tool {} result: {}", tool_name, compact);
                        }
                    }
                }
            }
            format!("Tool {} result: {}", tool_name, result)
        }
        _ => format!("Tool {} result: {}", tool_name, result),
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
    // Optimization: Track conversation length to avoid rebuilding messages unnecessarily
    last_conversation_len: usize,
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
            created_contexts: Vec::with_capacity(16), // Typical session creates ~5-10 contexts
            modified_files: Vec::with_capacity(32),   // Typical session modifies ~10-20 files
            executed_commands: Vec::with_capacity(64), // Typical session executes ~20-40 commands
            warnings: Vec::with_capacity(16),         // Typical session has ~5-10 warnings
            has_completed: false,
            completion_outcome: TaskOutcome::Unknown,
            turns_executed: 0,
            turn_durations_ms: Vec::with_capacity(max_tool_loops), // Pre-allocate for expected number of turns
            max_tool_loops,
            consecutive_tool_loops: 0,
            max_tool_loop_streak: 0,
            tool_loop_limit_hit: false,
            last_conversation_len: 0,
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
                self.completion_outcome = TaskOutcome::tool_loop_limit_reached(
                    self.max_tool_loops,
                    self.consecutive_tool_loops,
                );
            } else if self.turns_executed >= max_turns {
                self.completion_outcome =
                    TaskOutcome::turn_limit_reached(max_turns, self.turns_executed);
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
        self.completion_outcome =
            TaskOutcome::tool_loop_limit_reached(self.max_tool_loops, self.consecutive_tool_loops);
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
    use serde_json::json;

    #[test]
    fn record_turn_duration_records_once() {
        let mut durations = Vec::with_capacity(5); // Test only needs capacity for a few durations
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

        assert!(matches!(
            state.completion_outcome,
            TaskOutcome::TurnLimitReached { .. }
        ));
    }

    #[test]
    fn finalize_outcome_tool_loop_limit() {
        let mut state = TaskRunState::new(Vec::new(), Vec::new(), 2);
        state.turns_executed = 2;
        state.tool_loop_limit_hit = true;

        state.finalize_outcome(10);

        assert_eq!(
            state.completion_outcome,
            TaskOutcome::tool_loop_limit_reached(
                state.max_tool_loops,
                state.consecutive_tool_loops
            )
        );
    }

    #[test]
    fn into_results_computes_metrics() {
        let mut state = TaskRunState::new(Vec::new(), Vec::new(), 5);
        state.turn_durations_ms = vec![100, 200, 300];
        state.turns_executed = 3;
        state.completion_outcome = TaskOutcome::Success;
        state.modified_files = vec!["file.rs".to_owned()];
        state.executed_commands = vec!["write_file".to_owned()];
        state.warnings = vec!["warning".to_owned()];

        let total_duration_ms = 1_000u128;
        let results = state.into_results("summary".to_owned(), Vec::new(), total_duration_ms);

        assert_eq!(results.outcome, TaskOutcome::Success);
        assert_eq!(results.turns_executed, 3);
        assert_eq!(results.total_duration_ms, total_duration_ms);
        assert_eq!(results.max_turn_duration_ms, Some(300));
        assert_eq!(results.average_turn_duration_ms, Some(200.0));
        assert_eq!(results.modified_files, vec!["file.rs".to_owned()]);
        assert_eq!(results.executed_commands, vec!["write_file".to_owned()]);
        assert_eq!(results.summary, "summary");
        assert_eq!(results.warnings, vec!["warning".to_owned()]);
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
        for (count, ch) in clean.chars().enumerate() {
            if count >= HEAD_CHARS {
                break;
            }
            out.push(ch);
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
            "thinking" => "Analyzing request and planning approach...".to_owned(),
            "processing" => format!("Processing turn with {} model", self.client.model_id()),
            "tool_call" => {
                if let Some(tool) = details {
                    format!("Executing {} tool for task completion", tool)
                } else {
                    "Executing tool to gather information".to_owned()
                }
            }
            "file_read" => {
                if let Some(file) = details {
                    format!("Reading {} to understand structure", file)
                } else {
                    "Reading file to analyze content".to_owned()
                }
            }
            "file_write" => {
                if let Some(file) = details {
                    format!("Writing changes to {}", file)
                } else {
                    "Writing file with requested changes".to_owned()
                }
            }
            "search" => {
                if let Some(pattern) = details {
                    format!("Searching codebase for '{}'", pattern)
                } else {
                    "Searching codebase for relevant information".to_owned()
                }
            }
            "terminal" => {
                if let Some(cmd) = details {
                    format!(
                        "Running terminal command: {}",
                        cmd.split(' ').next().unwrap_or(cmd)
                    )
                } else {
                    "Executing terminal command".to_owned()
                }
            }
            "completed" => "Task completed successfully!".to_owned(),
            "error" => {
                if let Some(err) = details {
                    format!("Error encountered: {}", err)
                } else {
                    "An error occurred during execution".to_owned()
                }
            }
            _ => format!("{}...", operation),
        }
    }

    fn record_warning(
        &self,
        agent_prefix: &str,
        task_state: &mut TaskRunState,
        event_recorder: &mut ExecEventRecorder,
        warning_message: impl Into<String>,
    ) {
        let warning_message = warning_message.into();
        runner_println!(
            self,
            "{} {}",
            agent_prefix,
            format!("{} {}", style("(WARN)").yellow().bold(), warning_message)
        );
        event_recorder.warning(&warning_message);
        task_state.warnings.push(warning_message);
    }

    fn record_tool_failure(
        &self,
        agent_prefix: &str,
        task_state: &mut TaskRunState,
        event_recorder: &mut ExecEventRecorder,
        command_event: &crate::core::agent::events::ActiveCommandHandle,
        tool_name: &str,
        error: &anyhow::Error,
        tool_response_id: Option<&str>,
    ) {
        let failure_text = format!("Tool {} failed: {}", tool_name, error);
        runner_println!(
            self,
            "{} {}",
            agent_prefix,
            format!("{} {}", style("(ERR)").red().bold(), failure_text)
        );
        event_recorder.command_finished(
            command_event,
            CommandExecutionStatus::Failed,
            None,
            &failure_text,
        );
        event_recorder.warning(&failure_text);
        task_state.conversation.push(Content {
            role: "user".to_owned(),
            parts: vec![Part::Text {
                text: failure_text.clone(),
                thought_signature: None,
            }],
        });
        task_state.warnings.push(failure_text);
        if let Some(call_id) = tool_response_id {
            let error_payload = serde_json::json!({ "error": error.to_string() }).to_string();
            task_state
                .conversation_messages
                .push(Message::tool_response(call_id.to_owned(), error_payload));
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
        // Optimize: Pre-allocate with capacity to reduce reallocations during streaming
        // Typical response: 500-2000 chars, reasoning: 200-1000 chars
        let mut aggregated_text = String::with_capacity(2048);
        let mut aggregated_reasoning = String::with_capacity(1024);
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
                                event_recorder.warning(&warning);
                                warnings.push(warning);
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
                    event_recorder.warning(&warning);
                    warnings.push(warning);
                    used_streaming_fallback = agent_message_streamed;
                }
            }
        }

        if let Some(mut response) = streaming_response {
            let response_text = response.content.take().unwrap_or_default();
            if !response_text.is_empty() {
                aggregated_text = response_text;
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
                response.reasoning = Some(aggregated_reasoning);
            } else if let Some(ref reasoning) = response.reasoning {
                event_recorder.reasoning(reasoning);
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

        // Optimize: Create fallback request without cloning if possible
        // We only need to change stream=false, so we can reuse the request
        let fallback_request = LLMRequest {
            stream: false,
            ..request.clone()
        };

        let mut response = self
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

        let content = response.content.take().unwrap_or_default();
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
        verbosity: Option<VerbosityLevel>,
    ) -> Result<Self> {
        // Create client based on model
        let client: AnyClient = make_client(api_key.clone(), model);

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
            model: model.to_string(),
            _api_key: api_key,
            reasoning_effort,
            verbosity,
            quiet: false,
            event_sink: None,
            max_turns: defaults::DEFAULT_FULL_AUTO_MAX_TURNS,
            loop_detector: RefCell::new(LoopDetector::new()),
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
                tool_type: "function".to_owned(),
                function: Some(FunctionDefinition {
                    name: decl.name,
                    description: decl.description,
                    parameters: crate::llm::providers::gemini::sanitize_function_parameters(
                        decl.parameters,
                    ),
                }),
                shell: None,
                grammar: None,
                strict: None,
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

            // Optimize: Only rebuild messages for Gemini if conversation has changed
            if matches!(provider_kind, ModelProvider::Gemini)
                && task_state.conversation.len() != task_state.last_conversation_len
            {
                let rebuilt =
                    build_messages_from_conversation(&system_instruction, &task_state.conversation);
                task_state.conversation_messages = rebuilt;
                task_state.last_conversation_len = task_state.conversation.len();
            }

            let request_messages = task_state.conversation_messages.clone();

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
                output_format: None,
                tool_choice: None,
                parallel_tool_calls: None,
                parallel_tool_config,
                reasoning_effort: if self.provider_client.supports_reasoning_effort(&self.model) {
                    self.reasoning_effort
                } else {
                    None
                },
                verbosity: self.verbosity,
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
                let has_provider_tool_calls = resp
                    .tool_calls
                    .as_ref()
                    .is_some_and(|calls| !calls.is_empty());

                if !reasoning_recorded && let Some(reasoning) = reasoning.as_ref() {
                    event_recorder.reasoning(reasoning);
                }
                if response_text.trim().is_empty() && !has_provider_tool_calls {
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
                            role: "model".to_owned(),
                            parts: vec![Part::Text {
                                text: response_text.clone(),
                                thought_signature: None,
                            }],
                        });
                        task_state.conversation_messages.push(
                            Message::assistant(response_text.clone())
                                .with_reasoning(reasoning.clone()),
                        );
                    }

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

                let mut effective_tool_calls = resp.tool_calls;

                if effective_tool_calls
                    .as_ref()
                    .is_none_or(|calls| calls.is_empty())
                    && let Some(args_value) = resp
                        .content
                        .as_ref()
                        .and_then(|text| detect_textual_run_pty_cmd(text))
                {
                    let call_id = format!(
                        "textual_call_{}_{}",
                        turn,
                        task_state.conversation_messages.len()
                    );
                    let args_json = serde_json::to_string(&args_value)?;
                    effective_tool_calls = Some(vec![ToolCall::function(
                        call_id,
                        tools::RUN_PTY_CMD.to_owned(),
                        args_json,
                    )]);
                }

                if let Some(tool_calls) = effective_tool_calls.filter(|tc| !tc.is_empty()) {
                    had_tool_call = true;
                    let tool_calls_for_message = tool_calls.clone();

                    task_state.conversation_messages.push(
                        Message::assistant_with_tools(
                            response_text.clone(),
                            tool_calls_for_message,
                        )
                        .with_reasoning(reasoning.clone()),
                    );

                    // Determine if we can parallelize (read-only operations)
                    let can_parallelize = tool_calls.len() > 1
                        && tool_calls.iter().all(|call| {
                            if let Some(func) = &call.function {
                                matches!(
                                    func.name.as_str(),
                                    "list_files"
                                        | "read_file"
                                        | "grep_file"
                                        | "search_tools"
                                        | "get_errors"
                                )
                            } else {
                                false
                            }
                        });

                    if can_parallelize {
                        // Parallel execution path
                        use futures::future::join_all;

                        // Check loops for all calls first
                        let mut should_halt = false;
                        for call in &tool_calls {
                            let name = call
                                .function
                                .as_ref()
                                .expect("Tool call must have function")
                                .name
                                .clone();
                            let args = call
                                .parsed_arguments()
                                .unwrap_or_else(|_| serde_json::json!({}));

                            if let Some(warning) =
                                self.loop_detector.borrow_mut().record_call(&name, &args)
                            {
                                if self.loop_detector.borrow().is_hard_limit_exceeded(&name) {
                                    runner_println!(self, "{}", style(&warning).red().bold());
                                    task_state.warnings.push(warning.clone());
                                    task_state.conversation.push(Content {
                                        role: "user".to_owned(),
                                        parts: vec![Part::Text {
                                            text: warning,
                                            thought_signature: None,
                                        }],
                                    });
                                    task_state.has_completed = true;
                                    task_state.completion_outcome = TaskOutcome::LoopDetected;
                                    should_halt = true;
                                    break;
                                }
                                runner_println!(self, "{}", style(&warning).yellow().bold());
                                task_state.warnings.push(warning);
                            }
                        }

                        if should_halt {
                            break;
                        }

                        runner_println!(
                            self,
                            "{} [{}] Executing {} tools in parallel",
                            style("[PARALLEL]").cyan().bold(),
                            self.agent_type,
                            tool_calls.len()
                        );

                        let futures: Vec<_> = tool_calls
                            .iter()
                            .map(|call| {
                                let name = call
                                    .function
                                    .as_ref()
                                    .expect("Tool call must have function")
                                    .name
                                    .clone();
                                let args = call
                                    .parsed_arguments()
                                    .unwrap_or_else(|_| serde_json::json!({}));
                                let call_id = call.id.clone();
                                let tool_registry = self.tool_registry.clone();

                                // Check for loops before execution
                                let loop_warning =
                                    self.loop_detector.borrow_mut().record_call(&name, &args);

                                async move {
                                    if let Some(warning) = loop_warning {
                                        if warning.contains("CRITICAL")
                                            || warning.contains("HARD STOP")
                                        {
                                            return (
                                                name,
                                                args,
                                                call_id,
                                                Err(anyhow::anyhow!("{}", warning)),
                                            );
                                        }
                                        tracing::warn!("{}", warning);
                                    }

                                    let mut registry = tool_registry;
                                    registry.initialize_async().await.ok();
                                    let result =
                                        registry.execute_tool_ref(&name, &args).await.map_err(
                                            |e| anyhow::anyhow!("Tool '{}' failed: {}", name, e),
                                        );
                                    (name, args, call_id, result)
                                }
                            })
                            .collect();

                        let results = join_all(futures).await;

                        for (name, _args, call_id, result) in results {
                            let command_event = event_recorder.command_started(&name);

                            match result {
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
                                    let display_text =
                                        format_tool_result_for_display(&name, &result);
                                    task_state.conversation.push(Content {
                                        role: "user".to_owned(),
                                        parts: vec![Part::Text {
                                            text: display_text,
                                            thought_signature: None,
                                        }],
                                    });
                                    task_state
                                        .conversation_messages
                                        .push(Message::tool_response(call_id, tool_result));
                                    task_state.executed_commands.push(name.clone());
                                    event_recorder.command_finished(
                                        &command_event,
                                        CommandExecutionStatus::Completed,
                                        None,
                                        "",
                                    );
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

                                    let error_msg = format!("Error executing {}: {}", name, e);
                                    task_state.conversation.push(Content {
                                        role: "user".to_owned(),
                                        parts: vec![Part::Text {
                                            text: error_msg.clone(),
                                            thought_signature: None,
                                        }],
                                    });
                                    task_state
                                        .conversation_messages
                                        .push(Message::tool_response(call_id, error_msg));
                                    event_recorder.command_finished(
                                        &command_event,
                                        CommandExecutionStatus::Failed,
                                        None,
                                        &e.to_string(),
                                    );
                                }
                            }
                        }
                    } else {
                        // Sequential execution path (write operations or single call)
                        for call in tool_calls {
                            let name = call
                                .function
                                .as_ref()
                                .expect("Tool call must have function")
                                .name
                                .clone();

                            let args = call
                                .parsed_arguments()
                                .unwrap_or_else(|_| serde_json::json!({}));

                            // Check for loops before executing
                            if let Some(warning) =
                                self.loop_detector.borrow_mut().record_call(&name, &args)
                            {
                                // Check if hard limit exceeded
                                if self.loop_detector.borrow().is_hard_limit_exceeded(&name) {
                                    runner_println!(self, "{}", style(&warning).red().bold());
                                    task_state.warnings.push(warning.clone());

                                    // Add error to conversation and halt
                                    task_state.conversation.push(Content {
                                        role: "user".to_owned(),
                                        parts: vec![Part::Text {
                                            text: warning,
                                            thought_signature: None,
                                        }],
                                    });
                                    task_state.has_completed = true;
                                    task_state.completion_outcome = TaskOutcome::LoopDetected;
                                    break;
                                }

                                runner_println!(self, "{}", style(&warning).yellow().bold());
                                task_state.warnings.push(warning);
                            }

                            runner_println!(
                                self,
                                "{} [{}] Calling tool: {}",
                                style("[TOOL_CALL]").blue().bold(),
                                self.agent_type,
                                name
                            );

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
                                        role: "user".to_owned(),
                                        parts: vec![Part::Text {
                                            text: display_text,
                                            thought_signature: None,
                                        }],
                                    });
                                    // For LLM: use full result
                                    task_state
                                        .conversation_messages
                                        .push(Message::tool_response(call.id.clone(), tool_result));

                                    task_state.executed_commands.push(name.clone());
                                    event_recorder.command_finished(
                                        &command_event,
                                        CommandExecutionStatus::Completed,
                                        None,
                                        "",
                                    );

                                    if name == tools::WRITE_FILE
                                        && let Some(filepath) =
                                            args.get("path").and_then(|p| p.as_str())
                                    {
                                        task_state.modified_files.push(filepath.to_owned());
                                        event_recorder.file_change_completed(filepath);
                                    }
                                }
                                Err(e) => {
                                    self.record_tool_failure(
                                        &agent_prefix,
                                        &mut task_state,
                                        &mut event_recorder,
                                        &command_event,
                                        &name,
                                        &e,
                                        Some(call.id.as_str()),
                                    );
                                }
                            }
                        }
                    }
                }

                if !had_tool_call && !response_text.trim().is_empty() {
                    Self::print_compact_response(self.agent_type, &response_text, self.quiet);
                    if !agent_message_streamed || used_streaming_fallback {
                        event_recorder.agent_message(&response_text);
                    }
                    task_state.conversation.push(Content {
                        role: "model".to_owned(),
                        parts: vec![Part::Text {
                            text: response_text.clone(),
                            thought_signature: None,
                        }],
                    });
                    task_state.conversation_messages.push(
                        Message::assistant(response_text.clone()).with_reasoning(reasoning.clone()),
                    );
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
                        self.record_warning(
                            &agent_prefix,
                            &mut task_state,
                            &mut event_recorder,
                            warning_message,
                        );
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
                        task_state.completion_outcome =
                            TaskOutcome::turn_limit_reached(self.max_turns, turn + 1);
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
                        #[allow(clippy::collapsible_if)]
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
                                    match self.execute_tool(name, arguments).await {
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
                                                role: "user".to_owned(), // Gemini API only accepts "user" and "model"
                                                parts: vec![Part::Text {
                                                    text: display_text,
                                                    thought_signature: None,
                                                }],
                                            });

                                            // Track what the agent did
                                            task_state.executed_commands.push(name.to_owned());
                                            event_recorder.command_finished(
                                                &command_event,
                                                CommandExecutionStatus::Completed,
                                                None,
                                                "",
                                            );

                                            // Special handling for certain tools
                                            if name == tools::WRITE_FILE
                                                && let Some(filepath) =
                                                    arguments.get("path").and_then(|p| p.as_str())
                                            {
                                                task_state.modified_files.push(filepath.to_owned());
                                                event_recorder.file_change_completed(filepath);
                                            }
                                        }
                                        Err(e) => {
                                            self.record_tool_failure(
                                                &agent_prefix,
                                                &mut task_state,
                                                &mut event_recorder,
                                                &command_event,
                                                name,
                                                &e,
                                                None,
                                            );
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
                                        role: "user".to_owned(), // Gemini API only accepts "user" and "model"
                                        parts: vec![Part::Text {
                                            text: display_text,
                                            thought_signature: None,
                                        }],
                                    });

                                    // Track what the agent did
                                    task_state.executed_commands.push(name.to_owned());
                                    event_recorder.command_finished(
                                        &command_event,
                                        CommandExecutionStatus::Completed,
                                        None,
                                        "",
                                    );

                                    // Special handling for certain tools
                                    if name == tools::WRITE_FILE
                                        && let Some(filepath) =
                                            args.get("path").and_then(|p| p.as_str())
                                    {
                                        task_state.modified_files.push(filepath.to_owned());
                                        event_recorder.file_change_completed(filepath);
                                    }
                                }
                                Err(e) => {
                                    self.record_tool_failure(
                                        &agent_prefix,
                                        &mut task_state,
                                        &mut event_recorder,
                                        &command_event,
                                        name,
                                        &e,
                                        None,
                                    );
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
                                                role: "user".to_owned(), // Gemini API only accepts "user" and "model"
                                                parts: vec![Part::Text {
                                                    text: display_text,
                                                    thought_signature: None,
                                                }],
                                            });

                                            // Track what the agent did
                                            task_state.executed_commands.push(func_name.to_owned());
                                            event_recorder.command_finished(
                                                &command_event,
                                                CommandExecutionStatus::Completed,
                                                None,
                                                "",
                                            );

                                            // Special handling for certain tools
                                            if func_name == tools::WRITE_FILE
                                                && let Some(filepath) =
                                                    arguments.get("path").and_then(|p| p.as_str())
                                            {
                                                task_state.modified_files.push(filepath.to_owned());
                                                event_recorder.file_change_completed(filepath);
                                            }
                                        }
                                        Err(e) => {
                                            self.record_tool_failure(
                                                &agent_prefix,
                                                &mut task_state,
                                                &mut event_recorder,
                                                &command_event,
                                                &func_name,
                                                &e,
                                                None,
                                            );
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
                                        role: "user".to_owned(), // Gemini API only accepts "user" and "model"
                                        parts: vec![Part::Text {
                                            text: error_msg,
                                            thought_signature: None,
                                        }],
                                    });
                                }
                            }
                        } else {
                            let error_msg = format!("Failed to parse tool code: {}", tool_code);
                            event_recorder.warning(&error_msg);
                            task_state.warnings.push(error_msg.clone());
                            task_state.conversation.push(Content {
                                role: "user".to_owned(), // Gemini API only accepts "user" and "model"
                                parts: vec![Part::Text {
                                    text: error_msg,
                                    thought_signature: None,
                                }],
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
                                        role: "user".to_owned(), // Gemini API only accepts "user" and "model"
                                        parts: vec![Part::Text {
                                            text: display_text,
                                            thought_signature: None,
                                        }],
                                    });

                                    // Track what the agent did
                                    task_state.executed_commands.push(tool_name.to_owned());
                                    event_recorder.command_finished(
                                        &command_event,
                                        CommandExecutionStatus::Completed,
                                        None,
                                        "",
                                    );

                                    // Special handling for certain tools
                                    if tool_name == tools::WRITE_FILE
                                        && let Some(filepath) =
                                            parameters.get("path").and_then(|p| p.as_str())
                                    {
                                        task_state.modified_files.push(filepath.to_owned());
                                        event_recorder.file_change_completed(filepath);
                                    }
                                }
                                Err(e) => {
                                    self.record_tool_failure(
                                        &agent_prefix,
                                        &mut task_state,
                                        &mut event_recorder,
                                        &command_event,
                                        tool_name,
                                        &e,
                                        None,
                                    );
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
                            role: "model".to_owned(),
                            parts: vec![Part::Text {
                                text: response.content.clone(),
                                thought_signature: None,
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
                        role: "model".to_owned(),
                        parts: vec![Part::Text {
                            text: response.content.clone(),
                            thought_signature: None,
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
                        task_state.completion_outcome =
                            TaskOutcome::turn_limit_reached(self.max_turns, turn + 1);
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
                    task_state.completion_outcome =
                        TaskOutcome::turn_limit_reached(self.max_turns, turn + 1);
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
            event_recorder.agent_message(&summary);
        }

        if !task_state.completion_outcome.is_success() {
            event_recorder.turn_failed(&task_state.completion_outcome.description());
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
        // Enforce per-agent shell policies for RUN_PTY_CMD
        let is_shell = tool_name == tools::RUN_PTY_CMD;
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
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default()
                } else {
                    cmd_val.as_str().unwrap_or("").to_owned()
                }
            } else {
                String::new()
            };

            let agent_prefix = format!(
                "VTCODE_{}_COMMANDS_",
                self.agent_type.to_string().to_uppercase()
            );

            let mut deny_regex = cfg.commands.deny_regex;
            if let Ok(extra) = std::env::var(format!("{}DENY_REGEX", agent_prefix)) {
                deny_regex.extend(extra.split(',').map(|s| s.trim().to_owned()));
            }
            if !deny_regex.is_empty() {
                // Compile deny regexes once to avoid recompiling for each pattern match
                let compiled_deny: Vec<regex::Regex> = deny_regex
                    .iter()
                    .filter_map(|pat| regex::Regex::new(pat).ok())
                    .collect();
                for re in &compiled_deny {
                    if re.is_match(&cmd_text) {
                        return Err(anyhow!("Shell command denied by regex: {}", re.as_str()));
                    }
                }
            }

            let mut deny_glob = cfg.commands.deny_glob;
            if let Ok(extra) = std::env::var(format!("{}DENY_GLOB", agent_prefix)) {
                deny_glob.extend(extra.split(',').map(|s| s.trim().to_owned()));
            }
            if !deny_glob.is_empty() {
                // Compile glob-derived regexes once
                let compiled_globs: Vec<(String, regex::Regex)> = deny_glob
                    .iter()
                    .filter_map(|pat| {
                        let re = format!("^{}$", regex::escape(pat).replace(r"\*", ".*"));
                        regex::Regex::new(&re).ok().map(|r| (pat.clone(), r))
                    })
                    .collect();
                for (pat, re) in &compiled_globs {
                    if re.is_match(&cmd_text) {
                        return Err(anyhow!("Shell command denied by glob: {}", pat));
                    }
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
            match registry.execute_tool_ref(tool_name, args).await {
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
    #[allow(clippy::too_many_arguments)]
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
            .unwrap_or_else(|| "default".to_owned());

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
        if matches!(resolved_outcome, TaskOutcome::Unknown)
            && conversation.last().is_some_and(|c| {
                c.role == "model"
                    && c.parts.iter().any(|p| {
                        p.as_text().is_some_and(|t| {
                            t.contains("completed") || t.contains("done") || t.contains("finished")
                        })
                    })
            })
        {
            resolved_outcome = TaskOutcome::Success;
        }

        let mut status_line = format!("Final Status: {}", resolved_outcome.description());
        if !warnings.is_empty() && resolved_outcome.is_success() {
            status_line.push_str(" (with warnings)");
        }
        summary.push(status_line);
        summary.push(format!("Outcome Code: {}", resolved_outcome.code()));

        if !executed_commands.is_empty() {
            summary.push("Executed Commands:".to_owned());
            for command in executed_commands {
                summary.push(format!(" - {}", command));
            }
        }

        if !modified_files.is_empty() {
            summary.push("Modified Files:".to_owned());
            for file in modified_files {
                summary.push(format!(" - {}", file));
            }
        }

        if !warnings.is_empty() {
            summary.push("Warnings:".to_owned());
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
    if let Some(open_paren) = code.find('(')
        && let Some(close_paren) = code.rfind(')')
    {
        let func_name = code[..open_paren].trim().to_owned();
        let args_str = &code[open_paren + 1..close_paren];

        // Convert Python-style arguments to JSON
        let json_args = convert_python_args_to_json(args_str)?;
        return Some((func_name, json_args));
    }

    None
}

/// Convert Python-style function arguments to JSON
fn convert_python_args_to_json(args_str: &str) -> Option<String> {
    if args_str.trim().is_empty() {
        return Some("{}".to_owned());
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

            // Convert value to JSON format - properly escape string values
            let json_value = if value.starts_with('"') && value.ends_with('"') {
                // Already properly quoted, but ensure proper escaping
                serde_json::to_string(&value[1..value.len() - 1]).unwrap_or_else(|_| {
                    format!("\"{}\"", value[1..value.len() - 1].replace('"', "\\\""))
                })
            } else if value.starts_with('\'') && value.ends_with('\'') {
                // Convert single-quoted to double-quoted with proper escaping
                serde_json::to_string(&value[1..value.len() - 1]).unwrap_or_else(|_| {
                    format!("\"{}\"", value[1..value.len() - 1].replace('"', "\\\""))
                })
            } else if value == "True" || value == "true" {
                "true".to_owned()
            } else if value == "False" || value == "false" {
                "false".to_owned()
            } else if value == "None" || value == "null" {
                "null".to_owned()
            } else if let Ok(num) = value.parse::<f64>() {
                num.to_string()
            } else {
                // Assume it's a string that needs quotes - properly escape it
                serde_json::to_string(value)
                    .unwrap_or_else(|_| format!("\"{}\"", value.replace('"', "\\\"")))
            };

            json_parts.push(format!("\"{}\": {}", key, json_value));
        } else {
            // Handle positional arguments (not supported well, but try)
            return None;
        }
    }

    Some(format!("{{{}}}", json_parts.join(", ")))
}

fn detect_textual_run_pty_cmd(text: &str) -> Option<Value> {
    const FENCE_PREFIXES: [&str; 2] = ["```tool:run_pty_cmd", "```run_pty_cmd"];

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
