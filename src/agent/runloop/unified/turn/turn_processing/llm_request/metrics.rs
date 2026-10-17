use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use vtcode_core::llm::provider::{Message, ToolDefinition, Usage};

pub(super) fn prompt_cache_hit_ratio(provider: &str, usage: &Usage) -> f64 {
    vtcode_core::llm::usage_cost::normalized_turn_usage(provider, usage)
        .cache_hit_rate()
        .unwrap_or(0.0)
}

#[derive(Copy, Clone)]
pub(super) struct ToolCatalogCacheMetrics<'a> {
    pub step_count: usize,
    pub model: &'a str,
    pub cache_hit: bool,
    pub planning_active: bool,
    pub request_user_input_enabled: bool,
    pub available_tools: usize,
    pub stable_prefix_hash: u64,
    pub tool_catalog_hash: Option<u64>,
    pub prefix_change_reason: &'a str,
    pub ordered_wire_tool_names: Option<&'a [String]>,
    pub catalog_tool_count: Option<usize>,
    pub wire_tool_count: Option<usize>,
    pub deferred_tool_count: Option<usize>,
    pub active_loaded_skill_names: Option<&'a [String]>,
}

pub(super) fn emit_tool_catalog_cache_metrics(ctx: &TurnProcessingContext<'_>, metrics: ToolCatalogCacheMetrics<'_>) {
    tracing::info!(
        target: "vtcode.turn.metrics",
        metric = "tool_catalog_cache",
        run_id = %ctx.harness_state.run_id.0,
        turn_id = %ctx.harness_state.turn_id.0,
        turn = metrics.step_count,
        model = metrics.model,
        cache_hit = metrics.cache_hit,
        planning_workflow = metrics.planning_active,
        request_user_input_enabled = metrics.request_user_input_enabled,
        available_tools = metrics.available_tools,
        stable_prefix_hash = metrics.stable_prefix_hash,
        tool_catalog_hash = metrics.tool_catalog_hash,
        prefix_change_reason = metrics.prefix_change_reason,
        ordered_wire_tool_names = ?metrics.ordered_wire_tool_names,
        catalog_tool_count = metrics.catalog_tool_count,
        wire_tool_count = metrics.wire_tool_count,
        deferred_tool_count = metrics.deferred_tool_count,
        active_loaded_skill_names = ?metrics.active_loaded_skill_names,
        "turn metric"
    );

    #[derive(serde::Serialize)]
    struct ToolCatalogCacheRecord<'a> {
        kind: &'static str,
        turn: usize,
        model: &'a str,
        cache_hit: bool,
        planning_active: bool,
        request_user_input_enabled: bool,
        available_tools: usize,
        stable_prefix_hash: u64,
        tool_catalog_hash: Option<u64>,
        prefix_change_reason: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        ordered_wire_tool_names: Option<&'a [String]>,
        #[serde(skip_serializing_if = "Option::is_none")]
        catalog_tool_count: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        wire_tool_count: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        deferred_tool_count: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        active_loaded_skill_names: Option<&'a [String]>,
        ts: i64,
    }

    ctx.traj.log(&ToolCatalogCacheRecord {
        kind: "tool_catalog_cache_metrics",
        turn: metrics.step_count,
        model: metrics.model,
        cache_hit: metrics.cache_hit,
        planning_active: metrics.planning_active,
        request_user_input_enabled: metrics.request_user_input_enabled,
        available_tools: metrics.available_tools,
        stable_prefix_hash: metrics.stable_prefix_hash,
        tool_catalog_hash: metrics.tool_catalog_hash,
        prefix_change_reason: metrics.prefix_change_reason,
        ordered_wire_tool_names: metrics.ordered_wire_tool_names,
        catalog_tool_count: metrics.catalog_tool_count,
        wire_tool_count: metrics.wire_tool_count,
        deferred_tool_count: metrics.deferred_tool_count,
        active_loaded_skill_names: metrics.active_loaded_skill_names,
        ts: chrono::Utc::now().timestamp(),
    });
}

