use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

#[derive(Copy, Clone)]
pub(super) struct ToolCatalogCacheMetrics<'a> {
    pub step_count: usize,
    pub model: &'a str,
    pub cache_hit: bool,
    pub plan_mode: bool,
    pub request_user_input_enabled: bool,
    pub available_tools: usize,
    pub stable_prefix_hash: u64,
    pub tool_catalog_hash: Option<u64>,
    pub prefix_change_reason: &'a str,
}

pub(super) fn emit_tool_catalog_cache_metrics(
    ctx: &TurnProcessingContext<'_>,
    metrics: ToolCatalogCacheMetrics<'_>,
) {
    tracing::info!(
        target: "vtcode.turn.metrics",
        metric = "tool_catalog_cache",
        run_id = %ctx.harness_state.run_id.0,
        turn_id = %ctx.harness_state.turn_id.0,
        turn = metrics.step_count,
        model = metrics.model,
        cache_hit = metrics.cache_hit,
        plan_mode = metrics.plan_mode,
        request_user_input_enabled = metrics.request_user_input_enabled,
        available_tools = metrics.available_tools,
        stable_prefix_hash = metrics.stable_prefix_hash,
        tool_catalog_hash = metrics.tool_catalog_hash,
        prefix_change_reason = metrics.prefix_change_reason,
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
        stable_prefix_hash: u64,
        tool_catalog_hash: Option<u64>,
        prefix_change_reason: &'a str,
        ts: i64,
    }

    ctx.traj.log(&ToolCatalogCacheRecord {
        kind: "tool_catalog_cache_metrics",
        turn: metrics.step_count,
        model: metrics.model,
        cache_hit: metrics.cache_hit,
        plan_mode: metrics.plan_mode,
        request_user_input_enabled: metrics.request_user_input_enabled,
        available_tools: metrics.available_tools,
        stable_prefix_hash: metrics.stable_prefix_hash,
        tool_catalog_hash: metrics.tool_catalog_hash,
        prefix_change_reason: metrics.prefix_change_reason,
        ts: chrono::Utc::now().timestamp(),
    });
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_llm_retry_metrics(
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
