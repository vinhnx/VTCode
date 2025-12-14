//! Agent runner for executing individual agent instances

use crate::config::VTCodeConfig;
use crate::config::constants::{defaults, tools};
use crate::config::loader::ConfigManager;
use crate::config::models::{ModelId, Provider as ModelProvider};
use crate::config::types::{
    AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, VerbosityLevel,
};
use crate::core::agent::conversation::{
    build_conversation, build_messages_from_conversation, compose_system_instruction,
};
use crate::core::agent::events::{EventSink, ExecEventRecorder};
pub use crate::core::agent::task::{ContextItem, Task, TaskOutcome, TaskResults};
use crate::core::agent::types::AgentType;
use crate::core::context_optimizer::ContextOptimizer;
use crate::core::loop_detector::LoopDetector;
use crate::core::router::{RouteDecision, Router, TaskClass};
use crate::exec::events::{CommandExecutionStatus, ThreadEvent};
use crate::gemini::{Content, Part, Tool};
use crate::llm::factory::create_provider_for_model;
use crate::llm::provider as uni_provider;
use crate::llm::provider::{FunctionDefinition, LLMRequest, Message, ToolCall, ToolDefinition};
use crate::llm::{AnyClient, make_client};
use crate::mcp::McpClient;
use crate::prompts::system::compose_system_instruction_text;
use crate::tools::{ToolRegistry, build_function_declarations};
use crate::core::agent::state::{TaskRunState, ApiFailureTracker};

use crate::utils::colors::style;
use crate::utils::error_messages::ERR_TOOL_DENIED;
use anyhow::{Context, Result, anyhow};
use futures::StreamExt;
use serde_json::Value;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::time::{Duration, timeout};
use tracing::{info, warn};

// Role constants to avoid repeated allocations
const ROLE_USER: &str = "user";
const ROLE_MODEL: &str = "model";
const MAX_STREAMING_FAILURES: u8 = 2;
const LOOP_THROTTLE_BASE_MS: u64 = 75;
const LOOP_THROTTLE_MAX_MS: u64 = 500;
const STREAMING_COOLDOWN_SECS: u64 = 60;
const IDLE_TURN_LIMIT: usize = 3;

macro_rules! runner_println {
    ($runner:expr, $($arg:tt)*) => {
        if !$runner.quiet {
            println!($($arg)*);
        }
    };
}

#[derive(Clone)]
struct ShellPolicyCacheEntry {
    signature: u64,
    deny_regexes: Vec<(String, regex::Regex)>,
    deny_globs: Vec<(String, regex::Regex)>,
}

/// Format tool result for display in the TUI.
/// Limits verbose output from web_fetch to avoid overwhelming the terminal.
#[inline]
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
            if let Some(obj) = result.as_object()
                && let Some(matches) = obj.get("matches").and_then(|v| v.as_array())
                && matches.len() > 5
            {
                let truncated: Vec<_> = matches.iter().take(5).cloned().collect();
                let overflow = matches.len() - 5;
                let summary = serde_json::json!({
                    "matches": truncated,
                    "overflow": format!("[+{} more matches]", overflow),
                    "total": matches.len()
                });
                return format!("Tool {} result: {}", tool_name, summary);
            }
            format!("Tool {} result: {}", tool_name, result)
        }
        tools::LIST_FILES => {
            // Summarize if 50+ items
            if let Some(obj) = result.as_object()
                && let Some(files) = obj.get("files").and_then(|v| v.as_array())
                && files.len() > 50
            {
                let sample: Vec<_> = files.iter().take(5).cloned().collect();
                let summary = serde_json::json!({
                    "total_files": files.len(),
                    "sample": sample,
                    "note": format!("Showing 5 of {} files", files.len())
                });
                return format!("Tool {} result: {}", tool_name, summary);
            }
            format!("Tool {} result: {}", tool_name, result)
        }
        tools::RUN_PTY_CMD | "shell" => {
            // Extract errors + 2 context lines for build output
            if let Some(obj) = result.as_object()
                && let Some(stdout) = obj.get("stdout").and_then(|v| v.as_str())
                && stdout.len() > 2000
                && (stdout.contains("error") || stdout.contains("Error"))
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
            format!("Tool {} result: {}", tool_name, result)
        }
        _ => format!("Tool {} result: {}", tool_name, result),
    }
}

#[inline]
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

struct ToolFailureContext<'a> {
    agent_prefix: &'a str,
    task_state: &'a mut TaskRunState,
    event_recorder: &'a mut ExecEventRecorder,
    command_event: &'a crate::core::agent::events::ActiveCommandHandle,
}

#[cfg(test)]
mod tests {
    use super::*;

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
    shell_policy_cache: RefCell<Option<ShellPolicyCacheEntry>>,
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
    fn config(&self) -> &VTCodeConfig {
        self.config.as_ref()
    }