#[expect(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
pub(super) fn emit_llm_retry_metrics(
    ctx: &TurnProcessingContext<'_>,
    step_count: usize,
    model: &str,
    planning_active: bool,
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
        planning_active,
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
        planning_active: bool,
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
        planning_active,
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

/// Rough on-wire token estimate (~4 chars/token, matching the convention in
/// `estimate_schema_tokens` and the first-request budget test in
/// `tools/registry/builtins.rs`) for the tool schemas the model actually
/// receives. Serializes each `ToolDefinition` to its wire JSON so the estimate
/// reflects the real payload, including provider-native tool configs.
pub(super) fn estimate_tool_schema_tokens(tools: &[ToolDefinition]) -> usize {
    // Reuse a single buffer across all tools instead of allocating a fresh
    // `String` per definition. The buffer grows once to the largest serialized
    // tool and stays there for the rest of the call.
    let mut buf = Vec::new();
    tools
        .iter()
        .map(|tool| {
            buf.clear();
            serde_json::to_writer(&mut buf, tool)
                .map(|_| buf.len().div_ceil(4))
                .unwrap_or(0)
        })
        .sum()
}

/// Estimate the assembled message history using the provider message model's
/// existing token estimator. Unlike a text-only byte heuristic, it accounts
/// for tool calls, tool-call IDs, reasoning metadata, and image/file parts, so
/// structured wire content contributes to context-pressure accounting too.
pub(super) fn estimate_message_history_tokens(messages: &[Message]) -> usize {
    messages.iter().map(Message::estimate_tokens).sum()
}

/// Per-request token-budget breakdown for the assembled first-request prefix.
///
/// Closes the Phase 1.2 observability gap: a single metric recording how the
/// request prefix is spent across system prompt, tool schemas, and message
/// history, so token-overhead wins (deferred loading, lightweight subagent
/// profile) can be measured before/after. Cache read/write/miss fields are
/// already surfaced via `SessionStats` prompt-cache diagnostics and the
/// response-time usage path, so they are intentionally not duplicated here;
/// `subagent_bootstrap_tokens` is a spawn-time concern tracked separately.
#[derive(Copy, Clone)]
pub(super) struct TokenBudgetBreakdown<'a> {
    pub step_count: usize,
    pub model: &'a str,
    pub system_prompt_tokens: usize,
    pub tool_schema_tokens: usize,
    pub message_history_tokens: usize,
    pub on_wire_tools: usize,
    pub client_local_deferral: bool,
    pub tool_free_recovery: bool,
    /// True when this is the session's first assembled LLM request.
    pub first_call: bool,
}

impl TokenBudgetBreakdown<'_> {
    /// Per-call harness tax before the task prompt (instructions + tool schemas).
    pub(super) fn fixed_overhead_tokens(&self) -> usize {
        self.system_prompt_tokens.saturating_add(self.tool_schema_tokens)
    }
}

pub(super) fn emit_token_budget_breakdown(ctx: &TurnProcessingContext<'_>, breakdown: TokenBudgetBreakdown<'_>) {
    let fixed_overhead_tokens = breakdown.fixed_overhead_tokens();
    tracing::info!(
        target: "vtcode.turn.metrics",
        metric = "token_budget_breakdown",
        run_id = %ctx.harness_state.run_id.0,
        turn_id = %ctx.harness_state.turn_id.0,
        turn = breakdown.step_count,
        model = breakdown.model,
        system_prompt_tokens = breakdown.system_prompt_tokens,
        tool_schema_tokens = breakdown.tool_schema_tokens,
        message_history_tokens = breakdown.message_history_tokens,
        on_wire_tools = breakdown.on_wire_tools,
        client_local_deferral = breakdown.client_local_deferral,
        tool_free_recovery = breakdown.tool_free_recovery,
        first_call = breakdown.first_call,
        fixed_overhead_tokens,
        "turn metric"
    );

    #[derive(serde::Serialize)]
    struct TokenBudgetBreakdownRecord<'a> {
        kind: &'static str,
        turn: usize,
        model: &'a str,
        system_prompt_tokens: usize,
        tool_schema_tokens: usize,
        message_history_tokens: usize,
        on_wire_tools: usize,
        client_local_deferral: bool,
        tool_free_recovery: bool,
        first_call: bool,
        fixed_overhead_tokens: usize,
        ts: i64,
    }

    ctx.traj.log(&TokenBudgetBreakdownRecord {
        kind: "token_budget_breakdown",
        turn: breakdown.step_count,
        model: breakdown.model,
        system_prompt_tokens: breakdown.system_prompt_tokens,
        tool_schema_tokens: breakdown.tool_schema_tokens,
        message_history_tokens: breakdown.message_history_tokens,
        on_wire_tools: breakdown.on_wire_tools,
        client_local_deferral: breakdown.client_local_deferral,
        tool_free_recovery: breakdown.tool_free_recovery,
        first_call: breakdown.first_call,
        fixed_overhead_tokens,
        ts: chrono::Utc::now().timestamp(),
    });
}

#[cfg(test)]
mod tests;
