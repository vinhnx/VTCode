use anyhow::Result;
use serde_json::json;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task;
#[cfg(debug_assertions)]
use tracing::debug;
use vtcode_config::constants::context::TOKEN_BUDGET_HIGH_THRESHOLD;
use vtcode_core::config::{OpenAIPromptCacheKeyMode, PromptCachingConfig};
use vtcode_core::exec::events::{
    AgentMessageItem, ItemCompletedEvent, ItemStartedEvent, ItemUpdatedEvent, ReasoningItem,
    ThreadEvent, ThreadItem, ThreadItemDetails,
};
use vtcode_core::llm::provider::{self as uni, ParallelToolConfig};
use vtcode_core::prompts::upsert_harness_limits_section;
use vtcode_core::turn_metadata;

use crate::agent::runloop::unified::extract_action_from_messages;
use crate::agent::runloop::unified::incremental_system_prompt::PromptCacheShapingMode;
use crate::agent::runloop::unified::reasoning::resolve_reasoning_visibility;
use crate::agent::runloop::unified::run_loop_context::TurnExecutionSnapshot;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use crate::agent::runloop::unified::turn::turn_helpers::supports_responses_chaining;
use crate::agent::runloop::unified::ui_interaction::{
    StreamProgressEvent, StreamSpinnerOptions, stream_and_render_response_with_options_and_progress,
};

/// Delegate LLM retryability checks to the canonical [`vtcode_commons::ErrorCategory`] classifier.
#[cfg(test)]
fn is_retryable_llm_error(message: &str) -> bool {
    vtcode_commons::is_retryable_llm_error_message(message)
}

/// Classify an LLM error message into an [`vtcode_commons::ErrorCategory`] for
/// structured logging and user-facing hints.
fn classify_llm_error(message: &str) -> vtcode_commons::ErrorCategory {
    vtcode_commons::classify_error_message(message)
}

const STREAM_TIMEOUT_FALLBACK_PROVIDERS: &[&str] = &[
    "huggingface",
    "ollama",
    "minimax",
    "deepseek",
    "moonshot",
    "zai",
    "openrouter",
    "lmstudio",
];

const RECENT_TOOL_RESPONSE_WINDOW: usize = 10;
const TOOL_RETRY_MAX_CHARS: usize = 1200;

fn is_openai_prompt_cache_enabled(
    provider_name: &str,
    global_prompt_cache_enabled: bool,
    openai_prompt_cache_enabled: bool,
) -> bool {
    provider_name.eq_ignore_ascii_case("openai")
        && global_prompt_cache_enabled
        && openai_prompt_cache_enabled
}

fn resolve_prompt_cache_shaping_mode(
    provider_name: &str,
    prompt_cache: &PromptCachingConfig,
) -> PromptCacheShapingMode {
    if !prompt_cache.cache_friendly_prompt_shaping
        || !prompt_cache.is_provider_enabled(provider_name)
    {
        return PromptCacheShapingMode::Disabled;
    }

    if matches!(
        provider_name.to_ascii_lowercase().as_str(),
        "anthropic" | "minimax"
    ) {
        PromptCacheShapingMode::AnthropicBlockRuntimeContext
    } else {
        PromptCacheShapingMode::TrailingRuntimeContext
    }
}

fn build_openai_prompt_cache_key(
    openai_prompt_cache_enabled: bool,
    prompt_cache_key_mode: &OpenAIPromptCacheKeyMode,
    run_id: &str,
) -> Option<String> {
    if !openai_prompt_cache_enabled {
        return None;
    }

    match prompt_cache_key_mode {
        OpenAIPromptCacheKeyMode::Session => Some(format!("vtcode:openai:{run_id}")),
        OpenAIPromptCacheKeyMode::Off => None,
    }
}

#[derive(Default)]
struct StreamItemBuffer {
    started: bool,
    text: String,
}

struct HarnessStreamingBridge<'a> {
    emitter:
        Option<&'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
    assistant_item_id: String,
    reasoning_item_id: String,
    assistant: StreamItemBuffer,
    reasoning: StreamItemBuffer,
    reasoning_stage: Option<String>,
}

impl<'a> HarnessStreamingBridge<'a> {
    fn new(
        emitter: Option<
            &'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter,
        >,
        turn_id: &str,
        step: usize,
        attempt: usize,
    ) -> Self {
        Self {
            emitter,
            assistant_item_id: format!("{turn_id}-step-{step}-assistant-stream-{attempt}"),
            reasoning_item_id: format!("{turn_id}-step-{step}-reasoning-stream-{attempt}"),
            assistant: StreamItemBuffer::default(),
            reasoning: StreamItemBuffer::default(),
            reasoning_stage: None,
        }
    }

    fn on_progress(&mut self, event: StreamProgressEvent) {
        match event {
            StreamProgressEvent::OutputDelta(delta) => self.push_assistant_delta(&delta),
            StreamProgressEvent::ReasoningDelta(delta) => self.push_reasoning_delta(&delta),
            StreamProgressEvent::ReasoningStage(stage) => self.update_reasoning_stage(stage),
        }
    }

    fn abort(&mut self) {
        self.complete_open_items();
    }

    fn push_assistant_delta(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        self.assistant.text.push_str(delta);
        if !self.assistant.started {
            self.assistant.started = true;
            self.emit_item_started(ThreadItem {
                id: self.assistant_item_id.clone(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: self.assistant.text.clone(),
                }),
            });
            return;
        }

