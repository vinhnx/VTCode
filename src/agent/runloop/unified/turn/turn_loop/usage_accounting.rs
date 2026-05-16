//! Turn-loop usage accounting and cost estimation helpers.

use vtcode_core::exec::events::Usage as HarnessUsage;
use vtcode_core::llm::model_resolver::ModelResolver;
use vtcode_core::llm::provider as uni;

pub(super) fn accumulate_turn_usage(total: &mut HarnessUsage, usage: &Option<uni::Usage>) {
    let Some(usage) = usage else {
        return;
    };

    total.input_tokens = total
        .input_tokens
        .saturating_add(usage.prompt_tokens as u64);
    total.cached_input_tokens = total
        .cached_input_tokens
        .saturating_add(usage.cache_read_tokens_or_fallback() as u64);
    total.cache_creation_tokens = total
        .cache_creation_tokens
        .saturating_add(usage.cache_creation_tokens_or_zero() as u64);
    total.output_tokens = total
        .output_tokens
        .saturating_add(usage.completion_tokens as u64);
}

pub(super) fn has_turn_usage(usage: &HarnessUsage) -> bool {
    usage.input_tokens > 0
        || usage.cached_input_tokens > 0
        || usage.cache_creation_tokens > 0
        || usage.output_tokens > 0
}

pub(super) fn stop_reason_from_finish_reason(finish_reason: &uni::FinishReason) -> String {
    match finish_reason {
        uni::FinishReason::Stop => "end_turn".to_string(),
        uni::FinishReason::Length => "max_tokens".to_string(),
        uni::FinishReason::ToolCalls => "tool_calls".to_string(),
        uni::FinishReason::ContentFilter => "content_filter".to_string(),
        uni::FinishReason::Pause => "pause_turn".to_string(),
        uni::FinishReason::Refusal => "refusal".to_string(),
        uni::FinishReason::Error(message) => message.clone(),
    }
}

pub(super) fn estimate_session_cost_usd(
    provider: &str,
    model: &str,
    usage: &HarnessUsage,
) -> Option<f64> {
    let usage = uni::Usage {
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