//! Request classification: capability, tier-pricing, and reasoning-control helpers.

use super::*;

pub(crate) fn provider_error(message: impl Into<String>) -> LLMError {
    LLMError::Provider {
        message: error_display::format_llm_error("Merge Gateway", &message.into()),
        metadata: None,
    }
}

pub(crate) fn is_legacy_openai_base_url(base_url: &str) -> bool {
    let normalized = base_url.trim().trim_end_matches('/');
    normalized
        .split(['?', '#'])
        .next()
        .unwrap_or(normalized)
        .ends_with("/v1/openai")
}

/// Whether the request will put tool definitions on the wire. Tool-free
/// recovery keeps definitions for cache stability unless the route has no
/// tool-capable vendor (then definitions are omitted so synthesis can still
/// run). Only requests that actually send tools consult the no-tool-vendor
/// cache.
pub(crate) fn native_request_sends_tools(request: &LLMRequest, tool_vendor_missing: bool) -> bool {
    let tools_present = request.tools.as_ref().is_some_and(|tools| !tools.is_empty());
    let recovery_without_vendor = matches!(request.tool_choice, Some(ToolChoice::None)) && tool_vendor_missing;
    tools_present && !recovery_without_vendor
}

/// Builds the terminal error for a failed Merge Gateway request, appending
/// routing guidance when the route itself lacks a capable vendor: the fix is
/// a different route (e.g. `default_routing`), not a different request.
pub(crate) fn merge_request_error(status: StatusCode, body: &str) -> LLMError {
    if is_capability_unavailable(status, body) {
        provider_error(format!(
            "HTTP {status}: {body} Hint: Merge Gateway has no vendor serving this model with the requested capabilities yet; use default_routing or another model until the route gains one."
        ))
    } else {
        provider_error(format!("HTTP {status}: {body}"))
    }
}

/// Detects Merge Gateway `capability_unavailable` rejections: the resolved
/// route has no vendor serving the requested capability set (e.g. a brand-new
/// model with no streaming-tool vendor yet). Gateway fails these closed with
/// 400/422 instead of serving a downgraded request.
pub(crate) fn is_capability_unavailable(status: StatusCode, body: &str) -> bool {
    if !matches!(status, StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY) {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("capability_unavailable") || lower.contains("has no vendor that supports")
}

/// Whether a Merge Gateway route requires streaming. The `zai/` vendor
/// rejects non-streaming `/responses` requests fail-closed with 400
/// `streaming_only` ("Only streaming requests are supported").
pub(crate) fn is_streaming_only_model(model: &str) -> bool {
    model.trim().starts_with("zai/")
}

/// Detects Merge Gateway `streaming_only` rejections: the vendor behind the
/// route only serves streaming requests. Disjoint from
/// [`is_capability_unavailable`] (capability bodies name capabilities, never
/// the stream mode) and [`is_merge_tier_pricing_rejection`] (tier bodies name
/// the tier field).
pub(crate) fn is_streaming_only_rejection(status: StatusCode, body: &str) -> bool {
    if !matches!(status, StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY) {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("streaming_only") || lower.contains("only streaming requests")
}

/// Whether a `capability_unavailable` rejection names `reasoning` in the
/// requested capability set (e.g. `requested capabilities (['reasoning',
/// 'tools'])`). Such a rejection blames the reasoning+tools combination, not
/// tools alone, so it must not poison the no-tool-vendor verdict cache.
pub(crate) fn is_reasoning_capability_rejection(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    let Some(capabilities) = lower.find("capabilit").and_then(|start| {
        let rest = &lower[start..];
        let open = rest.find('[')?;
        let close = rest[open..].find(']')?;
        Some(&rest[open..open + close])
    }) else {
        return false;
    };
    capabilities
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| token == "reasoning")
}

/// Detects Merge Gateway tier-pricing rejections: the value is valid but the
/// route is not priced for it ("model does not support service tier 'flex'";
/// `priority` is priced nowhere and always fails). Fails closed with 400;
/// retrying without the tier serves standard. Disjoint from
/// [`is_capability_unavailable`]: capability bodies name capabilities, never
/// the tier field.
pub(crate) fn is_merge_tier_pricing_rejection(status: StatusCode, body: &str) -> bool {
    if status != StatusCode::BAD_REQUEST {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    (lower.contains("service_tier") || lower.contains("service tier"))
        && (lower.contains("does not support")
            || lower.contains("not priced")
            || lower.contains("fail closed")
            || lower.contains("fail-closed")
            || lower.contains("not available"))
}

/// Maps an OpenAI `service_tier` value onto Merge Gateway's tier vocabulary
/// (`standard`/`flex`/`priority`). OpenAI-only tiers (e.g. `ultrafast`) have
/// no Merge equivalent and map to `None` so the caller omits the field and
/// the gateway default applies instead of 422ing on `literal_error`.
pub(crate) fn map_openai_service_tier_for_merge(tier: &str) -> Option<&'static str> {
    if tier.eq_ignore_ascii_case("standard") {
        Some("standard")
    } else if tier.eq_ignore_ascii_case("flex") {
        Some("flex")
    } else if tier.eq_ignore_ascii_case("priority") {
        Some("priority")
    } else {
        None
    }
}

/// How a Merge Gateway route exposes reasoning controls. Merge Gateway routes
/// reasoning per provider: some vendors expose a provider-native
/// `reasoning_effort` parameter, others only accept a Gateway-managed thinking
/// budget through the top-level `thinking` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MergeReasoningControl {
    /// Provider-native `reasoning_effort` string on the payload root.
    ReasoningEffort,
    /// Gateway-controlled `thinking.budget_tokens` object on the payload root.
    ThinkingBudget,
}