        self.emit_item_updated(ThreadItem {
            id: self.assistant_item_id.clone(),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                text: self.assistant.text.clone(),
            }),
        });
    }

    fn push_reasoning_delta(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        self.reasoning.text.push_str(delta);
        if !self.reasoning.started {
            self.reasoning.started = true;
            self.emit_item_started(ThreadItem {
                id: self.reasoning_item_id.clone(),
                details: ThreadItemDetails::Reasoning(ReasoningItem {
                    text: self.reasoning.text.clone(),
                    stage: self.reasoning_stage.clone(),
                }),
            });
            return;
        }

        self.emit_item_updated(ThreadItem {
            id: self.reasoning_item_id.clone(),
            details: ThreadItemDetails::Reasoning(ReasoningItem {
                text: self.reasoning.text.clone(),
                stage: self.reasoning_stage.clone(),
            }),
        });
    }

    fn update_reasoning_stage(&mut self, stage: String) {
        self.reasoning_stage = Some(stage);
        if !self.reasoning.started {
            return;
        }
        self.emit_item_updated(ThreadItem {
            id: self.reasoning_item_id.clone(),
            details: ThreadItemDetails::Reasoning(ReasoningItem {
                text: self.reasoning.text.clone(),
                stage: self.reasoning_stage.clone(),
            }),
        });
    }

    fn complete_open_items(&mut self) {
        if self.assistant.started {
            self.assistant.started = false;
            self.emit_item_completed(ThreadItem {
                id: self.assistant_item_id.clone(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem {
                    text: self.assistant.text.clone(),
                }),
            });
        }
        if self.reasoning.started {
            self.reasoning.started = false;
            self.emit_item_completed(ThreadItem {
                id: self.reasoning_item_id.clone(),
                details: ThreadItemDetails::Reasoning(ReasoningItem {
                    text: self.reasoning.text.clone(),
                    stage: self.reasoning_stage.clone(),
                }),
            });
        }
    }

    fn emit_item_started(&self, item: ThreadItem) {
        if let Some(emitter) = self.emitter {
            let _ = emitter.emit(ThreadEvent::ItemStarted(ItemStartedEvent { item }));
        }
    }

    fn emit_item_updated(&self, item: ThreadItem) {
        if let Some(emitter) = self.emitter {
            let _ = emitter.emit(ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }));
        }
    }

    fn emit_item_completed(&self, item: ThreadItem) {
        if let Some(emitter) = self.emitter {
            let _ = emitter.emit(ThreadEvent::ItemCompleted(ItemCompletedEvent { item }));
        }
    }
}

fn supports_streaming_timeout_fallback(provider_name: &str) -> bool {
    STREAM_TIMEOUT_FALLBACK_PROVIDERS
        .iter()
        .any(|provider| provider_name.eq_ignore_ascii_case(provider))
}

fn is_stream_timeout_error(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("stream request timed out")
        || msg.contains("streaming request timed out")
        || msg.contains("llm request timed out after")
}

fn has_recent_tool_responses(messages: &[uni::Message]) -> bool {
    messages
        .iter()
        .rev()
        .take(RECENT_TOOL_RESPONSE_WINDOW)
        .any(|message| message.role == uni::MessageRole::Tool)
}

fn compact_tool_messages_for_retry(messages: &[uni::Message]) -> Vec<uni::Message> {
    let mut compacted = Vec::with_capacity(messages.len());
    for message in messages {
        if message.role != uni::MessageRole::Tool {
            compacted.push(message.clone());
            continue;
        }

        let text = message.content.as_text();
        if text.chars().count() <= TOOL_RETRY_MAX_CHARS {
            compacted.push(message.clone());
            continue;
        }

        let mut truncated = text.chars().take(TOOL_RETRY_MAX_CHARS).collect::<String>();
        if truncated.len() < text.len() {
            truncated.push_str("\n... [tool output truncated for retry]");
        }

        let mut cloned = message.clone();
        cloned.content = uni::MessageContent::text(truncated);
        compacted.push(cloned);
    }

    if compacted.is_empty() {
        messages.to_vec()
    } else {
        compacted
    }
}

fn resolve_compaction_threshold(
    configured_threshold: Option<u64>,
    context_size: usize,
) -> Option<u64> {
    let configured_threshold = configured_threshold.filter(|threshold| *threshold > 0);
    let derived_threshold = if context_size > 0 {
        Some(((context_size as f64) * TOKEN_BUDGET_HIGH_THRESHOLD).round() as u64)
    } else {
        None
    };

    configured_threshold.or(derived_threshold).map(|threshold| {
        let mut threshold = threshold.max(1);
        if context_size > 0 {
            threshold = threshold.min(context_size as u64);
        }
        threshold
    })
}

fn llm_attempt_timeout_secs(turn_timeout_secs: u64, plan_mode: bool, provider_name: &str) -> u64 {
    let baseline = (turn_timeout_secs / 5).clamp(30, 120);
    if !plan_mode {
        return baseline;
    }

    // Plan Mode requests usually include heavier context and can need
    // extra first-token latency budget before retries are useful.
    let plan_mode_floor = if supports_streaming_timeout_fallback(provider_name) {
        90
    } else {
        60
    };
    let plan_mode_budget = (turn_timeout_secs / 2).clamp(plan_mode_floor, 120);
    baseline.max(plan_mode_budget)
}

const DEFAULT_LLM_RETRY_ATTEMPTS: usize = 3;
const MAX_LLM_RETRY_ATTEMPTS: usize = 6;

fn llm_retry_attempts(configured_task_retries: Option<u32>) -> usize {
    configured_task_retries
        .and_then(|value| usize::try_from(value).ok())
        .map(|value| value.saturating_add(1))
        .unwrap_or(DEFAULT_LLM_RETRY_ATTEMPTS)
        .clamp(1, MAX_LLM_RETRY_ATTEMPTS)
}

fn compact_error_message(message: &str, max_chars: usize) -> String {
    if message.chars().count() <= max_chars {
        return message.to_string();
    }
    let mut preview = message.chars().take(max_chars).collect::<String>();
    preview.push_str("... [truncated]");
    preview
}

fn emit_tool_catalog_cache_metrics(
    ctx: &TurnProcessingContext<'_>,
    step_count: usize,
    model: &str,
    cache_hit: bool,
    plan_mode: bool,
    request_user_input_enabled: bool,
    available_tools: usize,
) {
    tracing::info!(
        target: "vtcode.turn.metrics",
        metric = "tool_catalog_cache",
        run_id = %ctx.harness_state.run_id.0,
        turn_id = %ctx.harness_state.turn_id.0,
        turn = step_count,
        model,
        cache_hit,
        plan_mode,
        request_user_input_enabled,
        available_tools,
        "turn metric"
    );

    #[derive(serde::Serialize)]
    struct ToolCatalogCacheRecord<'a> {
        kind: &'static str,
        turn: usize,
        model: &'a str,
        cache_hit: bool,
        plan_mode: bool,
        request_user_input_enabled: bool,
        available_tools: usize,
        ts: i64,
    }

    ctx.traj.log(&ToolCatalogCacheRecord {
        kind: "tool_catalog_cache_metrics",
        turn: step_count,
        model,
        cache_hit,
        plan_mode,
        request_user_input_enabled,
        available_tools,
        ts: chrono::Utc::now().timestamp(),
    });
}