    fn core_agent_config(&self) -> CoreAgentConfig {
        let cfg = self.config();
        let checkpoint_dir = cfg
            .agent
            .checkpointing
            .storage_dir
            .as_ref()
            .map(|dir| self._workspace.join(dir));

        CoreAgentConfig {
            model: self.model.clone(),
            api_key: self._api_key.clone(),
            provider: cfg.agent.provider.clone(),
            api_key_env: cfg.agent.api_key_env.clone(),
            workspace: self._workspace.clone(),
            verbose: false,
            theme: cfg.agent.theme.clone(),
            reasoning_effort: self.reasoning_effort.unwrap_or(cfg.agent.reasoning_effort),
            ui_surface: cfg.agent.ui_surface,
            prompt_cache: cfg.prompt_cache.clone(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: cfg.agent.custom_api_keys.clone(),
            checkpointing_enabled: cfg.agent.checkpointing.enabled,
            checkpointing_storage_dir: checkpoint_dir,
            checkpointing_max_snapshots: cfg.agent.checkpointing.max_snapshots,
            checkpointing_max_age_days: cfg.agent.checkpointing.max_age_days,
        }
    }

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
            "thinking" => "Analyzing request and planning approach...".into(),
            "processing" => format!("Processing turn with {} model", self.client.model_id()),
            "tool_call" => {
                if let Some(tool) = details {
                    format!("Executing {} tool for task completion", tool)
                } else {
                    "Executing tool to gather information".into()
                }
            }
            "file_read" => {
                if let Some(file) = details {
                    format!("Reading {} to understand structure", file)
                } else {
                    "Reading file to analyze content".into()
                }
            }
            "file_write" => {
                if let Some(file) = details {
                    format!("Writing changes to {}", file)
                } else {
                    "Writing file with requested changes".into()
                }
            }
            "search" => {
                if let Some(pattern) = details {
                    format!("Searching codebase for '{}'", pattern)
                } else {
                    "Searching codebase for relevant information".into()
                }
            }
            "terminal" => {
                if let Some(cmd) = details {
                    format!(
                        "Running terminal command: {}",
                        cmd.split(' ').next().unwrap_or(cmd)
                    )
                } else {
                    "Executing terminal command".into()
                }
            }
            "completed" => "Task completed successfully!".into(),
            "error" => {
                if let Some(err) = details {
                    format!("Error encountered: {}", err)
                } else {
                    "An error occurred during execution".into()
                }
            }
            _ => format!("{}...", operation),
        }
    }

    fn summarize_conversation_if_needed(
        &self,
        system_instruction: &str,
        task_state: &mut TaskRunState,
        preserve_recent_turns: usize,
        utilization: f64,
    ) {
        if utilization < 0.90 {
            return;
        }

        if task_state.conversation.len() <= preserve_recent_turns {
            return;
        }

        let split_at = task_state
            .conversation
            .len()
            .saturating_sub(preserve_recent_turns);
        if split_at == 0 {
            return;
        }

        let summarize_list = |items: &[String]| -> String {
            const MAX_ITEMS: usize = 5;
            if items.is_empty() {
                return "none".into();
            }
            let shown: Vec<&str> = items.iter().take(MAX_ITEMS).map(|s| s.as_str()).collect();
            if items.len() > MAX_ITEMS {
                format!("{} [+{} more]", shown.join(", "), items.len() - MAX_ITEMS)
            } else {
                shown.join(", ")
            }
        };

        let summary = format!(
            "Summarized {} earlier turns to stay within context budget. Files: {}; Commands: {}; Warnings: {}.",
            split_at,
            summarize_list(&task_state.modified_files),
            summarize_list(&task_state.executed_commands),
            summarize_list(
                &task_state
                    .warnings
                    .iter()
                    .map(|w| w.to_string())
                    .collect::<Vec<_>>()
            ),
        );

        let mut new_conversation = Vec::with_capacity(1 + preserve_recent_turns);
        new_conversation.push(Content::user_parts(vec![Part::Text {
            text: summary,
            thought_signature: None,
        }]));
        new_conversation.extend_from_slice(&task_state.conversation[split_at..]);
        task_state.conversation = new_conversation;
        task_state.conversation_messages =
            build_messages_from_conversation(system_instruction, &task_state.conversation);
        task_state.last_processed_message_idx = task_state.conversation.len();
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
        failure_ctx: &mut ToolFailureContext<'_>,
        tool_name: &str,
        error: &anyhow::Error,
        tool_response_id: Option<&str>,
    ) {
        let failure_text = format!("Tool {} failed: {}", tool_name, error);
        runner_println!(
            self,
            "{} {}",
            failure_ctx.agent_prefix,
            format!("{} {}", style("(ERR)").red().bold(), failure_text)
        );
        failure_ctx.event_recorder.command_finished(
            failure_ctx.command_event,
            CommandExecutionStatus::Failed,
            None,
            &failure_text,
        );
        failure_ctx.event_recorder.warning(&failure_text);
        // Move failure_text into warnings first, then reference for conversation
        failure_ctx.task_state.warnings.push(failure_text.clone());
        failure_ctx.task_state.conversation.push(Content {
            role: ROLE_USER.into(),
            parts: vec![Part::Text {
                text: failure_text,
                thought_signature: None,
            }],
        });
        if let Some(call_id) = tool_response_id {
            let error_payload = serde_json::json!({ "error": error.to_string() }).to_string();
            failure_ctx
                .task_state
                .conversation_messages
                .push(Message::tool_response(call_id.to_owned(), error_payload));
        }
    }

    fn record_tool_denied(
        &self,
        agent_prefix: &str,
        task_state: &mut TaskRunState,
        event_recorder: &mut ExecEventRecorder,
        call_id: &str,
        tool_name: &str,
        command_event: Option<&crate::core::agent::events::ActiveCommandHandle>,
    ) {
        let detail = format!("{ERR_TOOL_DENIED}: {tool_name}");
        runner_println!(
            self,
            "{} {}",
            agent_prefix,
            format!("{} {}", style("(WARN)").yellow().bold(), detail)
        );
        task_state.warnings.push(detail.clone());
        task_state.conversation.push(Content {
            role: ROLE_USER.into(),
            parts: vec![Part::Text {
                text: detail.clone(),
                thought_signature: None,
            }],
        });
        let error_payload = serde_json::json!({ "error": detail }).to_string();
        task_state
            .conversation_messages
            .push(Message::tool_response(call_id.to_owned(), error_payload));

        if let Some(event) = command_event {
            event_recorder.command_finished(event, CommandExecutionStatus::Failed, None, &detail);
        } else {
            event_recorder.warning(&detail);
        }
    }

    async fn optimize_tool_result(&self, name: &str, result: Value) -> Value {
        let mut optimizer = {
            let mut opt_ref = self.context_optimizer.borrow_mut();
            std::mem::take(&mut *opt_ref)
        };

        let optimized = optimizer.optimize_result(name, result).await;

        let mut opt_ref = self.context_optimizer.borrow_mut();
        *opt_ref = optimizer;

        optimized
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
        let streaming_deadline = self
            .config()
            .timeouts
            .ceiling_duration(self.config().timeouts.streaming_ceiling_seconds);
        let generation_deadline = self
            .config()
            .timeouts
            .ceiling_duration(self.config().timeouts.default_ceiling_seconds);
        let mut streaming_disabled = false;
        if supports_streaming {
            if let Some(last_failure) = *self.streaming_last_failure.borrow()
                && last_failure.elapsed().as_secs() >= STREAMING_COOLDOWN_SECS
            {
                *self.streaming_failures.borrow_mut() = 0;
                self.streaming_last_failure.borrow_mut().take();
            }
            streaming_disabled = *self.streaming_failures.borrow() >= MAX_STREAMING_FAILURES;
        }
        let mut agent_message_streamed = false;
        let mut used_streaming_fallback = false;
        let mut reasoning_recorded = false;
        // Optimize: Pre-allocate with capacity to reduce reallocations during streaming
        // Typical response: 500-2000 chars, reasoning: 200-1000 chars
        let mut aggregated_text = String::with_capacity(2048);
        let mut aggregated_reasoning = String::with_capacity(1024);
        let mut streaming_response: Option<crate::llm::provider::LLMResponse> = None;

        if supports_streaming && !streaming_disabled {
            let stream_result = if let Some(limit) = streaming_deadline {
                tokio::time::timeout(limit, self.provider_client.stream(request.clone())).await
            } else {
                Ok(self.provider_client.stream(request.clone()).await)
            };

            match stream_result {
                Ok(Ok(mut stream)) => {
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
                                let mut failures = self.streaming_failures.borrow_mut();
                                *failures = failures.saturating_add(1);
                                self.streaming_last_failure.replace(Some(Instant::now()));
                                self.failure_tracker.borrow_mut().record_failure();
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
                Ok(Err(err)) => {
                    let mut failures = self.streaming_failures.borrow_mut();
                    *failures = failures.saturating_add(1);
                    self.streaming_last_failure.replace(Some(Instant::now()));
                    self.failure_tracker.borrow_mut().record_failure();
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
                Err(_) => {
                    let mut failures = self.streaming_failures.borrow_mut();
                    *failures = failures.saturating_add(1);
                    self.streaming_last_failure.replace(Some(Instant::now()));
                    self.failure_tracker.borrow_mut().record_failure();
                    let timeout_display = streaming_deadline
                        .map(|d| format!("{d:?}"))
                        .unwrap_or_else(|| "configured streaming timeout".to_string());
                    runner_println!(
                        self,
                        "{} {} Streaming timed out after {}",
                        agent_prefix,
                        style("(WARN)").yellow().bold(),
                        timeout_display
                    );
                    let warning = format!("Streaming request timed out after {}", timeout_display);
                    event_recorder.warning(&warning);
                    warnings.push(warning);
                    used_streaming_fallback = agent_message_streamed;
                }
            }
        } else if streaming_disabled {
            let warning = "Skipping streaming after repeated streaming failures";
            warnings.push(warning.to_string());
            event_recorder.warning(warning);
        }

        if let Some(mut response) = streaming_response {
            *self.streaming_failures.borrow_mut() = 0;
            self.streaming_last_failure.borrow_mut().take();
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
                // Ensure the agent reply is always visible even if the TUI misses streaming updates
                Self::print_compact_response(self.agent_type, &aggregated_text, self.quiet);
                runner_println!(
                    self,
                    "{} {}",
                    agent_prefix,
                    format!(
                        "{} {}",
                        style("(ASSISTANT)").green().bold(),
                        aggregated_text.trim()
                    )
                );
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

        // Check circuit breaker before fallback
        if self.failure_tracker.borrow().should_circuit_break() {
            let backoff = self.failure_tracker.borrow().backoff_duration();
            warn!(
                "Circuit breaker active after {} consecutive failures. Waiting {:?} before retry.",
                self.failure_tracker.borrow().consecutive_failures,
                backoff
            );
            tokio::time::sleep(backoff).await;
        }

        // Optimize: Create fallback request without cloning if possible
        // We only need to change stream=false, so we can reuse the request
        let fallback_request = LLMRequest {
            stream: false,
            ..request.clone()
        };

        let generation_result = if let Some(limit) = generation_deadline {
            tokio::time::timeout(limit, self.provider_client.generate(fallback_request)).await
        } else {
            Ok(self.provider_client.generate(fallback_request).await)
        };

        let mut response = match generation_result {
            Ok(Ok(resp)) => resp,
            Ok(Err(e)) => {
                // Record failure for exponential backoff
                self.failure_tracker.borrow_mut().record_failure();
                runner_println!(
                    self,
                    "{} {} Failed",
                    agent_prefix,
                    style("(ERROR)").red().bold().on_black()
                );
                return Err(anyhow!(
                    "Agent {} execution failed at turn {}: {}",
                    self.agent_type,
                    turn_index,
                    e
                ));
            }
            Err(_) => {
                self.failure_tracker.borrow_mut().record_failure();
                let warning = match generation_deadline {
                    Some(limit) => format!("LLM request timed out after {:?}", limit),
                    None => "LLM request timed out".to_string(),
                };
                event_recorder.warning(&warning);
                warnings.push(warning.clone());
                runner_println!(
                    self,
                    "{} {} {}",
                    agent_prefix,
                    style("(WARN)").yellow().bold(),
                    warning
                );
                return Err(anyhow!(
                    "Agent {} execution failed at turn {}: request timed out",
                    self.agent_type,
                    turn_index
                ));
            }
        };

        let content = response.content.take().unwrap_or_default();
        let reasoning = response.reasoning.clone();

        // Reset failure tracker on success
        self.failure_tracker.borrow_mut().reset();
        *self.streaming_failures.borrow_mut() = self.streaming_failures.borrow().saturating_sub(1);
        self.streaming_last_failure.borrow_mut().take();

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
        let client: AnyClient = make_client(api_key.clone(), model)?;

        // Create unified provider client for tool calling
        let provider_client = create_provider_for_model(model.as_str(), api_key.clone(), None)
            .map_err(|e| anyhow!("Failed to create provider client: {}", e))?;

        // Load configuration once to seed system prompt and runtime policies
        let (config_value, system_prompt) = match ConfigManager::load_from_workspace(&workspace) {
            Ok(manager) => {
                let cfg = manager.config().clone();
                let prompt = compose_system_instruction_text(workspace.as_path(), Some(&cfg)).await;
                (cfg, prompt)
            }
            Err(err) => {
                warn!("Failed to load vtcode configuration for system prompt composition: {err:#}");
                let cfg = VTCodeConfig::default();
                let prompt = compose_system_instruction_text(workspace.as_path(), None).await;
                (cfg, prompt)
            }
        };

        let max_repeated_tool_calls = config_value.tools.max_repeated_tool_calls.max(1);
        let config = Arc::new(config_value);
        let mut tool_registry = ToolRegistry::new(workspace.clone()).await;
        tool_registry.set_harness_session(session_id.clone());
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
            shell_policy_cache: RefCell::new(None),
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

    /// Apply workspace configuration to the tool registry, including tool policies and MCP setup.
    pub async fn apply_workspace_configuration(&mut self, vt_cfg: &VTCodeConfig) -> Result<()> {
        self.config = Arc::new(vt_cfg.clone());
        *self.loop_detector.borrow_mut() =
            LoopDetector::with_max_repeated_calls(self.config.tools.max_repeated_tool_calls.max(1));
        self.shell_policy_cache.borrow_mut().take();

        self.system_prompt =
            compose_system_instruction_text(self._workspace.as_path(), Some(self.config())).await;

        self.tool_registry.apply_timeout_policy(&vt_cfg.timeouts);
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

            // Prepare conversation with task context
            let system_instruction =
                compose_system_instruction(&self.system_prompt, task, contexts);
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
            // Determine loop guards via cached configuration
            let cfg = self.config();
            let max_tool_loops = cfg.tools.max_tool_loops.max(1);

            let mut task_state =
                TaskRunState::new(conversation, conversation_messages, max_tool_loops);
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
                // Check token budget before each turn
                let utilization = {
                    let token_budget = self.context_optimizer.borrow().token_budget();
                    if let Some(budget) = token_budget {
                        budget.usage_ratio().await
                    } else {
                        0.0
                    }
                };
                if utilization > 0.90 {
                    // At 90%+ utilization, warn and consider stopping
                    warn!(
                        "Token budget at {:.1}% - approaching limit",
                        utilization * 100.0
                    );
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

                // Model routing: choose per-turn model and reasoning effort
                let latest_user = task_state
                    .conversation
                    .iter()
                    .rev()
                    .find(|c| c.role == ROLE_USER)
                    .and_then(|c| {
                        c.parts.iter().find_map(|p| match p {
                            Part::Text { text, .. } => Some(text.as_str()),
                            _ => None,
                        })
                    })
                    .unwrap_or("");
                let trimmed_user: String = latest_user.chars().take(1000).collect();
                let route_input = format!(
                    "Task: {}\nDescription: {}\nInstructions: {}\nLatest user: {}",
                    task.title,
                    task.description,
                    task.instructions.clone().unwrap_or_default(),
                    trimmed_user
                );
                let cfg = self.config();
                let core_agent_cfg = self.core_agent_config();
                let mut route_decision: RouteDecision = if cfg.router.enabled {
                    Router::route(cfg, &core_agent_cfg, &route_input)
                } else {
                    RouteDecision {
                        class: TaskClass::Standard,
                        selected_model: self.model.clone(),
                    }
                };

                // Heuristic bias: prefer fast models for read/search-oriented turns
                let user_lower = trimmed_user.to_lowercase();
                let fast_hint = user_lower.contains("read")
                    || user_lower.contains("grep")
                    || user_lower.contains("search")
                    || user_lower.contains("list");
                if fast_hint
                    && matches!(
                        route_decision.class,
                        TaskClass::Standard | TaskClass::Complex
                    )
                {
                    route_decision.class = TaskClass::RetrievalHeavy;
                }

                if let Some(last_tool) = task_state.executed_commands.last() {
                    let lower = last_tool.to_lowercase();
                    let is_read_like =
                        lower.contains("read") || lower.contains("list") || lower.contains("grep");
                    let is_write_like = lower.contains("write")
                        || lower.contains("edit")
                        || lower.contains("patch");

                    if is_read_like && !matches!(route_decision.class, TaskClass::RetrievalHeavy) {
                        route_decision.class = TaskClass::RetrievalHeavy;
                    } else if is_write_like
                        && matches!(
                            route_decision.class,
                            TaskClass::Standard | TaskClass::RetrievalHeavy
                        )
                    {
                        route_decision.class = TaskClass::CodegenHeavy;
                    }
                }

                let (read_count, write_count) =
                    task_state.executed_commands.iter().rev().take(3).fold(
                        (0f64, 0f64),
                        |mut acc, cmd| {
                            let lower = cmd.to_lowercase();
                            let weight = if acc.0 + acc.1 == 0.0 {
                                3.0
                            } else if acc.0 + acc.1 < 2.0 {
                                2.0
                            } else {
                                1.0
                            };
                            if lower.contains("read")
                                || lower.contains("list")
                                || lower.contains("grep")
                            {
                                acc.0 += weight;
                            }
                            if lower.contains("write")
                                || lower.contains("edit")
                                || lower.contains("patch")
                            {
                                acc.1 += weight;
                            }
                            acc
                        },
                    );
                let user_len = trimmed_user.len();
                let mut read_score = read_count;
                let mut write_score = write_count;
                if user_len < 200 {
                    read_score *= 1.5;
                } else if user_len > 400 {
                    write_score *= 1.25;
                }

                if read_score > write_score
                    && !matches!(route_decision.class, TaskClass::RetrievalHeavy)
                {
                    route_decision.class = TaskClass::RetrievalHeavy;
                } else if write_score > read_score
                    && matches!(
                        route_decision.class,
                        TaskClass::Standard | TaskClass::RetrievalHeavy
                    )
                {
                    route_decision.class = TaskClass::CodegenHeavy;
                }

                let turn_model = if route_decision.selected_model.trim().is_empty() {
                    self.model.clone()
                } else {
                    route_decision.selected_model.clone()
                };

                let mut turn_reasoning = self.reasoning_effort;
                match route_decision.class {
                    TaskClass::Simple | TaskClass::RetrievalHeavy => {
                        turn_reasoning = Some(ReasoningEffortLevel::Low);
                    }
                    TaskClass::Complex | TaskClass::CodegenHeavy => {
                        turn_reasoning = Some(turn_reasoning.unwrap_or(ReasoningEffortLevel::High));
                    }
                    TaskClass::Standard => {}
                }

                // Context compaction before the request
                self.summarize_conversation_if_needed(
                    &system_instruction,
                    &mut task_state,
                    cfg.context.preserve_recent_turns,
                    utilization,
                );

                let parallel_tool_config = if matches!(
                    route_decision.class,
                    TaskClass::Simple | TaskClass::RetrievalHeavy
                ) {
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

                let request_messages = task_state.conversation_messages.clone();

                let supports_streaming = self.provider_client.supports_streaming();

                // NOTE: Do NOT perform complex MessageContent introspection here.
                // WebFetch already returns a `next_action_hint` telling the model to analyze
                // `content` with `prompt`. The router-level model selection can be extended
                // separately to map such follow-ups to a small/fast model.
                let request = LLMRequest {
                    messages: request_messages,
                    system_prompt: Some(system_instruction.clone()),
                    tools: Some(tools.clone()),
                    model: turn_model.clone(),
                    max_tokens: Some(2000),
                    temperature: Some(0.7),
                    stream: supports_streaming,
                    output_format: None,
                    tool_choice: None,
                    parallel_tool_calls: None,
                    parallel_tool_config,
                    reasoning_effort: if self.provider_client.supports_reasoning_effort(&turn_model)
                    {
                        turn_reasoning
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
                        mut agent_message_streamed,
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

                    if !response_text.trim().is_empty() && !agent_message_streamed {
                        event_recorder.agent_message(&response_text);
                        agent_message_streamed = true;
                        Self::print_compact_response(self.agent_type, &response_text, self.quiet);
                        runner_println!(
                            self,
                            "{} {}",
                            agent_prefix,
                            format!(
                                "{} {}",
                                style("(ASSISTANT)").green().bold(),
                                response_text.trim()
                            )
                        );
                    }

                    const LOOP_DETECTED_MESSAGE: &str = "A potential loop was detected";
                    if response_text.contains(LOOP_DETECTED_MESSAGE) {
                        if !response_text.trim().is_empty() {
                            Self::print_compact_response(
                                self.agent_type,
                                &response_text,
                                self.quiet,
                            );
                            if agent_message_streamed {
                                if used_streaming_fallback {
                                    event_recorder.agent_message(&response_text);
                                }
                            } else {
                                event_recorder.agent_message(&response_text);
                            }
                            task_state.conversation.push(Content {
                                role: ROLE_MODEL.into(),
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
                        // Clone tool_calls once for message, move original for processing
                        task_state.conversation_messages.push(
                            Message::assistant_with_tools(
                                response_text.clone(),
                                tool_calls.clone(),
                            )
                            .with_reasoning(reasoning.clone()),
                        );

                        // Determine if we can parallelize (read-only operations)
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
                            // Parallel execution path
                            use futures::future::join_all;

                            // Check loops for all calls first
                            let mut should_halt = false;
                            for call in &tool_calls {
                                let name = call
                                    .function
                                    .as_ref()
                                    .ok_or_else(|| {
                                        anyhow!("Tool call missing function definition")
                                    })?
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
                                            role: ROLE_USER.to_owned(),
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

                            // Pre-allocate futures with exact capacity
                            let mut futures = Vec::with_capacity(tool_calls.len());
                            for call in &tool_calls {
                                let name = match call.function.as_ref() {
                                    Some(func) => func.name.clone(),
                                    None => {
                                        warn!("Tool call missing function definition");
                                        continue;
                                    }
                                };
                                let args = call
                                    .parsed_arguments()
                                    .unwrap_or_else(|_| serde_json::json!({}));
                                let call_id = call.id.clone();

                                if !self.is_valid_tool(&name).await {
                                    self.record_tool_denied(
                                        &agent_prefix,
                                        &mut task_state,
                                        &mut event_recorder,
                                        &call_id,
                                        &name,
                                        None,
                                    );
                                    continue;
                                }

                                let tool_registry = self.tool_registry.clone();

                                futures.push({
                                    // OPTIMIZATION: Loop check already done before parallel execution
                                    async move {
                                        let mut registry = tool_registry;
                                        let result = registry
                                            .execute_tool_ref(&name, &args)
                                            .await
                                            .map_err(|e| {
                                                anyhow::anyhow!("Tool '{}' failed: {}", name, e)
                                            });
                                        (name, args, call_id, result)
                                    }
                                });
                            }

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

                                        // Optimize result through context optimizer (same as sequential path)
                                        let optimized_result =
                                            self.optimize_tool_result(&name, result).await;

                                        let tool_result = serde_json::to_string(&optimized_result)?;
                                        let display_text = format_tool_result_for_display(
                                            &name,
                                            &optimized_result,
                                        );
                                        task_state.conversation.push(Content {
                                            role: ROLE_USER.to_owned(),
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
                                        let error_msg = format!("Error executing {}: {}", name, e);
                                        runner_println!(
                                            self,
                                            "{} {}",
                                            agent_prefix,
                                            format!("{} {}", style("(ERR)").red(), error_msg)
                                        );
                                        task_state.conversation.push(Content {
                                            role: ROLE_USER.into(),
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
                                    .ok_or_else(|| {
                                        anyhow!("Tool call missing function definition")
                                    })?
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
                                            role: ROLE_USER.into(),
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

                                // Safety: Validate tool name before execution
                                if !self.is_valid_tool(&name).await {
                                    self.record_tool_denied(
                                        &agent_prefix,
                                        &mut task_state,
                                        &mut event_recorder,
                                        &call.id,
                                        &name,
                                        Some(&command_event),
                                    );
                                    continue;
                                }

                                let repeat_count =
                                    self.loop_detector.borrow().get_call_count(&name);
                                if repeat_count > 1 {
                                    let delay_ms = (LOOP_THROTTLE_BASE_MS * repeat_count as u64)
                                        .min(LOOP_THROTTLE_MAX_MS);
                                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                                }

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

                                        // Optimize tool result through context optimizer before sending to LLM
                                        let optimized_result =
                                            self.optimize_tool_result(&name, result).await;

                                        let tool_result = serde_json::to_string(&optimized_result)?;
                                        // For display: use limited version to avoid overwhelming TUI
                                        let display_text = format_tool_result_for_display(
                                            &name,
                                            &optimized_result,
                                        );
                                        task_state.conversation.push(Content {
                                            role: ROLE_USER.to_owned(),
                                            parts: vec![Part::Text {
                                                text: display_text,
                                                thought_signature: None,
                                            }],
                                        });
                                        // For LLM: use full result
                                        task_state.conversation_messages.push(
                                            Message::tool_response(call.id.clone(), tool_result),
                                        );

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
                                        let mut failure_ctx = ToolFailureContext {
                                            agent_prefix: &agent_prefix,
                                            task_state: &mut task_state,
                                            event_recorder: &mut event_recorder,
                                            command_event: &command_event,
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
                    }

                    if !had_tool_call && !response_text.trim().is_empty() {
                        Self::print_compact_response(self.agent_type, &response_text, self.quiet);
                        if !agent_message_streamed || used_streaming_fallback {
                            event_recorder.agent_message(&response_text);
                        }
                        task_state.conversation.push(Content {
                            role: ROLE_MODEL.into(),
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

                    if !task_state.has_completed {
                        // Use const to avoid repeated allocations
                        const COMPLETION_INDICATORS: &[&str] = &[
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
                        let response_lower = response_text.to_lowercase();
                        let is_completed = COMPLETION_INDICATORS
                            .iter()
                            .any(|&indicator| response_lower.contains(indicator));
                        let has_explicit_completion = response_lower
                            .contains("the task is complete")
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
                    // Force-print agent message to stdout/TUI even if streaming sinks miss it
                    Self::print_compact_response(
                        self.agent_type,
                        response.content.trim(),
                        self.quiet,
                    );
                    runner_println!(
                        self,
                        "{} {}",
                        agent_prefix,
                        format!(
                            "{} {}",
                            style("(ASSISTANT)").green().bold(),
                            response.content.trim()
                        )
                    );
                    // Try to parse the response as JSON to check for tool calls
                    let mut had_tool_call = false;

                    // Try to parse as a tool call response
                    if let Ok(tool_call_response) = serde_json::from_str::<Value>(&response.content)
                    {
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
                                                    role: ROLE_USER.to_owned(), // Gemini API only accepts "user" and "model"
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
                                                    && let Some(filepath) = arguments
                                                        .get("path")
                                                        .and_then(|p| p.as_str())
                                                {
                                                    task_state
                                                        .modified_files
                                                        .push(filepath.to_owned());
                                                    event_recorder.file_change_completed(filepath);
                                                }
                                            }
                                            Err(e) => {
                                                let mut failure_ctx = ToolFailureContext {
                                                    agent_prefix: &agent_prefix,
                                                    task_state: &mut task_state,
                                                    event_recorder: &mut event_recorder,
                                                    command_event: &command_event,
                                                };
                                                self.record_tool_failure(
                                                    &mut failure_ctx,
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
                                            role: ROLE_USER.to_owned(), // Gemini API only accepts "user" and "model"
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
                                        let mut failure_ctx = ToolFailureContext {
                                            agent_prefix: &agent_prefix,
                                            task_state: &mut task_state,
                                            event_recorder: &mut event_recorder,
                                            command_event: &command_event,
                                        };
                                        self.record_tool_failure(&mut failure_ctx, name, &e, None);
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
                                        let command_event =
                                            event_recorder.command_started(&func_name);
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
                                                let display_text = format_tool_result_for_display(
                                                    &func_name, &result,
                                                );
                                                task_state.conversation.push(Content {
                                                    role: ROLE_USER.to_owned(), // Gemini API only accepts "user" and "model"
                                                    parts: vec![Part::Text {
                                                        text: display_text,
                                                        thought_signature: None,
                                                    }],
                                                });

                                                // Track what the agent did
                                                task_state
                                                    .executed_commands
                                                    .push(func_name.to_owned());
                                                event_recorder.command_finished(
                                                    &command_event,
                                                    CommandExecutionStatus::Completed,
                                                    None,
                                                    "",
                                                );

                                                // Special handling for certain tools
                                                if func_name == tools::WRITE_FILE
                                                    && let Some(filepath) = arguments
                                                        .get("path")
                                                        .and_then(|p| p.as_str())
                                                {
                                                    task_state
                                                        .modified_files
                                                        .push(filepath.to_owned());
                                                    event_recorder.file_change_completed(filepath);
                                                }
                                            }
                                            Err(e) => {
                                                let mut failure_ctx = ToolFailureContext {
                                                    agent_prefix: &agent_prefix,
                                                    task_state: &mut task_state,
                                                    event_recorder: &mut event_recorder,
                                                    command_event: &command_event,
                                                };
                                                self.record_tool_failure(
                                                    &mut failure_ctx,
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
                                            role: ROLE_USER.to_owned(), // Gemini API only accepts "user" and "model"
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
                                    role: ROLE_USER.to_owned(), // Gemini API only accepts "user" and "model"
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
                                            role: ROLE_USER.to_owned(), // Gemini API only accepts "user" and "model"
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
                                        let mut failure_ctx = ToolFailureContext {
                                            agent_prefix: &agent_prefix,
                                            task_state: &mut task_state,
                                            event_recorder: &mut event_recorder,
                                            command_event: &command_event,
                                        };
                                        self.record_tool_failure(
                                            &mut failure_ctx,
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
                                role: ROLE_MODEL.to_owned(),
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
                            role: ROLE_MODEL.to_owned(),
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
                        let has_explicit_completion = response_lower
                            .contains("the task is complete")
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
        };

        self.tool_registry.set_harness_task(None);
        result
    }

    /// Build available tools for this agent type
    fn build_agent_tools(&self) -> Result<Vec<Tool>> {
        use crate::llm::providers::gemini::sanitize_function_parameters;

        // Build function declarations based on available tools
        let declarations = build_function_declarations();

        // Filter tools based on agent type and permissions
        let mut allowed_tools = Vec::with_capacity(declarations.len());
        for decl in declarations {
            if !self.is_tool_allowed(&decl.name) {
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

    /// Validate if a tool name is safe, registered, and allowed by policy
    #[inline]
    async fn is_valid_tool(&self, tool_name: &str) -> bool {
        // Normalize legacy alias for shell commands
        let canonical = if tool_name == "shell" {
            tools::RUN_PTY_CMD
        } else {
            tool_name
        };

        // Ensure the tool exists in the registry (including MCP tools)
        if !self.tool_registry.has_tool(canonical).await {
            return false;
        }

        // Enforce policy gate: Allow and Prompt are executable, Deny blocks
        if let Ok(policy_manager) = self.tool_registry.policy_manager() {
            return matches!(
                policy_manager.get_policy(canonical),
                crate::tool_policy::ToolPolicy::Allow | crate::tool_policy::ToolPolicy::Prompt
            );
        }

        true
    }

    /// Execute a tool by name with given arguments
    async fn execute_tool(&self, tool_name: &str, args: &Value) -> Result<Value> {
        // Fail fast if tool is denied or missing to avoid tight retry loops
        if !self.is_valid_tool(tool_name).await {
            return Err(anyhow!("{}: {}", ERR_TOOL_DENIED, tool_name));
        }

        // Enforce per-agent shell policies for RUN_PTY_CMD
        let is_shell = tool_name == tools::RUN_PTY_CMD;
        if is_shell {
            let cfg = self.config();
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

            let mut deny_regex_patterns: Vec<String> = cfg.commands.deny_regex.clone();
            if let Ok(extra) = std::env::var(format!("{}DENY_REGEX", agent_prefix)) {
                deny_regex_patterns.extend(extra.split(',').filter_map(|entry| {
                    let trimmed = entry.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_owned())
                }));
            }

            let mut deny_glob_patterns: Vec<String> = cfg.commands.deny_glob.clone();
            if let Ok(extra) = std::env::var(format!("{}DENY_GLOB", agent_prefix)) {
                deny_glob_patterns.extend(extra.split(',').filter_map(|entry| {
                    let trimmed = entry.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_owned())
                }));
            }

            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut hasher = DefaultHasher::new();
            deny_regex_patterns.hash(&mut hasher);
            deny_glob_patterns.hash(&mut hasher);
            let signature = hasher.finish();

            let cached_entry = {
                let cache_guard = self.shell_policy_cache.borrow();
                cache_guard
                    .as_ref()
                    .filter(|entry| entry.signature == signature)
                    .cloned()
            };

            let policy_entry = if let Some(entry) = cached_entry {
                entry
            } else {
                let deny_regexes = deny_regex_patterns
                    .iter()
                    .filter_map(|pattern| {
                        if pattern.is_empty() {
                            return None;
                        }
                        match regex::Regex::new(pattern) {
                            Ok(regex) => Some((pattern.clone(), regex)),
                            Err(err) => {
                                warn!(
                                    agent = ?self.agent_type,
                                    pattern,
                                    error = %err,
                                    "Invalid deny regex pattern skipped"
                                );
                                None
                            }
                        }
                    })
                    .collect::<Vec<_>>();

                let deny_globs = deny_glob_patterns
                    .iter()
                    .filter_map(|pattern| {
                        if pattern.is_empty() {
                            return None;
                        }
                        let re_pattern =
                            format!("^{}$", regex::escape(pattern).replace(r"\\*", ".*"));
                        match regex::Regex::new(&re_pattern) {
                            Ok(regex) => Some((pattern.clone(), regex)),
                            Err(err) => {
                                warn!(
                                    agent = ?self.agent_type,
                                    pattern,
                                    error = %err,
                                    "Invalid deny glob pattern skipped"
                                );
                                None
                            }
                        }
                    })
                    .collect::<Vec<_>>();

                let entry = ShellPolicyCacheEntry {
                    signature,
                    deny_regexes,
                    deny_globs,
                };
                self.shell_policy_cache.borrow_mut().replace(entry.clone());
                entry
            };

            for (pattern, compiled) in &policy_entry.deny_regexes {
                if compiled.is_match(&cmd_text) {
                    return Err(anyhow!("Shell command denied by regex: {}", pattern));
                }
            }

            for (pattern, compiled) in &policy_entry.deny_globs {
                if compiled.is_match(&cmd_text) {
                    return Err(anyhow!("Shell command denied by glob: {}", pattern));
                }
            }
            info!(target = "policy", agent = ?self.agent_type, tool = tool_name, cmd = %cmd_text, "shell_policy_checked");
        }
        // Clone the tool registry for this execution
        let mut registry = self.tool_registry.clone();

        // Initialize async components
        registry
            .initialize_async()
            .await
            .context("Failed to initialize tool registry before execution")?;

        // Try with simple adaptive retry (up to 2 retries)
        let mut delay = std::time::Duration::from_millis(200);
        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 0..3 {
            match registry.execute_tool_ref(tool_name, args).await {
                Ok(result) => return Ok(result),
                Err(e) if attempt < 2 => {
                    last_error = Some(e);
                    tokio::time::sleep(delay).await;
                    delay = delay.saturating_mul(2);
                    continue;
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }
        Err(anyhow!(
            "Tool '{}' failed after retries: {}",
            tool_name,
            last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown error".to_string())
        ))
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