/// Classifies a Merge Gateway route by its reasoning control surface. Unknown
/// routes stay conservative: reasoning is never forwarded for them.
///
/// `xiaomimimo/` routes are intentionally unclassified: the gateway currently
/// has no vendor serving reasoning jointly with tools for them, so forwarding
/// a `thinking` block turns every agentic request into a
/// `capability_unavailable` rejection.
///
/// `anthropic/claude-haiku-5-5` is unclassified for the same reason: the
/// gateway reports `has no vendor that supports the requested capabilities
/// (['reasoning', 'tools'])` for it, so reasoning controls are omitted until
/// the route gains a joint vendor.
pub(crate) fn merge_reasoning_control_for_model(model: &str) -> Option<MergeReasoningControl> {
    let model = model.trim();
    if model == models::merge_gateway::ANTHROPIC_CLAUDE_HAIKU_5_5 {
        return None;
    }
    if model.starts_with("openai/")
        || model.starts_with("xai/")
        || model.starts_with("moonshot/")
        || model.starts_with("zai/")
    {
        Some(MergeReasoningControl::ReasoningEffort)
    } else if model.starts_with("anthropic/")
        || model.starts_with("google/gemini-")
        || model.starts_with("deepseek/")
        || model.starts_with("qwen/")
        || model.starts_with("minimax/")
        || model.starts_with("thinkingmachines/")
    {
        Some(MergeReasoningControl::ThinkingBudget)
    } else {
        None
    }
}

/// Whether a configured reasoning effort actually puts reasoning controls on
/// the wire. `None` sends nothing and `Unknown` (an unrecognized config
/// value) is never forwarded, so neither can cause a reasoning-attributed
/// gateway rejection.
pub(crate) fn is_active_reasoning_effort(effort: vtcode_config::types::ReasoningEffortLevel) -> bool {
    !matches!(
        effort,
        vtcode_config::types::ReasoningEffortLevel::None | vtcode_config::types::ReasoningEffortLevel::Unknown
    )
}

/// Maps a reasoning effort level to a Gateway thinking budget in tokens,
/// mirroring the Anthropic budget mapping.
fn merge_thinking_budget(effort: vtcode_config::types::ReasoningEffortLevel) -> Option<u32> {
    match effort {
        vtcode_config::types::ReasoningEffortLevel::None | vtcode_config::types::ReasoningEffortLevel::Unknown => None,
        vtcode_config::types::ReasoningEffortLevel::Minimal => Some(1024),
        vtcode_config::types::ReasoningEffortLevel::Low => Some(4096),
        vtcode_config::types::ReasoningEffortLevel::Medium => Some(8192),
        vtcode_config::types::ReasoningEffortLevel::High => Some(16384),
        vtcode_config::types::ReasoningEffortLevel::XHigh | vtcode_config::types::ReasoningEffortLevel::Max => {
            Some(32768)
        }
    }
}

/// Builds the top-level `thinking` payload for a thinking-budget route. The
/// budget is clamped below `max_tokens`; when the output budget cannot fit even
/// a minimal thinking budget, thinking is omitted instead of erroring.
pub(crate) fn merge_thinking_payload(
    effort: vtcode_config::types::ReasoningEffortLevel,
    max_tokens: Option<u32>,
) -> Option<Value> {
    let budget = merge_thinking_budget(effort)?;
    let budget = match max_tokens {
        Some(max_tokens) if max_tokens > 0 => budget.min(max_tokens.saturating_sub(100)),
        _ => budget,
    };
    if budget < 1024 {
        return None;
    }
    Some(json!({ "type": "enabled", "budget_tokens": budget }))
}