#[allow(clippy::too_many_arguments)]
fn emit_llm_retry_metrics(
    ctx: &TurnProcessingContext<'_>,
    step_count: usize,
    model: &str,
    plan_mode: bool,
    attempts_made: usize,
    max_retries: usize,
    success: bool,
    stream_fallback_used: bool,
    last_error_retryable: Option<bool>,
    last_error_preview: Option<&str>,
) {
    let retries_used = attempts_made.saturating_sub(1);
    let exhausted_retry_budget = !success && attempts_made >= max_retries;
    tracing::info!(
        target: "vtcode.turn.metrics",
        metric = "llm_retry_outcome",
        run_id = %ctx.harness_state.run_id.0,
        turn_id = %ctx.harness_state.turn_id.0,
        turn = step_count,
        model,
        plan_mode,
        attempts_made,
        retries_used,
        max_retries,
        success,
        exhausted_retry_budget,
        stream_fallback_used,
        last_error_retryable = last_error_retryable.unwrap_or(false),
        "turn metric"
    );

    #[derive(serde::Serialize)]
    struct LlmRetryMetricsRecord<'a> {
        kind: &'static str,
        turn: usize,
        model: &'a str,
        plan_mode: bool,
        attempts_made: usize,
        retries_used: usize,
        max_retries: usize,
        success: bool,
        exhausted_retry_budget: bool,
        stream_fallback_used: bool,
        last_error_retryable: Option<bool>,
        last_error: Option<&'a str>,
        ts: i64,
    }

    ctx.traj.log(&LlmRetryMetricsRecord {
        kind: "llm_retry_metrics",
        turn: step_count,
        model,
        plan_mode,
        attempts_made,
        retries_used,
        max_retries,
        success,
        exhausted_retry_budget,
        stream_fallback_used,
        last_error_retryable,
        last_error: last_error_preview,
        ts: chrono::Utc::now().timestamp(),
    });
}

#[derive(Clone)]
struct TurnRequestSnapshot {
    provider_name: String,
    plan_mode: bool,
    full_auto: bool,
    request_user_input_enabled: bool,
    context_window_size: usize,
    turn_timeout_secs: u64,
    openai_prompt_cache_enabled: bool,
    openai_prompt_cache_key_mode: OpenAIPromptCacheKeyMode,
    prompt_cache_shaping_mode: PromptCacheShapingMode,
    capabilities: uni::ProviderCapabilities,
    execution: TurnExecutionSnapshot,
}

#[derive(Clone)]
struct PromptAssemblyInput<'a> {
    step_count: usize,
    active_model: &'a str,
    turn: &'a TurnRequestSnapshot,
}

struct PromptAssemblyOutput {
    system_prompt: String,
    current_tools: Option<Arc<Vec<uni::ToolDefinition>>>,
    has_tools: bool,
}

struct TurnRequestBuildResult {
    request: uni::LLMRequest,
    has_tools: bool,
}

fn capture_turn_request_snapshot(
    ctx: &mut TurnProcessingContext<'_>,
    active_model: &str,
) -> TurnRequestSnapshot {
    let (
        provider_name,
        plan_mode,
        request_user_input_enabled,
        context_window_size,
        turn_timeout_secs,
        openai_prompt_cache_enabled,
        openai_prompt_cache_key_mode,
        prompt_cache_shaping_mode,
        full_auto,
    ) = {
        let parts = ctx.parts_mut();
        let prompt_cache_config = &parts.llm.config.prompt_cache;
        let plan_mode = parts.state.session_stats.is_plan_mode();
        let provider_name = parts.llm.provider_client.name().to_ascii_lowercase();
        let openai_prompt_cache_enabled = is_openai_prompt_cache_enabled(
            &provider_name,
            prompt_cache_config.enabled,
            prompt_cache_config.providers.openai.enabled,
        );
        let prompt_cache_shaping_mode =
            resolve_prompt_cache_shaping_mode(&provider_name, prompt_cache_config);

        (
            provider_name,
            plan_mode,
            if plan_mode {
                true
            } else {
                parts
                    .llm
                    .vt_cfg
                    .as_ref()
                    .map(|cfg| cfg.chat.ask_questions.enabled)
                    .unwrap_or(true)
            },
            parts
                .llm
                .provider_client
                .effective_context_size(active_model),
            parts
                .llm
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.optimization.agent_execution.max_execution_time_secs)
                .unwrap_or(300),
            openai_prompt_cache_enabled,
            prompt_cache_config
                .providers
                .openai
                .prompt_cache_key_mode
                .clone(),
            prompt_cache_shaping_mode,
            parts.state.full_auto,
        )
    };
    let capabilities = {
        let parts = ctx.parts_mut();
        uni::get_cached_capabilities(&**parts.llm.provider_client, active_model)
    };

    TurnRequestSnapshot {
        provider_name,
        plan_mode,
        full_auto,
        request_user_input_enabled,
        context_window_size,
        turn_timeout_secs,
        openai_prompt_cache_enabled,
        openai_prompt_cache_key_mode,
        prompt_cache_shaping_mode,
        capabilities,
        execution: ctx.harness_state.execution_snapshot(),
    }
}

async fn assemble_prompt(
    ctx: &mut TurnProcessingContext<'_>,
    input: PromptAssemblyInput<'_>,
) -> Result<PromptAssemblyOutput> {
    let mut system_prompt = {
        let parts = ctx.parts_mut();
        parts
            .llm
            .context_manager
            .build_system_prompt(
                parts.state.working_history,
                input.step_count,
                crate::agent::runloop::unified::context_manager::SystemPromptParams {
                    full_auto: input.turn.full_auto,
                    plan_mode: input.turn.plan_mode,
                    context_window_size: Some(input.turn.context_window_size),
                    prompt_cache_shaping_mode: input.turn.prompt_cache_shaping_mode,
                },
            )
            .await?
    };

    upsert_harness_limits_section(
        &mut system_prompt,
        input.turn.execution.max_tool_calls,
        input.turn.execution.max_tool_wall_clock_secs,
        input.turn.execution.max_tool_retries,
    );

    let tool_snapshot = {
        let parts = ctx.parts_mut();
        parts
            .tool
            .tool_catalog
            .filtered_snapshot_with_stats(
                parts.tool.tools,
                input.turn.plan_mode,
                input.turn.request_user_input_enabled,
            )
            .await
    };
    let current_tools = tool_snapshot.snapshot;
    let has_tools = current_tools.is_some();
    emit_tool_catalog_cache_metrics(
        ctx,
        input.step_count,
        input.active_model,
        tool_snapshot.cache_hit,
        input.turn.plan_mode,
        input.turn.request_user_input_enabled,
        current_tools.as_ref().map_or(0, |defs| defs.len()),
    );

    if let Some(defs) = current_tools.as_ref()
        && !input.turn.prompt_cache_shaping_mode.is_enabled()
    {
        let _ = writeln!(
            system_prompt,
            "\n[Runtime Tool Catalog]\n- version: {}\n- available_tools: {}",
            ctx.tool_catalog.current_version(),
            defs.len()
        );
    }

    Ok(PromptAssemblyOutput {
        system_prompt,
        current_tools,
        has_tools,
    })
}

