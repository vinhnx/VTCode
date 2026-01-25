//! Agent runner for executing individual agent instances

use crate::config::VTCodeConfig;
use crate::config::constants::{defaults, tools};
use crate::config::loader::ConfigManager;
use crate::config::models::{ModelId, Provider as ModelProvider};
use crate::config::types::{
    AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, SystemPromptMode,
    VerbosityLevel,
};
use crate::core::agent::completion::{check_completion_indicators, check_for_response_loop};
use crate::core::agent::conversation::{
    build_conversation, build_messages_from_conversation, compose_system_instruction,
};
use crate::core::agent::display::format_tool_result_for_display;
use crate::core::agent::events::{EventSink, ExecEventRecorder};
use crate::core::agent::state::{ApiFailureTracker, TaskRunState};
pub use crate::core::agent::task::{ContextItem, Task, TaskOutcome, TaskResults};
use crate::core::agent::types::AgentType;
use crate::core::context_optimizer::ContextOptimizer;
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
use crate::utils::error_messages::ERR_TOOL_DENIED;
use constants::{
    IDLE_TURN_LIMIT, LOOP_THROTTLE_BASE_MS, LOOP_THROTTLE_MAX_MS, MAX_STREAMING_FAILURES,
    ROLE_MODEL, ROLE_USER, STREAMING_COOLDOWN_SECS,
};
use helpers::detect_textual_run_pty_cmd;
use anyhow::{Context, Result, anyhow};
use futures::StreamExt;
use serde_json::Value;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::time::{Duration, timeout};
use tracing::{info, warn};
use types::{ProviderResponseSummary, ToolFailureContext};

