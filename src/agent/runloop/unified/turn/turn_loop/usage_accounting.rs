//! Turn-loop usage accounting and cost estimation helpers.

use vtcode_core::exec::events::Usage as HarnessUsage;
use vtcode_core::llm::provider as uni;
use vtcode_core::llm::usage_cost;

pub(super) fn accumulate_turn_usage(
    provider: &str,
    total: &mut HarnessUsage,
    usage: &Option<uni::Usage>,
) {
    let Some(usage) = usage else {
        return;
    };

    total.add(&usage_cost::normalized_turn_usage(provider, usage));
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

pub(super) fn estimate_session_costs(
    provider: &str,
    model: &str,
    usage: &HarnessUsage,
) -> Option<usage_cost::SessionCostEstimate> {
    usage_cost::estimate_session_costs(provider, model, usage)
}