fn resolve_context_management(
    ctx: &TurnProcessingContext<'_>,
    active_model: &str,
) -> Option<serde_json::Value> {
    let harness_config = ctx.vt_cfg.map(|cfg| &cfg.agent.harness);
    let auto_compaction_enabled = harness_config
        .map(|cfg| cfg.auto_compaction_enabled)
        .unwrap_or(false);
    let supports_server_compaction = ctx
        .provider_client
        .supports_responses_compaction(active_model);
    if auto_compaction_enabled && supports_server_compaction {
        let context_size = ctx.provider_client.effective_context_size(active_model);
        let configured_threshold =
            harness_config.and_then(|cfg| cfg.auto_compaction_threshold_tokens);

        resolve_compaction_threshold(configured_threshold, context_size).map(|compact_threshold| {
            json!([{
                "type": "compaction",
                "compact_threshold": compact_threshold,
            }])
        })
    } else {
        None
    }
}

fn interrupted_provider_error(provider_name: &str) -> anyhow::Error {
    anyhow::Error::new(uni::LLMError::Provider {
        message: vtcode_core::llm::error_display::format_llm_error(
            provider_name,
            "Interrupted by user",
        ),
        metadata: None,
    })
}

async fn build_turn_request(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    active_model: &str,
    turn_snapshot: &TurnRequestSnapshot,
    parallel_cfg_opt: Option<Box<ParallelToolConfig>>,
    use_streaming: bool,
) -> Result<TurnRequestBuildResult> {
    let prompt_output = assemble_prompt(
        ctx,
        PromptAssemblyInput {
            step_count,
            active_model,
            turn: turn_snapshot,
        },
    )
    .await?;

    let reasoning_effort = {
        let parts = ctx.parts_mut();
        parts.llm.vt_cfg.as_ref().and_then(|cfg| {
            if turn_snapshot.capabilities.reasoning_effort {
                Some(cfg.agent.reasoning_effort)
            } else {
                None
            }
        })
    };
    let temperature = if reasoning_effort.is_some()
        && matches!(
            turn_snapshot.provider_name.as_str(),
            "anthropic" | "minimax"
        ) {
        None
    } else {
        Some(0.7)
    };
    let parallel_config =
        if prompt_output.has_tools && turn_snapshot.capabilities.parallel_tool_config {
            parallel_cfg_opt
        } else {
            None
        };
    let tool_choice = if prompt_output.has_tools {
        Some(uni::ToolChoice::auto())
    } else {
        None
    };

    let metadata = match turn_metadata::build_turn_metadata_value_with_timeout(
        &ctx.config.workspace,
        std::time::Duration::from_millis(250),
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(error = %err, "Turn metadata collection failed");
            None
        }
    };
    let prompt_cache_key = build_openai_prompt_cache_key(
        turn_snapshot.openai_prompt_cache_enabled,
        &turn_snapshot.openai_prompt_cache_key_mode,
        &ctx.harness_state.run_id.0,
    );
    let previous_response_id = if supports_responses_chaining(&turn_snapshot.provider_name) {
        ctx.session_stats
            .previous_response_id_for(&turn_snapshot.provider_name, active_model)
    } else {
        None
    };
    let context_management = resolve_context_management(ctx, active_model);
    let normalized_messages = {
        let parts = ctx.parts_mut();
        parts
            .llm
            .context_manager
            .normalize_history_for_request(parts.state.working_history)
    };

    let request = uni::LLMRequest {
        messages: normalized_messages,
        system_prompt: Some(Arc::new(prompt_output.system_prompt)),
        tools: prompt_output.current_tools,
        model: active_model.to_string(),
        temperature,
        stream: use_streaming,
        tool_choice,
        parallel_tool_config: parallel_config,
        reasoning_effort,
        metadata,
        context_management,
        previous_response_id,
        prompt_cache_key,
        ..Default::default()
    };

    Ok(TurnRequestBuildResult {
        request,
        has_tools: prompt_output.has_tools,
    })
}