mod constants;
mod helpers;
mod types;

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
    fn is_simple_task(task: &Task, contexts: &[ContextItem]) -> bool {
        let title_chars = task.title.chars().count();
        let description_chars = task.description.chars().count();
        let instructions_chars = task
            .instructions
            .as_ref()
            .map(|text| text.chars().count())
            .unwrap_or(0);
        let total_chars = title_chars + description_chars + instructions_chars;

        let title_words = task.title.split_whitespace().count();
        let description_words = task.description.split_whitespace().count();
        let instructions_words = task
            .instructions
            .as_ref()
            .map(|text| text.split_whitespace().count())
            .unwrap_or(0);
        let total_words = title_words + description_words + instructions_words;

        let context_chars: usize = contexts.iter().map(|ctx| ctx.content.chars().count()).sum();

        total_chars <= 240 && total_words <= 40 && contexts.len() <= 1 && context_chars <= 800
    }

    fn config(&self) -> &VTCodeConfig {
        self.config.as_ref()
    }

    #[allow(dead_code)]
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
            quiet: self.quiet,
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
            max_conversation_turns: 50,
        }
    }

    fn print_compact_response(agent: &AgentType, text: &str, quiet: bool) {
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

        let preferred_split_at = task_state
            .conversation
            .len()
            .saturating_sub(preserve_recent_turns);

        // Context Manager: Find a safe split point that doesn't break tool call/output pairs.
        let split_at = task_state.find_safe_split_point(preferred_split_at);

        if split_at == 0 {
            return;
        }

        // Dynamic context discovery: Write full history to file before summarization
        // This allows the agent to recover details via grep_file if needed
        let history_file_path = self.persist_history_before_summarization(
            &task_state.conversation[..split_at],
            task_state.turns_executed,
            &task_state.modified_files,
            &task_state.executed_commands,
        );

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

        let base_summary = format!(
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

        // Include history file reference in summary if available
        let summary = if let Some(path) = history_file_path {
            format!(
                "{}\n\nFull conversation history saved to: {}\nUse grep_file to search for specific details if needed.",
                base_summary,
                path.display()
            )
        } else {
            base_summary
        };

        let mut new_conversation = Vec::with_capacity(1 + preserve_recent_turns);
        new_conversation.push(Content::user_parts(vec![Part::Text {
            text: summary,
            thought_signature: None,
        }]));
        new_conversation.extend_from_slice(&task_state.conversation[split_at..]);
        task_state.conversation = new_conversation;
        task_state.conversation_messages =
            build_messages_from_conversation(system_instruction, &task_state.conversation);

        // Context Manager: Ensure history invariants are maintained after summarization.
        task_state.normalize();

        task_state.last_processed_message_idx = task_state.conversation.len();
    }

    /// Persist conversation history to a file before summarization
    ///
    /// This implements Cursor-style dynamic context discovery: full history
    /// is written to `.vtcode/history/` so the agent can recover details
    /// via grep_file if the summary loses important information.
    fn persist_history_before_summarization(
        &self,
        conversation: &[Content],
        turn_number: usize,
        modified_files: &[String],
        executed_commands: &[String],
    ) -> Option<std::path::PathBuf> {
        use crate::context::history_files::{HistoryFileManager, content_to_history_messages};

        // Create history manager for this session
        let mut manager = HistoryFileManager::new(&self._workspace, &self.session_id);

        // Convert conversation to history messages
        let messages = content_to_history_messages(conversation, 0);

        // Write history file
        match manager.write_history_sync(
            &messages,
            turn_number,
            "summarization",
            modified_files,
            executed_commands,
        ) {
            Ok(result) => {
                info!(
                    path = %result.file_path.display(),
                    messages = result.metadata.message_count,
                    "Persisted conversation history before summarization"
                );
                Some(result.file_path)
            }
            Err(e) => {
                warn!(error = %e, "Failed to persist conversation history before summarization");
                None
            }
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

        if let Some(call_id) = tool_response_id {
            failure_ctx.task_state.push_tool_error(
                call_id.to_string(),
                tool_name,
                failure_text,
                failure_ctx.is_gemini,
            );
        } else {
            // Fallback for when we don't have a call_id (should be rare in Codex-style)
            failure_ctx.task_state.conversation.push(Content {
                role: ROLE_USER.into(),
                parts: vec![Part::Text {
                    text: failure_text,
                    thought_signature: None,
                }],
            });
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
        is_gemini: bool,
    ) {
        let detail = format!("{ERR_TOOL_DENIED}: {tool_name}");
        runner_println!(
            self,
            "{} {}",
            agent_prefix,
            format!("{} {}", style("(WARN)").yellow().bold(), detail)
        );
        task_state.warnings.push(detail.clone());

        task_state.push_tool_error(call_id.to_string(), tool_name, detail.clone(), is_gemini);

        if let Some(event) = command_event {
            event_recorder.command_finished(event, CommandExecutionStatus::Failed, None, &detail);
        } else {
            event_recorder.warning(&detail);
        }
    }

    /// Build universal ToolDefinitions for the current agent.
    async fn build_universal_tools(&mut self) -> Result<Vec<ToolDefinition>> {
        let gemini_tools = self.build_agent_tools().await?;

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
                defer_loading: None,
            })
            .collect();

        Ok(tools)
    }

    /// Validate LLM request before sending to provider.
    /// Catches configuration errors early to avoid wasted API calls.
    fn validate_llm_request(&self, request: &LLMRequest) -> Result<()> {
        // Validate system prompt presence
        if request
            .system_prompt
            .as_ref()
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(anyhow!("System prompt cannot be empty"));
        }

        // Validate message history
        if request.messages.is_empty() {
            return Err(anyhow!("Message history cannot be empty"));
        }

        // Validate tools if present
        if let Some(tools) = &request.tools {
            let mut seen_names = std::collections::HashSet::new();
            for tool in tools {
                self.validate_tool_definition(tool, &mut seen_names)?;
            }
        }

        // Validate model is specified
        if request.model.trim().is_empty() {
            return Err(anyhow!("Model identifier cannot be empty"));
        }

        Ok(())
    }

    /// Validate a single tool definition for schema correctness.
    fn validate_tool_definition(
        &self,
        tool: &ToolDefinition,
        seen_names: &mut std::collections::HashSet<String>,
    ) -> Result<()> {
        if let Some(func) = &tool.function {
            // Check name is not empty
            if func.name.trim().is_empty() {
                return Err(anyhow!("Tool function name cannot be empty"));
            }
            // Check for duplicate names
            if !seen_names.insert(func.name.clone()) {
                return Err(anyhow!("Duplicate tool name: {}", func.name));
            }
            // Validate parameters schema if it's an object
            if let Some(obj) = func.parameters.as_object()
                && let Some(required) = obj.get("required")
                && let Some(required_arr) = required.as_array()
                && let Some(props) = obj.get("properties").and_then(|p| p.as_object())
            {
                for req in required_arr {
                    if let Some(req_name) = req.as_str()
                        && !props.contains_key(req_name)
                    {
                        return Err(anyhow!(
                            "Tool '{}' has required field '{}' not in properties",
                            func.name,
                            req_name
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Get the selected model for the current turn.
    fn get_selected_model(&self) -> String {
        self.model.clone()
    }

    /// Record a tool call for loop detection and check if a hard limit has been exceeded.
    /// Returns true if execution should halt due to a loop.
    fn check_for_loop(&self, name: &str, args: &Value, task_state: &mut TaskRunState) -> bool {
        if let Some(warning) = self.loop_detector.borrow_mut().record_call(name, args) {
            if self.loop_detector.borrow().is_hard_limit_exceeded(name) {
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
                return true;
            }
            runner_println!(self, "{}", style(&warning).yellow().bold());
            task_state.warnings.push(warning);
        }
        false
    }

    fn normalize_tool_args(
        &self,
        name: &str,
        args: &Value,
        task_state: &mut TaskRunState,
    ) -> Value {
        let Some(obj) = args.as_object() else {
            return args.clone();
        };

        let mut normalized = obj.clone();
        let workspace_path = self._workspace.to_string_lossy().to_string();
        let fallback_dir = task_state
            .last_dir_path
            .clone()
            .unwrap_or_else(|| workspace_path.clone());

        if matches!(name, tools::GREP_FILE | tools::LIST_FILES) && !normalized.contains_key("path")
        {
            normalized.insert("path".to_string(), Value::String(fallback_dir));
        }

        if name == tools::READ_FILE
            && !normalized.contains_key("file_path")
            && let Some(last_file) = task_state.last_file_path.clone()
        {
            normalized.insert("file_path".to_string(), Value::String(last_file));
        }

        if matches!(
            name,
            tools::WRITE_FILE | tools::EDIT_FILE | tools::CREATE_FILE
        ) && !normalized.contains_key("path")
            && let Some(last_file) = task_state.last_file_path.clone()
        {
            normalized.insert("path".to_string(), Value::String(last_file));
        }

        Value::Object(normalized)
    }

    fn update_last_paths_from_args(&self, name: &str, args: &Value, task_state: &mut TaskRunState) {
        if let Some(path) = args.get("file_path").and_then(|value| value.as_str()) {
            task_state.last_file_path = Some(path.to_string());
            if let Some(parent) = Path::new(path).parent() {
                task_state.last_dir_path = Some(parent.to_string_lossy().to_string());
            }
            return;
        }

        if let Some(path) = args.get("path").and_then(|value| value.as_str()) {
            if matches!(
                name,
                tools::READ_FILE | tools::WRITE_FILE | tools::EDIT_FILE | tools::CREATE_FILE
            ) {
                task_state.last_file_path = Some(path.to_string());
                if let Some(parent) = Path::new(path).parent() {
                    task_state.last_dir_path = Some(parent.to_string_lossy().to_string());
                }
            } else {
                task_state.last_dir_path = Some(path.to_string());
            }
        }
    }

    /// Execute multiple tool calls in parallel. Only safe for read-only operations.
    async fn execute_parallel_tool_calls(
        &self,
        tool_calls: Vec<ToolCall>,
        task_state: &mut TaskRunState,
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
            let name = func.name.clone();
            let args = call
                .parsed_arguments()
                .unwrap_or_else(|_| serde_json::json!({}));
            let args = self.normalize_tool_args(&name, &args, task_state);
            if self.check_for_loop(&name, &args, task_state) {
                return Ok(());
            }
            prepared_calls.push((call, name, args));
        }

        let total_calls = prepared_calls.len();
        runner_println!(
            self,
            "{} [{}] Executing {} tools in parallel",
            style("[PARALLEL]").cyan().bold(),
            self.agent_type,
            total_calls
        );

        let mut futures = Vec::with_capacity(prepared_calls.len());
        for (call, name, args) in prepared_calls {
            let call_id = call.id.clone();

            if !self.is_valid_tool(&name).await {
                self.record_tool_denied(
                    agent_prefix,
                    task_state,
                    event_recorder,
                    &call_id,
                    &name,
                    None,
                    is_gemini,
                );
                continue;
            }

            let tool_registry = self.tool_registry.clone();
            let args_clone = args.clone();
            futures.push(async move {
                let registry = tool_registry;
                let result = registry
                    .execute_tool_ref(&name, &args_clone)
                    .await
                    .map_err(|e| anyhow::anyhow!("Tool '{}' failed: {}", name, e));
                (name, call_id, args_clone, result)
            });
        }

        let results = join_all(futures).await;
        let mut halt_turn = false;
        for (name, call_id, args, result) in results {
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

                    let optimized_result = self.optimize_tool_result(&name, result).await;
                    let tool_result = serde_json::to_string(&optimized_result)?;
                    let display_text = format_tool_result_for_display(&name, &optimized_result);

                    self.update_last_paths_from_args(&name, &args, task_state);

                    task_state.push_tool_result(
                        call_id,
                        &name,
                        display_text,
                        tool_result,
                        is_gemini,
                    );
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
                    let err_lower = error_msg.to_lowercase();
                    if err_lower.contains("rate limit") {
                        task_state.warnings.push(
                            "Tool was rate limited; halting further tool calls this turn.".into(),
                        );
                        task_state.mark_tool_loop_limit_hit();
                        halt_turn = true;
                    } else if err_lower.contains("denied by policy")
                        || err_lower.contains("not permitted while full-auto")
                    {
                        task_state.warnings.push(
                            "Tool denied by policy; halting further tool calls this turn.".into(),
                        );
                        halt_turn = true;
                    }
                    task_state.push_tool_error(call_id, &name, error_msg, is_gemini);
                    event_recorder.command_finished(
                        &command_event,
                        CommandExecutionStatus::Failed,
                        None,
                        &e.to_string(),
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
    async fn execute_sequential_tool_calls(
        &self,
        tool_calls: Vec<ToolCall>,
        task_state: &mut TaskRunState,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
    ) -> Result<()> {
        for call in tool_calls {
            let name = match call.function.as_ref() {
                Some(func) => func.name.clone(),
                None => continue,
            };
            let args = call
                .parsed_arguments()
                .unwrap_or_else(|_| serde_json::json!({}));
            let args = self.normalize_tool_args(&name, &args, task_state);

            if self.check_for_loop(&name, &args, task_state) {
                break;
            }

            runner_println!(
                self,
                "{} [{}] Calling tool: {}",
                style("[TOOL_CALL]").blue().bold(),
                self.agent_type,
                name
            );

            let command_event = event_recorder.command_started(&name);
            if !self.is_valid_tool(&name).await {
                self.record_tool_denied(
                    agent_prefix,
                    task_state,
                    event_recorder,
                    &call.id,
                    &name,
                    Some(&command_event),
                    is_gemini,
                );
                continue;
            }

            let repeat_count = self.loop_detector.borrow().get_call_count(&name);
            if repeat_count > 1 {
                let delay_ms =
                    (LOOP_THROTTLE_BASE_MS * repeat_count as u64).min(LOOP_THROTTLE_MAX_MS);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }

            // Use internal execution since is_valid_tool was already called above
            match self.execute_tool_internal(&name, &args).await {
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

                    let optimized_result = self.optimize_tool_result(&name, result).await;
                    let tool_result = serde_json::to_string(&optimized_result)?;
                    let display_text = format_tool_result_for_display(&name, &optimized_result);

                    self.update_last_paths_from_args(&name, &args, task_state);

                    task_state.push_tool_result(
                        call.id.clone(),
                        &name,
                        display_text,
                        tool_result,
                        is_gemini,
                    );
                    event_recorder.command_finished(
                        &command_event,
                        CommandExecutionStatus::Completed,
                        None,
                        "",
                    );

                    if name == tools::WRITE_FILE
                        && let Some(filepath) = args.get("path").and_then(|p| p.as_str())
                    {
                        task_state.modified_files.push(filepath.to_owned());
                        event_recorder.file_change_completed(filepath);
                    }
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    let err_lower = err_msg.to_lowercase();
                    if err_lower.contains("rate limit") {
                        task_state.warnings.push(
                            "Tool was rate limited; halting further tool calls this turn.".into(),
                        );
                        task_state.mark_tool_loop_limit_hit();
                        let mut failure_ctx = ToolFailureContext {
                            agent_prefix,
                            task_state,
                            event_recorder,
                            command_event: &command_event,
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
                        task_state.warnings.push(
                            "Tool denied by policy; halting further tool calls this turn.".into(),
                        );
                        let mut failure_ctx = ToolFailureContext {
                            agent_prefix,
                            task_state,
                            event_recorder,
                            command_event: &command_event,
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
                        let mut failure_ctx = ToolFailureContext {
                            agent_prefix,
                            task_state,
                            event_recorder,
                            command_event: &command_event,
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
        // Pre-flight validation: fail fast before API call
        self.validate_llm_request(request)
            .context("LLM request validation failed")?;

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
                                if event_recorder.reasoning_stream_update(&aggregated_reasoning) {
                                    reasoning_recorded = true;
                                }
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
                Self::print_compact_response(&self.agent_type, &aggregated_text, self.quiet);
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

            if reasoning_recorded {
                event_recorder.reasoning_stream_complete();
                if response.reasoning.is_none() && !aggregated_reasoning.is_empty() {
                    response.reasoning = Some(aggregated_reasoning);
                }
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
                reasoning_recorded,
            });
        }

        if agent_message_streamed {
            event_recorder.agent_message_stream_complete();
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

    /// Apply workspace configuration to the tool registry, including tool policies and MCP setup.
    pub async fn apply_workspace_configuration(&mut self, vt_cfg: &VTCodeConfig) -> Result<()> {
        self.config = Arc::new(vt_cfg.clone());
        *self.loop_detector.borrow_mut() =
            LoopDetector::with_max_repeated_calls(self.config.tools.max_repeated_tool_calls.max(1));

        self.system_prompt = compose_system_instruction_text(
            self._workspace.as_path(),
            Some(self.config()),
            None, // No prompt_context
        )
        .await;

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
                    self.tool_registry
                        .set_mcp_client(Arc::clone(&mcp_client))
                        .await;
                    if let Err(err) = self.tool_registry.refresh_mcp_tools().await {
                        warn!("Failed to refresh MCP tools: {}", err);
                    }

                    // Sync MCP tools to files for dynamic context discovery
                    if vt_cfg.context.dynamic.enabled && vt_cfg.context.dynamic.sync_mcp_tools
                        && let Err(err) = mcp_client.sync_tools_to_files(&self._workspace).await
                    {
                        warn!("Failed to sync MCP tools to files: {}", err);
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

        // Initialize dynamic context discovery directories
        if vt_cfg.context.dynamic.enabled
            && let Err(err) = crate::context::initialize_dynamic_context(
                &self._workspace,
                &vt_cfg.context.dynamic,
            )
            .await
        {
                warn!("Failed to initialize dynamic context directories: {}", err);
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

    /// Check if a tool is allowed for this agent
    async fn is_tool_allowed(&self, tool_name: &str) -> bool {
        let policy = self.tool_registry.get_tool_policy(tool_name).await;
        matches!(
            policy,
            crate::tool_policy::ToolPolicy::Allow | crate::tool_policy::ToolPolicy::Prompt
        )
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
        self.is_tool_allowed(canonical).await;

        true
    }

    /// Execute a tool by name with given arguments.
    /// This is the public API that includes validation; for internal use after
    /// validation, prefer `execute_tool_internal`.
    #[allow(dead_code)]
    async fn execute_tool(&self, tool_name: &str, args: &Value) -> Result<Value> {
        // Fail fast if tool is denied or missing to avoid tight retry loops
        if !self.is_valid_tool(tool_name).await {
            return Err(anyhow!("{}: {}", ERR_TOOL_DENIED, tool_name));
        }
        self.execute_tool_internal(tool_name, args).await
    }

    /// Internal tool execution, skipping validation.
    /// Use when `is_valid_tool` has already been called by the caller.
    async fn execute_tool_internal(&self, tool_name: &str, args: &Value) -> Result<Value> {
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

            self.tool_registry.check_shell_policy(
                &cmd_text,
                &deny_regex_patterns,
                &deny_glob_patterns,
            )?;

            info!(target = "policy", agent = ?self.agent_type, tool = tool_name, cmd = %cmd_text, "shell_policy_checked");
        }

        // Use pre-computed retry delays to avoid repeated Duration construction
        const RETRY_DELAYS_MS: [u64; 3] = [200, 400, 800];

        // Clone the registry once and reuse across retries (avoids cloning on each attempt)
        let registry = self.tool_registry.clone();

        // Execute tool with adaptive retry
        let mut last_error: Option<anyhow::Error> = None;
        for (attempt, delay_ms) in RETRY_DELAYS_MS.iter().enumerate() {
            match registry.execute_tool_ref(tool_name, args).await {
                Ok(result) => return Ok(result),
                Err(e) if attempt < 2 => {
                    last_error = Some(e);
                    tokio::time::sleep(Duration::from_millis(*delay_ms)).await;
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