/// Execute an LLM request and return the response.
pub(crate) async fn execute_llm_request(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    active_model: &str,
    _max_tokens_opt: Option<u32>,
    parallel_cfg_opt: Option<Box<ParallelToolConfig>>,
) -> Result<(uni::LLMResponse, bool)> {
    let turn_snapshot = capture_turn_request_snapshot(ctx, active_model);
    let request_timeout_secs = llm_attempt_timeout_secs(
        turn_snapshot.turn_timeout_secs,
        turn_snapshot.plan_mode,
        &turn_snapshot.provider_name,
    );

    ctx.renderer
        .set_reasoning_visible(resolve_reasoning_visibility(
            ctx.vt_cfg,
            turn_snapshot.capabilities.reasoning,
        ));
    let mut use_streaming = turn_snapshot.capabilities.streaming;
    let initial_request = build_turn_request(
        ctx,
        step_count,
        active_model,
        &turn_snapshot,
        parallel_cfg_opt.clone(),
        use_streaming,
    )
    .await?;
    let mut request = initial_request.request;
    let has_tools = initial_request.has_tools;
    if let Err(err) = ctx.provider_client.as_ref().validate_request(&request) {
        return Err(anyhow::Error::new(err));
    }

    let action_suggestion = extract_action_from_messages(ctx.working_history);

    let max_retries = llm_retry_attempts(ctx.vt_cfg.map(|cfg| cfg.agent.max_task_retries));
    let mut llm_result = Err(anyhow::anyhow!("LLM request failed to execute"));
    let mut attempts_made = 0usize;
    let mut stream_fallback_used = false;
    let mut compacted_tool_retry_used = false;
    let mut dropped_previous_response_id_for_retry = false;
    let mut last_error_retryable: Option<bool> = None;
    let mut last_error_preview: Option<String> = None;
    let mut last_error_category: Option<vtcode_commons::ErrorCategory> = None;

    #[cfg(debug_assertions)]
    let mut request_timer = Instant::now();

    for attempt in 0..max_retries {
        attempts_made = attempt + 1;
        if attempt > 0 {
            use crate::agent::runloop::unified::turn::turn_helpers::calculate_backoff;
            // Use category-aware backoff: rate limits get longer base delays,
            // timeouts get moderate delays, network errors use standard exponential.
            let (base_ms, max_ms) = match last_error_category {
                Some(vtcode_commons::ErrorCategory::RateLimit) => (1000, 30_000),
                Some(vtcode_commons::ErrorCategory::Timeout) => (1000, 15_000),
                _ => (500, 10_000),
            };
            let delay = calculate_backoff(attempt - 1, base_ms, max_ms);
            let delay_secs = delay.as_secs_f64();
            let reason_hint = last_error_category
                .as_ref()
                .map(|cat| cat.user_label())
                .unwrap_or("unknown error");
            crate::agent::runloop::unified::turn::turn_helpers::display_status(
                ctx.renderer,
                &format!(
                    "LLM request failed ({}), retrying in {:.1}s... (attempt {}/{})",
                    reason_hint,
                    delay_secs,
                    attempt + 1,
                    max_retries
                ),
            )?;
            let cancel_notifier = ctx.ctrl_c_notify.notified();
            tokio::pin!(cancel_notifier);
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = &mut cancel_notifier => {
                    if ctx.ctrl_c_state.is_cancel_requested() || ctx.ctrl_c_state.is_exit_requested() {
                        llm_result = Err(interrupted_provider_error(&turn_snapshot.provider_name));
                        break;
                    }
                }
            }
        }

        let spinner_msg = if attempt > 0 {
            let action = action_suggestion.clone();
            if action.is_empty() {
                format!("Retrying request (attempt {}/{})", attempt + 1, max_retries)
            } else {
                format!("{} (Retry {}/{})", action, attempt + 1, max_retries)
            }
        } else {
            action_suggestion.clone()
        };

        use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;
        let _spinner = PlaceholderSpinner::new(
            ctx.handle,
            ctx.input_status_state.left.clone(),
            ctx.input_status_state.right.clone(),
            spinner_msg,
        );
        if has_tools {
            _spinner.set_defer_restore(true);
        }
        task::yield_now().await;
        let attempt_started_at = Instant::now();

        #[cfg(debug_assertions)]
        {
            request_timer = Instant::now();
            let tool_count = request.tools.as_ref().map_or(0, |tools| tools.len());
            debug!(
                target = "vtcode::agent::llm",
                model = %request.model,
                streaming = use_streaming,
                step = step_count,
                messages = request.messages.len(),
                tools = tool_count,
                attempt = attempt + 1,
                "Dispatching provider request"
            );
        }

        request.stream = use_streaming;
        let has_post_tool_context = has_recent_tool_responses(&request.messages);

        let step_result = if use_streaming {
            let mut stream_bridge = HarnessStreamingBridge::new(
                ctx.harness_emitter,
                &ctx.harness_state.turn_id.0,
                step_count,
                attempt + 1,
            );
            let stream_options = StreamSpinnerOptions {
                defer_finish: has_tools,
                strip_proposed_plan_blocks: turn_snapshot.plan_mode,
            };
            let mut progress = |event: StreamProgressEvent| stream_bridge.on_progress(event);
            let stream_future = stream_and_render_response_with_options_and_progress(
                &**ctx.provider_client,
                request.clone(),
                &_spinner,
                ctx.renderer,
                ctx.ctrl_c_state,
                ctx.ctrl_c_notify,
                stream_options,
                Some(&mut progress),
            );
            let res =
                tokio::time::timeout(Duration::from_secs(request_timeout_secs), stream_future)
                    .await;

            match res {
                Ok(Ok((response, emitted_tokens))) => {
                    stream_bridge.complete_open_items();
                    Ok((response, emitted_tokens))
                }
                Ok(Err(err)) => {
                    stream_bridge.abort();
                    Err(anyhow::Error::new(err))
                }
                Err(_) => {
                    stream_bridge.abort();
                    Err(anyhow::anyhow!(
                        "LLM request timed out after {} seconds",
                        request_timeout_secs
                    ))
                }
            }
        } else if ctx.ctrl_c_state.is_cancel_requested() || ctx.ctrl_c_state.is_exit_requested() {
            Err(interrupted_provider_error(&turn_snapshot.provider_name))
        } else {
            let generate_future = tokio::time::timeout(
                Duration::from_secs(request_timeout_secs),
                ctx.provider_client.generate(request.clone()),
            );
            tokio::pin!(generate_future);
            let cancel_notifier = ctx.ctrl_c_notify.notified();
            tokio::pin!(cancel_notifier);

            let outcome = tokio::select! {
                res = &mut generate_future => match res {
                    Ok(inner) => inner.map_err(anyhow::Error::from),
                    Err(_) => Err(anyhow::anyhow!(
                        "LLM request timed out after {} seconds",
                        request_timeout_secs
                    )),
                },
                _ = &mut cancel_notifier => {
                    Err(interrupted_provider_error(&turn_snapshot.provider_name))
                }
            };

            match outcome {
                Ok(response) => Ok((response, false)),
                Err(err) => Err(err),
            }
        };
        let attempt_elapsed = attempt_started_at.elapsed();
        match &step_result {
            Ok((response, _)) => {
                ctx.telemetry.record_llm_request(
                    active_model,
                    attempt_elapsed,
                    response.usage.as_ref(),
                );
            }
            Err(_) => {
                ctx.telemetry
                    .record_llm_request(active_model, attempt_elapsed, None);
            }
        }

        #[cfg(debug_assertions)]
        {
            debug!(
                target = "vtcode::agent::llm",
                model = %active_model,
                streaming = use_streaming,
                step = step_count,
                elapsed_ms = request_timer.elapsed().as_millis(),
                succeeded = step_result.is_ok(),
                attempt = attempt + 1,
                "Provider request finished"
            );
        }

        match step_result {
            Ok((response, response_streamed)) => {
                if supports_responses_chaining(&turn_snapshot.provider_name) {
                    ctx.session_stats.set_previous_response_chain(
                        &turn_snapshot.provider_name,
                        active_model,
                        response.request_id.as_deref(),
                    );
                } else {
                    ctx.session_stats.clear_previous_response_chain();
                }
                llm_result = Ok((response, response_streamed));
                _spinner.finish();
                break;
            }
            Err(err) => {
                let msg = err.to_string();
                let category = classify_llm_error(&msg);
                let is_retryable = category.is_retryable();
                last_error_retryable = Some(is_retryable);
                last_error_preview = Some(compact_error_message(&msg, 180));
                last_error_category = Some(category);

                tracing::warn!(
                    target: "vtcode.llm.retry",
                    error = %msg,
                    category = %category.user_label(),
                    retryable = is_retryable,
                    attempt = attempt + 1,
                    max_retries,
                    "LLM request attempt failed"
                );

                if !crate::agent::runloop::unified::turn::turn_helpers::should_continue_operation(
                    ctx.ctrl_c_state,
                ) {
                    llm_result = Err(err);
                    _spinner.finish();
                    break;
                }

                // Fail-fast for permanent errors: don't waste retry budget
                // on authentication failures, resource exhaustion, or policy violations.
                if category.is_permanent() {
                    tracing::info!(
                        target: "vtcode.llm.retry",
                        category = %category.user_label(),
                        "Permanent error detected; skipping remaining retries"
                    );
                    llm_result = Err(err);
                    _spinner.finish();
                    break;
                }

                if is_retryable && attempt < max_retries - 1 {
                    if request.previous_response_id.is_some()
                        && !dropped_previous_response_id_for_retry
                    {
                        request.previous_response_id = None;
                        dropped_previous_response_id_for_retry = true;
                        ctx.session_stats.clear_previous_response_chain();
                        crate::agent::runloop::unified::turn::turn_helpers::display_status(
                            ctx.renderer,
                            "Retrying without previous response chain after provider error.",
                        )?;
                    }
                    if use_streaming
                        && supports_streaming_timeout_fallback(&turn_snapshot.provider_name)
                        && is_stream_timeout_error(&msg)
                    {
                        use_streaming = false;
                        stream_fallback_used = true;
                        crate::agent::runloop::unified::turn::turn_helpers::display_status(
                            ctx.renderer,
                            "Streaming timed out; retrying with non-streaming for this provider.",
                        )?;
                    }
                    _spinner.finish();
                    continue;
                }

                // Universal post-tool recovery: when a provider fails after
                // receiving tool results, try non-streaming first, then compact
                // the tool messages. Works for all providers, not just MiniMax.
                if has_post_tool_context && attempt < max_retries - 1 {
                    if use_streaming {
                        use_streaming = false;
                        stream_fallback_used = true;
                        crate::agent::runloop::unified::turn::turn_helpers::display_status(
                            ctx.renderer,
                            &format!(
                                "{} post-tool follow-up failed; retrying with non-streaming.",
                                turn_snapshot.provider_name
                            ),
                        )?;
                        _spinner.finish();
                        continue;
                    }

                    if !compacted_tool_retry_used {
                        let compacted = compact_tool_messages_for_retry(&request.messages);
                        request.messages = compacted;
                        compacted_tool_retry_used = true;
                        crate::agent::runloop::unified::turn::turn_helpers::display_status(
                            ctx.renderer,
                            &format!(
                                "{} follow-up still failed; retrying with compacted tool context.",
                                turn_snapshot.provider_name
                            ),
                        )?;
                        _spinner.finish();
                        continue;
                    }
                }

                llm_result = Err(err);
                _spinner.finish();
                break;
            }
        }
    }

    #[cfg(debug_assertions)]
    {
        debug!(
            target = "vtcode::agent::llm",
            model = %active_model,
            streaming = use_streaming,
            step = step_count,
            elapsed_ms = request_timer.elapsed().as_millis(),
            succeeded = llm_result.is_ok(),
            "Provider request finished"
        );
    }

    if attempts_made == 0 {
        attempts_made = 1;
    }
    if last_error_preview.is_none()
        && let Err(err) = &llm_result
    {
        last_error_preview = Some(compact_error_message(&err.to_string(), 180));
    }
    emit_llm_retry_metrics(
        ctx,
        step_count,
        active_model,
        turn_snapshot.plan_mode,
        attempts_made,
        max_retries,
        llm_result.is_ok(),
        stream_fallback_used,
        last_error_retryable,
        last_error_preview.as_deref(),
    );

    let (response, response_streamed) = match llm_result {
        Ok(result) => result,
        Err(error) => {
            return Err(error);
        }
    };
    if let Some(usage) = response.usage.as_ref() {
        #[derive(serde::Serialize)]
        struct PromptCacheMetricsRecord<'a> {
            kind: &'static str,
            turn: usize,
            model: &'a str,
            prompt_tokens: u32,
            completion_tokens: u32,
            total_tokens: u32,
            cached_prompt_tokens: u32,
            cache_hit_ratio: f64,
            ts: i64,
        }

        let cached_prompt_tokens = usage.cached_prompt_tokens.unwrap_or(0);
        let cache_hit_ratio = if usage.prompt_tokens == 0 {
            0.0
        } else {
            cached_prompt_tokens as f64 / usage.prompt_tokens as f64
        };
        let record = PromptCacheMetricsRecord {
            kind: "prompt_cache_metrics",
            turn: step_count,
            model: active_model,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            cached_prompt_tokens,
            cache_hit_ratio,
            ts: chrono::Utc::now().timestamp(),
        };
        ctx.traj.log(&record);
    }
    Ok((response, response_streamed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn retryable_llm_error_includes_internal_server_error_message() {
        assert!(is_retryable_llm_error(
            "Provider error: Internal Server Error"
        ));
    }

    #[test]
    fn retryable_llm_error_excludes_non_transient_messages() {
        assert!(!is_retryable_llm_error("Provider error: Invalid API key"));
    }

    #[test]
    fn retryable_llm_error_excludes_forbidden_quota_failures() {
        assert!(!is_retryable_llm_error(
            "Provider error: HuggingFace API error (403 Forbidden): {\"error\":\"You have exceeded your monthly spending limit.\"}"
        ));
    }

    #[test]
    fn retryable_llm_error_includes_rate_limit_429() {
        assert!(is_retryable_llm_error(
            "Provider error: 429 Too Many Requests"
        ));
    }

    #[test]
    fn retryable_llm_error_includes_service_unavailable_class() {
        assert!(is_retryable_llm_error(
            "Provider error: 503 Service Unavailable"
        ));
        assert!(is_retryable_llm_error(
            "Provider error: 504 Gateway Timeout"
        ));
    }

    #[test]
    fn retryable_llm_error_excludes_usage_limit_messages() {
        assert!(!is_retryable_llm_error(
            "Provider error: you have reached your weekly usage limit"
        ));
    }

    #[test]
    fn supports_streaming_timeout_fallback_covers_supported_providers() {
        assert!(supports_streaming_timeout_fallback("huggingface"));
        assert!(supports_streaming_timeout_fallback("ollama"));
        assert!(supports_streaming_timeout_fallback("minimax"));
        assert!(supports_streaming_timeout_fallback("HUGGINGFACE"));
        assert!(!supports_streaming_timeout_fallback("openai"));
    }

    #[test]
    fn compact_tool_messages_for_retry_keeps_recent_tool_outputs_only() {
        let messages = vec![
            uni::Message::user("u1".to_string()),
            uni::Message::tool_response("call_1".to_string(), "old tool".to_string()),
            uni::Message::assistant("a1".to_string()),
            uni::Message::tool_response("call_2".to_string(), "new tool".to_string()),
        ];

        let compacted = compact_tool_messages_for_retry(&messages);
        assert_eq!(
            compacted
                .iter()
                .filter(|message| message.role == uni::MessageRole::Tool)
                .count(),
            2
        );
        assert_eq!(compacted.len(), 4);
    }

    #[test]
    fn compact_tool_messages_for_retry_keeps_all_tool_call_ids() {
        let messages = vec![
            uni::Message::tool_response("call_1".to_string(), "first".to_string()),
            uni::Message::assistant("a1".to_string()),
            uni::Message::tool_response("call_2".to_string(), "second".to_string()),
            uni::Message::assistant("a2".to_string()),
            uni::Message::tool_response("call_3".to_string(), "third".to_string()),
        ];

        let compacted = compact_tool_messages_for_retry(&messages);
        let tool_ids = compacted
            .iter()
            .filter(|message| message.role == uni::MessageRole::Tool)
            .filter_map(|message| message.tool_call_id.clone())
            .collect::<Vec<_>>();

        assert_eq!(tool_ids, vec!["call_1", "call_2", "call_3"]);
    }

    #[test]
    fn llm_retry_attempts_uses_default_when_unset() {
        assert_eq!(llm_retry_attempts(None), DEFAULT_LLM_RETRY_ATTEMPTS);
    }

    #[test]
    fn llm_retry_attempts_uses_configured_retries_plus_initial_attempt() {
        assert_eq!(llm_retry_attempts(Some(2)), 3);
    }

    #[test]
    fn llm_retry_attempts_respects_upper_bound() {
        assert_eq!(llm_retry_attempts(Some(16)), MAX_LLM_RETRY_ATTEMPTS);
    }

    #[test]
    fn resolve_compaction_threshold_prefers_configured_value() {
        assert_eq!(resolve_compaction_threshold(Some(42), 200_000), Some(42));
    }

    #[test]
    fn resolve_compaction_threshold_uses_context_ratio_when_unset() {
        assert_eq!(resolve_compaction_threshold(None, 200_000), Some(180_000));
    }

    #[test]
    fn resolve_compaction_threshold_clamps_to_context_size() {
        assert_eq!(
            resolve_compaction_threshold(Some(300_000), 200_000),
            Some(200_000)
        );
    }

    #[test]
    fn resolve_compaction_threshold_requires_context_or_override() {
        assert_eq!(resolve_compaction_threshold(None, 0), None);
    }

    #[test]
    fn stream_timeout_error_detection_matches_common_messages() {
        assert!(is_stream_timeout_error(
            "Stream request timed out after 75s"
        ));
        assert!(is_stream_timeout_error(
            "Streaming request timed out after configured timeout"
        ));
        assert!(is_stream_timeout_error(
            "LLM request timed out after 120 seconds"
        ));
    }

    #[test]
    fn llm_attempt_timeout_defaults_to_fifth_of_turn_budget() {
        assert_eq!(llm_attempt_timeout_secs(300, false, "openai"), 60);
    }

    #[test]
    fn llm_attempt_timeout_expands_for_plan_mode() {
        assert_eq!(llm_attempt_timeout_secs(300, true, "openai"), 120);
    }

    #[test]
    fn llm_attempt_timeout_plan_mode_respects_smaller_turn_budget() {
        assert_eq!(llm_attempt_timeout_secs(180, true, "openai"), 90);
    }

    #[test]
    fn llm_attempt_timeout_plan_mode_huggingface_uses_higher_floor() {
        assert_eq!(llm_attempt_timeout_secs(150, true, "huggingface"), 90);
    }

    #[test]
    fn llm_attempt_timeout_respects_plan_mode_cap() {
        assert_eq!(llm_attempt_timeout_secs(1_200, true, "huggingface"), 120);
    }

    #[test]
    fn openai_prompt_cache_enablement_requires_provider_and_flags() {
        assert!(is_openai_prompt_cache_enabled("openai", true, true));
        assert!(!is_openai_prompt_cache_enabled("openai", false, true));
        assert!(!is_openai_prompt_cache_enabled("openai", true, false));
        assert!(!is_openai_prompt_cache_enabled("anthropic", true, true));
    }

    #[test]
    fn prompt_cache_shaping_mode_requires_global_opt_in_and_provider_cache() {
        let mut cfg = PromptCachingConfig::default();
        cfg.enabled = true;
        cfg.cache_friendly_prompt_shaping = true;
        cfg.providers.openai.enabled = true;

        assert_eq!(
            resolve_prompt_cache_shaping_mode("openai", &cfg),
            PromptCacheShapingMode::TrailingRuntimeContext
        );

        cfg.cache_friendly_prompt_shaping = false;
        assert_eq!(
            resolve_prompt_cache_shaping_mode("openai", &cfg),
            PromptCacheShapingMode::Disabled
        );
    }

    #[test]
    fn prompt_cache_shaping_mode_uses_block_mode_for_anthropic_family() {
        let mut cfg = PromptCachingConfig::default();
        cfg.enabled = true;
        cfg.cache_friendly_prompt_shaping = true;
        cfg.providers.anthropic.enabled = true;

        assert_eq!(
            resolve_prompt_cache_shaping_mode("anthropic", &cfg),
            PromptCacheShapingMode::AnthropicBlockRuntimeContext
        );
        assert_eq!(
            resolve_prompt_cache_shaping_mode("minimax", &cfg),
            PromptCacheShapingMode::AnthropicBlockRuntimeContext
        );
    }

    #[test]
    fn prompt_cache_shaping_mode_respects_gemini_mode_off() {
        let mut cfg = PromptCachingConfig::default();
        cfg.enabled = true;
        cfg.cache_friendly_prompt_shaping = true;
        cfg.providers.gemini.enabled = true;
        cfg.providers.gemini.mode = vtcode_core::config::core::GeminiPromptCacheMode::Off;

        assert_eq!(
            resolve_prompt_cache_shaping_mode("gemini", &cfg),
            PromptCacheShapingMode::Disabled
        );
    }

    #[test]
    fn openai_prompt_cache_key_uses_stable_session_identifier() {
        let run_id = "run-abc-123";
        let first = build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, run_id);
        let second =
            build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, run_id);

        assert_eq!(first, Some("vtcode:openai:run-abc-123".to_string()));
        assert_eq!(first, second);
    }

    #[test]
    fn openai_prompt_cache_key_honors_off_mode_or_disabled_cache() {
        assert_eq!(
            build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Off, "run-1"),
            None
        );
        assert_eq!(
            build_openai_prompt_cache_key(false, &OpenAIPromptCacheKeyMode::Session, "run-1"),
            None
        );
    }

    #[test]
    fn upsert_harness_limits_adds_single_section() {
        let mut prompt = "Base prompt".to_string();

        upsert_harness_limits_section(&mut prompt, 12, 180, 2);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("- max_tool_calls_per_turn: 12"));
        assert!(prompt.contains("- max_tool_wall_clock_secs: 180"));
        assert!(prompt.contains("- max_tool_retries: 2"));
    }

    #[test]
    fn upsert_harness_limits_replaces_existing_values() {
        let mut prompt = "Base prompt\n[Harness Limits]\n- max_tool_calls_per_turn: 3\n- max_tool_wall_clock_secs: 60\n- max_tool_retries: 1\n".to_string();

        upsert_harness_limits_section(&mut prompt, 9, 240, 4);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("- max_tool_calls_per_turn: 9"));
        assert!(prompt.contains("- max_tool_wall_clock_secs: 240"));
        assert!(prompt.contains("- max_tool_retries: 4"));
        assert!(!prompt.contains("- max_tool_calls_per_turn: 3"));
    }

    #[test]
    fn upsert_harness_limits_preserves_trailing_prompt_sections() {
        let mut prompt = "Base prompt\n[Harness Limits]\n- max_tool_calls_per_turn: 3\n- max_tool_wall_clock_secs: 60\n- max_tool_retries: 1\n[Additional Context]\nKeep this section".to_string();

        upsert_harness_limits_section(&mut prompt, 11, 90, 3);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("[Additional Context]\nKeep this section"));
        assert!(prompt.ends_with("- max_tool_retries: 3\n"));
    }

    #[test]
    fn upsert_harness_limits_replaces_indented_section_header() {
        let mut prompt = "Base prompt\n  [Harness Limits]\n- max_tool_calls_per_turn: 1\n- max_tool_wall_clock_secs: 1\n- max_tool_retries: 1\n".to_string();

        upsert_harness_limits_section(&mut prompt, 5, 30, 2);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("- max_tool_calls_per_turn: 5"));
        assert!(!prompt.contains("- max_tool_calls_per_turn: 1"));
    }

    #[test]
    fn upsert_harness_limits_removes_duplicate_sections() {
        let mut prompt = "Base prompt\n[Harness Limits]\n- max_tool_calls_per_turn: 2\n- max_tool_wall_clock_secs: 10\n- max_tool_retries: 1\n[Other]\nkeep\n[Harness Limits]\n- max_tool_calls_per_turn: 3\n- max_tool_wall_clock_secs: 20\n- max_tool_retries: 2\n".to_string();

        upsert_harness_limits_section(&mut prompt, 7, 70, 3);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("- max_tool_calls_per_turn: 7"));
        assert!(prompt.contains("[Other]\nkeep"));
    }

    #[test]
    fn harness_streaming_bridge_emits_incremental_agent_and_reasoning_items() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("harness.jsonl");
        let emitter =
            crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
                .expect("harness emitter");

        let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_123", 1, 1);
        bridge.on_progress(StreamProgressEvent::ReasoningStage("analysis".to_string()));
        bridge.on_progress(StreamProgressEvent::ReasoningDelta("think".to_string()));
        bridge.on_progress(StreamProgressEvent::OutputDelta("hello".to_string()));
        bridge.on_progress(StreamProgressEvent::OutputDelta(" world".to_string()));
        bridge.complete_open_items();

        let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
        let mut saw_assistant_started = false;
        let mut saw_assistant_updated = false;
        let mut saw_assistant_completed = false;
        let mut saw_reasoning_started = false;
        let mut saw_reasoning_completed = false;

        for line in payload.lines() {
            let value: serde_json::Value = serde_json::from_str(line).expect("json");
            let event = value.get("event").expect("event");
            let event_type = event
                .get("type")
                .and_then(|kind| kind.as_str())
                .unwrap_or_default();
            let item_type = event
                .get("item")
                .and_then(|item| item.get("type"))
                .and_then(|kind| kind.as_str())
                .unwrap_or_default();
            let item_text = event
                .get("item")
                .and_then(|item| item.get("text"))
                .and_then(|text| text.as_str())
                .unwrap_or_default();

            if event_type == "item.started" && item_type == "agent_message" {
                saw_assistant_started = item_text == "hello";
            }
            if event_type == "item.updated" && item_type == "agent_message" {
                saw_assistant_updated = item_text == "hello world";
            }
            if event_type == "item.completed" && item_type == "agent_message" {
                saw_assistant_completed = item_text == "hello world";
            }
            if event_type == "item.started" && item_type == "reasoning" {
                saw_reasoning_started = item_text == "think";
            }
            if event_type == "item.completed" && item_type == "reasoning" {
                let stage = event
                    .get("item")
                    .and_then(|item| item.get("stage"))
                    .and_then(|stage| stage.as_str())
                    .unwrap_or_default();
                saw_reasoning_completed = item_text == "think" && stage == "analysis";
            }
        }

        assert!(saw_assistant_started);
        assert!(saw_assistant_updated);
        assert!(saw_assistant_completed);
        assert!(saw_reasoning_started);
        assert!(saw_reasoning_completed);
    }

    #[test]
    fn harness_streaming_bridge_abort_closes_open_items() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("harness.jsonl");
        let emitter =
            crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
                .expect("harness emitter");

        let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_456", 3, 2);
        bridge.on_progress(StreamProgressEvent::OutputDelta("partial".to_string()));
        bridge.abort();

        let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
        let completed_count = payload
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|value| {
                value
                    .get("event")
                    .and_then(|event| event.get("type"))
                    .and_then(|kind| kind.as_str())
                    == Some("item.completed")
            })
            .count();
        assert_eq!(completed_count, 1, "abort should close active stream item");
    }
}
