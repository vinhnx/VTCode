//! Model capability detection for Anthropic Claude models
//!
//! Provides methods to determine what features each Claude model supports:
//! - Reasoning/extended thinking
//! - Vision (image inputs)
//! - Structured outputs
//! - Parallel tool configuration
//! - Context window sizes

use crate::providers::anthropic_types::{ThinkingConfig, ThinkingDisplay};
use vtcode_config::constants::{models, reasoning};

const CLAUDE_OPUS_4_8: &str = "claude-opus-4-8";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudeThinkingMode {
    ManualBudget,
    Adaptive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClaudeThinkingProfile {
    pub mode: ClaudeThinkingMode,
    pub supports_manual_budget: bool,
    pub adaptive_only: bool,
    pub default_thinking_enabled: bool,
    pub manual_interleaved_beta: bool,
    pub supports_effort: bool,
    pub supports_task_budget: bool,
    pub default_display: ThinkingDisplay,
    pub default_effort: &'static str,
    pub supports_xhigh_effort: bool,
    pub supports_max_effort: bool,
    /// `max_tokens` sent when the caller does not set one. It caps thinking
    /// plus response text together, so it must leave room for adaptive
    /// thinking on agentic turns while staying within the model's output limit.
    pub default_max_tokens: u32,
    /// Whether `tool_choice` `any`/`tool` is rejected with a 400 regardless
    /// of the thinking config. Such requests must fall back to `auto`.
    pub rejects_forced_tool_choice: bool,
    /// Whether the model accepts server-side refusal fallbacks (the
    /// `fallbacks` request parameter). Sonnet 5 is not listed as supported.
    pub supports_server_side_fallback: bool,
    /// Whether `thinking: {"type": "disabled"}` is rejected with a 400. Such
    /// models take `{"type": "between_tools"}` as their lowest setting, which
    /// keeps up-front thinking off while still returning the progress notes
    /// written between tool calls. Adaptive-only models reject every
    /// "thinking off" request, so they leave this `false`.
    pub rejects_disabled_thinking: bool,
}

/// Default `max_tokens` for Claude 5.x models: a starting point for agentic
/// coding that stays within their 128K output limit.
const CLAUDE_5_DEFAULT_MAX_TOKENS: u32 = 64_000;
/// Default `max_tokens` for unprofiled models when thinking is enabled.
const LEGACY_THINKING_DEFAULT_MAX_TOKENS: u32 = 16_000;
/// Default `max_tokens` for unprofiled models when thinking is disabled.
const LEGACY_DEFAULT_MAX_TOKENS: u32 = 4_096;

const ANTHROPIC_EFFORTS_UP_TO_HIGH: &[&str] = &[reasoning::LOW, reasoning::MEDIUM, reasoning::HIGH];
const ANTHROPIC_EFFORTS_UP_TO_MAX: &[&str] = &[reasoning::LOW, reasoning::MEDIUM, reasoning::HIGH, reasoning::MAX];
const ANTHROPIC_EFFORTS_UP_TO_XHIGH_AND_MAX: &[&str] = &[
    reasoning::LOW,
    reasoning::MEDIUM,
    reasoning::HIGH,
    reasoning::XHIGH,
    reasoning::MAX,
];

pub(crate) fn resolve_model_name<'a>(model: &'a str, default_model: &'a str) -> &'a str {
    if model.trim().is_empty() { default_model } else { model }
}

pub(crate) fn matches_model(model: &str, candidate: &str) -> bool {
    model == candidate || model.contains(candidate)
}

pub(crate) fn claude_thinking_profile(model: &str, default_model: &str) -> Option<ClaudeThinkingProfile> {
    let requested = resolve_model_name(model, default_model);

    // Check most specific models first – `matches_model` uses `contains`,
    // so `claude-fable-5-1` contains `claude-fable-5`. The 5.1 variants
    // must be checked before their 5 counterparts.
    if matches_model(requested, models::anthropic::CLAUDE_FABLE_5_1) {
        return Some(ClaudeThinkingProfile {
            mode: ClaudeThinkingMode::Adaptive,
            supports_manual_budget: false,
            adaptive_only: true,
            default_thinking_enabled: true,
            manual_interleaved_beta: false,
            supports_effort: true,
            supports_task_budget: true,
            default_display: ThinkingDisplay::Omitted,
            default_effort: reasoning::HIGH,
            supports_xhigh_effort: true,
            supports_max_effort: true,
            default_max_tokens: CLAUDE_5_DEFAULT_MAX_TOKENS,
            rejects_forced_tool_choice: true,
            supports_server_side_fallback: true,
            rejects_disabled_thinking: false,
        });
    }

    // `claude-sonnet-5-5` contains `claude-sonnet-5`, so the 5.5 profile must
    // be checked before the 5 profile. Sonnet 5.5 runs adaptive thinking by
    // default but rejects `disabled`: its lowest setting is `between_tools`,
    // which also fails at `xhigh`/`max` effort. Forced tool use and sampling
    // parameters are rejected outright.
    if matches_model(requested, models::anthropic::CLAUDE_SONNET_5_5) {
        return Some(ClaudeThinkingProfile {
            mode: ClaudeThinkingMode::Adaptive,
            supports_manual_budget: false,
            adaptive_only: false,
            default_thinking_enabled: true,
            manual_interleaved_beta: false,
            supports_effort: true,
            supports_task_budget: false,
            default_display: ThinkingDisplay::Omitted,
            default_effort: reasoning::HIGH,
            supports_xhigh_effort: true,
            supports_max_effort: true,
            default_max_tokens: CLAUDE_5_DEFAULT_MAX_TOKENS,
            rejects_forced_tool_choice: true,
            supports_server_side_fallback: true,
            rejects_disabled_thinking: true,
        });
    }

    if matches_model(requested, models::anthropic::CLAUDE_SONNET_5) {
        return Some(ClaudeThinkingProfile {
            mode: ClaudeThinkingMode::Adaptive,
            supports_manual_budget: false,
            adaptive_only: false,
            default_thinking_enabled: true,
            manual_interleaved_beta: false,
            supports_effort: true,
            supports_task_budget: false,
            default_display: ThinkingDisplay::Omitted,
            default_effort: reasoning::HIGH,
            supports_xhigh_effort: true,
            supports_max_effort: true,
            default_max_tokens: CLAUDE_5_DEFAULT_MAX_TOKENS,
            rejects_forced_tool_choice: false,
            supports_server_side_fallback: false,
            rejects_disabled_thinking: false,
        });
    }

    // Claude Haiku 5.5 runs adaptive thinking by default with a `medium`
    // default effort. Unlike Sonnet 5.5/Opus 5.5/Fable 5.1 it accepts forced
    // `tool_choice` (the response starts with the tool call and has no
    // thinking block) and it has no server-side refusal fallback.
    if matches_model(requested, models::anthropic::CLAUDE_HAIKU_5_5) {
        return Some(ClaudeThinkingProfile {
            mode: ClaudeThinkingMode::Adaptive,
            supports_manual_budget: false,
            adaptive_only: false,
            default_thinking_enabled: true,
            manual_interleaved_beta: false,
            supports_effort: true,
            supports_task_budget: false,
            default_display: ThinkingDisplay::Omitted,
            default_effort: reasoning::MEDIUM,
            supports_xhigh_effort: true,
            supports_max_effort: true,
            default_max_tokens: CLAUDE_5_DEFAULT_MAX_TOKENS,
            rejects_forced_tool_choice: false,
            supports_server_side_fallback: false,
            rejects_disabled_thinking: false,
        });
    }

    if matches_model(requested, models::anthropic::CLAUDE_FABLE_5) {
        return Some(ClaudeThinkingProfile {
            mode: ClaudeThinkingMode::Adaptive,
            supports_manual_budget: false,
            adaptive_only: true,
            default_thinking_enabled: true,
            manual_interleaved_beta: false,
            supports_effort: true,
            supports_task_budget: true,
            default_display: ThinkingDisplay::Omitted,
            default_effort: reasoning::HIGH,
            supports_xhigh_effort: true,
            supports_max_effort: true,
            default_max_tokens: CLAUDE_5_DEFAULT_MAX_TOKENS,
            rejects_forced_tool_choice: false,
            supports_server_side_fallback: true,
            rejects_disabled_thinking: false,
        });
    }

    // `claude-opus-5-5` contains `claude-opus-5`, so the 5.5 profile must be
    // checked before the 5 profile. Opus 5.5 runs adaptive thinking always on
    // (it cannot be disabled) with a `medium` default effort.
    if matches_model(requested, models::anthropic::CLAUDE_OPUS_5_5) {
        return Some(ClaudeThinkingProfile {
            mode: ClaudeThinkingMode::Adaptive,
            supports_manual_budget: false,
            adaptive_only: true,
            default_thinking_enabled: true,
            manual_interleaved_beta: false,
            supports_effort: true,
            supports_task_budget: true,
            default_display: ThinkingDisplay::Omitted,
            default_effort: reasoning::MEDIUM,
            supports_xhigh_effort: true,
            supports_max_effort: true,
            default_max_tokens: CLAUDE_5_DEFAULT_MAX_TOKENS,
            rejects_forced_tool_choice: true,
            supports_server_side_fallback: true,
            rejects_disabled_thinking: false,
        });
    }

    if matches_model(requested, models::anthropic::CLAUDE_OPUS_5) {
        return Some(ClaudeThinkingProfile {
            mode: ClaudeThinkingMode::Adaptive,
            supports_manual_budget: false,
            adaptive_only: false,
            default_thinking_enabled: true,
            manual_interleaved_beta: false,
            supports_effort: true,
            supports_task_budget: true,
            default_display: ThinkingDisplay::Omitted,
            default_effort: reasoning::HIGH,
            supports_xhigh_effort: true,
            supports_max_effort: true,
            default_max_tokens: CLAUDE_5_DEFAULT_MAX_TOKENS,
            rejects_forced_tool_choice: false,
            supports_server_side_fallback: true,
            rejects_disabled_thinking: false,
        });
    }

    None
}

/// Claude 5.x family ids. `matches_model` uses `contains`, so the Fable 5 id
/// also matches `claude-fable-5-1`, the Opus 5 id matches `claude-opus-5-5`,
/// and the Sonnet 5 id matches `claude-sonnet-5-5`. The 5.5 ids (including
/// Haiku 5.5) are listed explicitly so the family stays readable as it grows.
const CLAUDE_5_FAMILY: &[&str] = &[
    models::anthropic::CLAUDE_SONNET_5_5,
    models::anthropic::CLAUDE_SONNET_5,
    models::anthropic::CLAUDE_HAIKU_5_5,
    models::anthropic::CLAUDE_FABLE_5,
    models::anthropic::CLAUDE_OPUS_5_5,
    models::anthropic::CLAUDE_OPUS_5,
];

fn is_claude_5_family(model: &str) -> bool {
    CLAUDE_5_FAMILY.iter().any(|candidate| matches_model(model, candidate))
}

fn supports_native_1m_context(model: &str) -> bool {
    is_claude_5_family(model)
}

pub(crate) fn supports_reasoning(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);
    if claude_thinking_profile(requested, default_model).is_some() {
        return true;
    }

    models::minimax::SUPPORTED_MODELS.contains(&requested)
}

pub(crate) fn supports_reasoning_effort(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);

    if claude_thinking_profile(requested, default_model).is_some() {
        return true;
    }

    if models::minimax::SUPPORTED_MODELS.contains(&requested) {
        return true;
    }

    models::anthropic::REASONING_MODELS.contains(&requested)
}

pub(crate) fn supports_effort(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model).is_some_and(|profile| profile.supports_effort)
}

pub(crate) fn supports_task_budget(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model).is_some_and(|profile| profile.supports_task_budget)
}

pub(crate) fn supports_manual_thinking_budget(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model).is_some_and(|profile| profile.supports_manual_budget)
}

pub(crate) fn supports_manual_interleaved_beta(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model).is_some_and(|profile| profile.manual_interleaved_beta)
}

/// Whether the model accepts a request whose last message is an assistant
/// turn (assistant-message prefill). A trailing assistant turn returns 400 on
/// Claude Opus/Sonnet 4.6 and later, which covers every profiled Claude 5.x
/// model. Older Claude models and non-Claude Anthropic-compatible backends
/// (MiniMax and similar) accept it, so their history is sent unchanged. A
/// Claude id without a recognizable version is treated as a current model.
pub(crate) fn supports_assistant_prefill(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);
    if claude_thinking_profile(requested, default_model).is_some() {
        return false;
    }

    let lowered = requested.to_ascii_lowercase();
    let Some(start) = lowered.find("claude") else {
        return true;
    };
    claude_version(&lowered[start..]).is_some_and(|version| version < (4, 6))
}

/// Extracts `(major, minor)` from a Claude model id such as
/// `claude-opus-4-1`, `claude-3-5-sonnet-20240620`, `claude-sonnet-4@20250514`
/// or `claude-2.1`. The first short numeric segment is the major version and a
/// directly following short numeric segment is the minor version; eight-digit
/// date snapshots are not version segments.
fn claude_version(id: &str) -> Option<(u32, u32)> {
    let mut segments = id
        .split(['-', '.', '@', ':', '_'])
        .skip_while(|segment| !is_version_segment(segment));
    let major = segments.next()?.parse().ok()?;
    let minor = segments
        .next()
        .filter(|segment| is_version_segment(segment))
        .and_then(|segment| segment.parse().ok())
        .unwrap_or(0);
    Some((major, minor))
}

fn is_version_segment(segment: &str) -> bool {
    (1..=2).contains(&segment.len()) && segment.bytes().all(|b| b.is_ascii_digit())
}

pub(crate) fn supports_mid_conversation_system_messages(model: &str, default_model: &str) -> bool {
    supports_turn_scoped_system_messages(model, default_model)
}

/// Whether the model accepts Anthropic's turn-scoped `clear_at` system-message
/// field. The current beta is available to the model families that accept
/// mid-conversation system messages, but not Sonnet 5 (Sonnet 5.5 adds it
/// alongside per-message effort and mid-conversation tool changes).
pub(crate) fn supports_turn_scoped_system_messages(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);
    matches_model(requested, models::anthropic::CLAUDE_FABLE_5)
        || matches_model(requested, CLAUDE_OPUS_4_8)
        || matches_model(requested, models::anthropic::CLAUDE_OPUS_5)
        || matches_model(requested, models::anthropic::CLAUDE_SONNET_5_5)
}

pub(crate) fn adaptive_thinking_always_on(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model).is_some_and(|profile| profile.adaptive_only)
}

/// The `max_tokens` to send when the request does not set one. Profiled
/// models use their profile default regardless of the thinking field, since
/// they think adaptively even when it is omitted; other models fall back to
/// the legacy defaults.
pub(crate) fn default_max_tokens_for_model(model: &str, default_model: &str, thinking_enabled: bool) -> u32 {
    match claude_thinking_profile(model, default_model) {
        Some(profile) => profile.default_max_tokens,
        None if thinking_enabled => LEGACY_THINKING_DEFAULT_MAX_TOKENS,
        None => LEGACY_DEFAULT_MAX_TOKENS,
    }
}

/// Whether `model` rejects forced tool use (`tool_choice` `any`/`tool`)
/// even when thinking is off.
pub(crate) fn rejects_forced_tool_choice(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model).is_some_and(|profile| profile.rejects_forced_tool_choice)
}

/// Whether `model` accepts server-side refusal fallbacks. Unprofiled models
/// never get fallbacks requested on their behalf.
pub(crate) fn supports_server_side_fallback(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model).is_some_and(|profile| profile.supports_server_side_fallback)
}

/// Whether a request to `model` with this `thinking` field runs with thinking
/// on. An omitted field means the model's default, which is on for every
/// profiled Claude 5.x model (Opus 5.5 omits `disabled` because it rejects it).
pub(crate) fn thinking_is_on(thinking: Option<&ThinkingConfig>, model: &str, default_model: &str) -> bool {
    match thinking {
        // `between_tools` turns up-front thinking off; only the progress notes
        // written between tool calls still come back as thinking blocks.
        Some(ThinkingConfig::Disabled) | Some(ThinkingConfig::BetweenTools) => false,
        // Unknown configs are treated as thinking so callers stay conservative.
        Some(_) => true,
        None => claude_thinking_profile(model, default_model).is_some_and(|profile| profile.default_thinking_enabled),
    }
}

/// Whether the model enforces "preserved thinking": each replayed thinking
/// block's signature binds the exact prefix that produced it (top-level
/// `system`, the `tools` set, and every earlier message). Any reorder or edit
/// of earlier turns invalidates every later thinking block (a 400 on
/// enforced accounts, a silent drop otherwise), so request builders must keep
/// history append-only for these models.
pub(crate) fn preserves_thinking_across_turns(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);
    matches_model(requested, models::anthropic::CLAUDE_SONNET_5_5)
        || matches_model(requested, models::anthropic::CLAUDE_HAIKU_5_5)
        || matches_model(requested, models::anthropic::CLAUDE_OPUS_5_5)
        || matches_model(requested, models::anthropic::CLAUDE_FABLE_5_1)
}

/// Whether the model accepts `thinking.display: "updates"` (beta
/// `thinking-display-updates-2026-08-18`): its between-tool progress notes
/// come back as their own `thinking` blocks. Other models reject the value.
/// `claude-fable-5-1` contains the Fable 5 id, and `claude-sonnet-5-5` and
/// `claude-opus-5-5` contain their 5-generation ids, so all four releases match.
pub(crate) fn supports_thinking_display_updates(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);
    matches_model(requested, models::anthropic::CLAUDE_SONNET_5_5)
        || matches_model(requested, models::anthropic::CLAUDE_OPUS_5_5)
        || matches_model(requested, models::anthropic::CLAUDE_FABLE_5)
}

/// Display used when neither the request nor `[provider.anthropic]` sets one.
/// On Claude Opus 5.5 and Claude Sonnet 5.5 the text written between tool calls
/// arrives only as progress-update thinking blocks, which the API default
/// (`omitted`) empties; requesting `updates` keeps that narration visible in
/// the reasoning view while the reasoning itself stays hidden.
pub(crate) fn default_thinking_display(model: &str, default_model: &str) -> Option<ThinkingDisplay> {
    let requested = resolve_model_name(model, default_model);
    (matches_model(requested, models::anthropic::CLAUDE_SONNET_5_5)
        || matches_model(requested, models::anthropic::CLAUDE_OPUS_5_5))
    .then_some(ThinkingDisplay::Updates)
}

pub(crate) fn default_effort_for_model(model: &str, default_model: &str) -> Option<&'static str> {
    claude_thinking_profile(model, default_model)
        .filter(|profile| profile.supports_effort)
        .map(|profile| profile.default_effort)
}

pub(crate) fn allowed_efforts_for_model(model: &str, default_model: &str) -> Option<&'static [&'static str]> {
    let profile = claude_thinking_profile(model, default_model)?;
    if !profile.supports_effort {
        return None;
    }

    if profile.supports_xhigh_effort {
        Some(ANTHROPIC_EFFORTS_UP_TO_XHIGH_AND_MAX)
    } else if profile.supports_max_effort {
        Some(ANTHROPIC_EFFORTS_UP_TO_MAX)
    } else {
        Some(ANTHROPIC_EFFORTS_UP_TO_HIGH)
    }
}

pub(crate) fn effort_allowed_for_model(model: &str, default_model: &str, effort: &str) -> bool {
    let normalized = effort.trim().to_ascii_lowercase();
    allowed_efforts_for_model(model, default_model).is_some_and(|allowed| allowed.contains(&normalized.as_str()))
}

pub(crate) fn supports_compaction(model: &str) -> bool {
    is_claude_5_family(model)
}

pub(crate) fn supports_parallel_tool_config(_model: &str) -> bool {
    true
}

pub fn effective_context_size(model: &str) -> usize {
    if supports_native_1m_context(model) {
        1_000_000
    } else {
        200_000
    }
}

pub(crate) fn rejects_sampling(model: &str, default_model: &str) -> bool {
    is_claude_5_family(resolve_model_name(model, default_model))
}

/// Pre-5.x Claude models without a thinking profile that still accept
/// structured outputs.
const LEGACY_STRUCTURED_OUTPUT_MODELS: &[&str] = &["claude-sonnet-4-5", "claude-opus-4-5"];

pub(crate) fn supports_structured_output(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);

    // All models with a thinking profile support structured outputs.
    if claude_thinking_profile(requested, default_model).is_some() {
        return true;
    }

    LEGACY_STRUCTURED_OUTPUT_MODELS
        .iter()
        .any(|candidate| matches_model(requested, candidate))
}

/// Model ids that accept structured outputs, derived from
/// `supports_structured_output` so user-facing errors cannot drift from it:
/// every supported model with a thinking profile, then the legacy ids.
pub(crate) fn structured_output_models() -> Vec<&'static str> {
    models::anthropic::SUPPORTED_MODELS
        .iter()
        .copied()
        .filter(|model| supports_structured_output(model, ""))
        .chain(LEGACY_STRUCTURED_OUTPUT_MODELS.iter().copied())
        .collect()
}

pub(crate) fn supports_vision(model: &str, default_model: &str) -> bool {
    let requested = resolve_model_name(model, default_model);

    // All models with a thinking profile support vision.
    if claude_thinking_profile(requested, default_model).is_some() {
        return true;
    }

    // Legacy Claude 3 and Claude 4 Sonnet families support vision.
    requested.starts_with("claude-3") || requested.starts_with("claude-4-sonnet")
}

pub fn is_claude_model(model: &str, default_model: &str) -> bool {
    claude_thinking_profile(model, default_model).is_some()
}

pub(crate) fn supported_models() -> Vec<String> {
    let mut supported: Vec<String> = models::anthropic::SUPPORTED_MODELS.iter().map(|s| s.to_string()).collect();

    supported.extend(models::minimax::SUPPORTED_MODELS.iter().map(|s| s.to_string()));

    supported.sort();
    supported.dedup();
    supported
}

/// Returns true if the effective effort for this request is "low", "medium", or "high"
/// (i.e., at most high, not xhigh or max).
///
/// This is used by Opus 5 disabled-thinking validation: Opus 5 only allows
/// `thinking: {type: "disabled"}` at effort ≤ high. When the override is `Omit`,
/// the API uses the model default effort, so we check that instead of the config.
pub(crate) fn effort_is_at_most_high(
    request: &crate::provider::LLMRequest,
    anthropic_config: &vtcode_config::core::AnthropicConfig,
) -> bool {
    use crate::provider::{AnthropicOptionalStringOverride, LLMRequest};
    use vtcode_config::types::ReasoningEffortLevel;

    if let Some(overrides) = request.anthropic_request_overrides.as_ref() {
        match &overrides.effort {
            AnthropicOptionalStringOverride::Explicit(effort) => {
                return effort_str_is_at_most_high(effort);
            }
            AnthropicOptionalStringOverride::Omit => {
                return default_effort_for_model(&request.model, "").is_some_and(effort_str_is_at_most_high);
            }
            AnthropicOptionalStringOverride::Inherit => {}
        }
    }

    if let Some(effort) = request.effort.as_ref() {
        return effort_str_is_at_most_high(effort);
    }
    if let Some(effort) = request.reasoning_effort {
        return matches!(effort, ReasoningEffortLevel::Low | ReasoningEffortLevel::Medium | ReasoningEffortLevel::High);
    }
    match anthropic_config.effort {
        Some(effort) => effort_str_is_at_most_high(effort.as_str()),
        None => default_effort_for_model(&request.model, "").is_some_and(effort_str_is_at_most_high),
    }
}

/// Returns true if `effort` names one of the `low`, `medium`, or `high`
/// levels (case-insensitive).
pub(crate) fn effort_str_is_at_most_high(effort: &str) -> bool {
    let normalized = effort.trim().to_ascii_lowercase();
    matches!(normalized.as_str(), reasoning::LOW | reasoning::MEDIUM | reasoning::HIGH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_5_5_runs_adaptive_thinking_always_on_with_medium_default() {
        let profile = claude_thinking_profile(models::anthropic::CLAUDE_OPUS_5_5, "").expect("opus 5.5 profile");
        assert!(profile.adaptive_only);
        assert_eq!(profile.default_effort, reasoning::MEDIUM);
        assert!(profile.supports_xhigh_effort);
        assert!(profile.supports_max_effort);
        assert!(adaptive_thinking_always_on(models::anthropic::CLAUDE_OPUS_5_5, ""));
        assert_eq!(default_effort_for_model(models::anthropic::CLAUDE_OPUS_5_5, ""), Some(reasoning::MEDIUM));
    }

    #[test]
    fn sonnet_5_5_keeps_default_thinking_but_rejects_disabled() {
        // `claude-sonnet-5-5` contains `claude-sonnet-5`; the 5.5 profile must
        // win for the 5.5 id: `disabled` is a 400 there, `between_tools` is not.
        let profile = claude_thinking_profile(models::anthropic::CLAUDE_SONNET_5_5, "").expect("sonnet 5.5 profile");
        assert!(!profile.adaptive_only);
        assert!(profile.default_thinking_enabled);
        assert!(!profile.supports_manual_budget);
        assert!(profile.rejects_disabled_thinking);
        assert_eq!(profile.default_effort, reasoning::HIGH);
        assert!(!profile.supports_task_budget);
        assert!(profile.rejects_forced_tool_choice);
        assert!(profile.supports_server_side_fallback);
        assert!(!adaptive_thinking_always_on(models::anthropic::CLAUDE_SONNET_5_5, ""));
        assert!(thinking_is_on(None, models::anthropic::CLAUDE_SONNET_5_5, ""));
        assert!(!thinking_is_on(Some(&ThinkingConfig::Disabled), models::anthropic::CLAUDE_SONNET_5_5, ""));
        assert!(!thinking_is_on(Some(&ThinkingConfig::BetweenTools), models::anthropic::CLAUDE_SONNET_5_5, ""));
    }

    #[test]
    fn sonnet_5_profile_is_unchanged_by_sonnet_5_5() {
        let profile = claude_thinking_profile(models::anthropic::CLAUDE_SONNET_5, "").expect("sonnet 5 profile");
        assert!(!profile.adaptive_only);
        assert_eq!(profile.default_effort, reasoning::HIGH);
        assert!(!profile.rejects_disabled_thinking);
        assert!(!profile.rejects_forced_tool_choice);
    }

    #[test]
    fn opus_5_profile_is_unchanged_by_opus_5_5() {
        // `claude-opus-5-5` contains `claude-opus-5`; the 5 profile must still
        // resolve for the exact 5 id with its high default and opt-out support.
        let profile = claude_thinking_profile(models::anthropic::CLAUDE_OPUS_5, "").expect("opus 5 profile");
        assert!(!profile.adaptive_only);
        assert_eq!(profile.default_effort, reasoning::HIGH);
        assert!(!adaptive_thinking_always_on(models::anthropic::CLAUDE_OPUS_5, ""));
    }

    #[test]
    fn claude_5_models_default_to_64k_max_tokens_regardless_of_thinking() {
        for model in [
            models::anthropic::CLAUDE_FABLE_5_1,
            models::anthropic::CLAUDE_SONNET_5_5,
            models::anthropic::CLAUDE_SONNET_5,
            models::anthropic::CLAUDE_HAIKU_5_5,
            models::anthropic::CLAUDE_FABLE_5,
            models::anthropic::CLAUDE_OPUS_5_5,
            models::anthropic::CLAUDE_OPUS_5,
        ] {
            let profile = claude_thinking_profile(model, "").expect("claude 5.x profile");
            assert_eq!(profile.default_max_tokens, 64_000, "{model}");
            assert_eq!(default_max_tokens_for_model(model, "", true), 64_000, "{model}");
            assert_eq!(default_max_tokens_for_model(model, "", false), 64_000, "{model}");
        }
    }

    #[test]
    fn forced_tool_choice_is_rejected_by_sonnet_5_5_opus_5_5_and_fable_5_1() {
        for model in [
            models::anthropic::CLAUDE_SONNET_5_5,
            models::anthropic::CLAUDE_OPUS_5_5,
            models::anthropic::CLAUDE_FABLE_5_1,
        ] {
            assert!(rejects_forced_tool_choice(model, ""), "{model}");
        }
        for model in [
            models::anthropic::CLAUDE_SONNET_5,
            models::anthropic::CLAUDE_HAIKU_5_5,
            models::anthropic::CLAUDE_FABLE_5,
            models::anthropic::CLAUDE_OPUS_5,
            "claude-unlisted-model",
        ] {
            assert!(!rejects_forced_tool_choice(model, ""), "{model}");
        }
    }

    #[test]
    fn haiku_5_5_runs_adaptive_thinking_with_medium_default_and_no_fallback() {
        let profile = claude_thinking_profile(models::anthropic::CLAUDE_HAIKU_5_5, "").expect("haiku 5.5 profile");
        assert_eq!(profile.mode, ClaudeThinkingMode::Adaptive);
        assert!(!profile.adaptive_only);
        assert!(profile.default_thinking_enabled);
        assert!(!profile.supports_manual_budget);
        assert!(profile.supports_effort);
        assert_eq!(profile.default_effort, reasoning::MEDIUM);
        assert_eq!(default_effort_for_model(models::anthropic::CLAUDE_HAIKU_5_5, ""), Some(reasoning::MEDIUM));
        assert!(!profile.rejects_forced_tool_choice);
        assert!(!profile.supports_server_side_fallback);
        assert!(!profile.rejects_disabled_thinking);
        assert!(!adaptive_thinking_always_on(models::anthropic::CLAUDE_HAIKU_5_5, ""));
    }

    #[test]
    fn omitted_thinking_follows_the_model_default() {
        assert!(thinking_is_on(None, models::anthropic::CLAUDE_OPUS_5_5, ""));
        assert!(!thinking_is_on(None, "claude-unlisted-model", ""));
        assert!(!thinking_is_on(Some(&ThinkingConfig::Disabled), models::anthropic::CLAUDE_SONNET_5, ""));
        assert!(thinking_is_on(Some(&ThinkingConfig::Adaptive { display: None }), "claude-unlisted-model", ""));
    }

    #[test]
    fn unprofiled_models_keep_legacy_max_tokens_defaults() {
        assert_eq!(default_max_tokens_for_model("claude-unlisted-model", "", true), 16_000);
        assert_eq!(default_max_tokens_for_model("claude-unlisted-model", "", false), 4_096);
    }

    #[test]
    fn empty_model_uses_the_default_model_max_tokens() {
        assert_eq!(default_max_tokens_for_model("", models::anthropic::CLAUDE_SONNET_5, false), 64_000);
    }

    #[test]
    fn preserved_thinking_is_limited_to_prefix_bound_models() {
        for model in [
            models::anthropic::CLAUDE_SONNET_5_5,
            models::anthropic::CLAUDE_HAIKU_5_5,
            models::anthropic::CLAUDE_OPUS_5_5,
            models::anthropic::CLAUDE_FABLE_5_1,
        ] {
            assert!(preserves_thinking_across_turns(model, ""), "{model}");
        }
        assert!(preserves_thinking_across_turns("", models::anthropic::CLAUDE_SONNET_5_5));
        for model in [
            models::anthropic::CLAUDE_SONNET_5,
            models::anthropic::CLAUDE_OPUS_5,
            models::anthropic::CLAUDE_FABLE_5,
        ] {
            assert!(!preserves_thinking_across_turns(model, ""), "{model}");
        }
    }

    #[test]
    fn thinking_display_updates_support_and_default() {
        for model in [
            models::anthropic::CLAUDE_SONNET_5_5,
            models::anthropic::CLAUDE_OPUS_5_5,
            models::anthropic::CLAUDE_FABLE_5_1,
            models::anthropic::CLAUDE_FABLE_5,
        ] {
            assert!(supports_thinking_display_updates(model, ""), "{model}");
        }
        for model in [models::anthropic::CLAUDE_OPUS_5, models::anthropic::CLAUDE_SONNET_5] {
            assert!(!supports_thinking_display_updates(model, ""), "{model}");
        }

        for model in [models::anthropic::CLAUDE_SONNET_5_5, models::anthropic::CLAUDE_OPUS_5_5] {
            assert_eq!(default_thinking_display(model, ""), Some(ThinkingDisplay::Updates), "{model}");
        }
        for model in [models::anthropic::CLAUDE_OPUS_5, models::anthropic::CLAUDE_SONNET_5] {
            assert_eq!(default_thinking_display(model, ""), None, "{model}");
        }
    }

    #[test]
    fn claude_5_family_capabilities_cover_every_profiled_model() {
        for model in [
            models::anthropic::CLAUDE_SONNET_5_5,
            models::anthropic::CLAUDE_SONNET_5,
            models::anthropic::CLAUDE_HAIKU_5_5,
            models::anthropic::CLAUDE_FABLE_5,
            models::anthropic::CLAUDE_FABLE_5_1,
            models::anthropic::CLAUDE_OPUS_5,
            models::anthropic::CLAUDE_OPUS_5_5,
        ] {
            assert!(claude_thinking_profile(model, "").is_some(), "{model}");
            assert_eq!(effective_context_size(model), 1_000_000, "{model}");
            assert!(supports_compaction(model), "{model}");
            assert!(rejects_sampling(model, ""), "{model}");
            assert!(supports_structured_output(model, ""), "{model}");
        }
        assert!(rejects_sampling("", models::anthropic::CLAUDE_OPUS_5_5));

        for model in [CLAUDE_OPUS_4_8, "claude-sonnet-4-5", models::minimax::MINIMAX_M3, ""] {
            assert_eq!(effective_context_size(model), 200_000, "{model}");
            assert!(!supports_compaction(model), "{model}");
            assert!(!rejects_sampling(model, ""), "{model}");
        }
        assert!(supports_structured_output("claude-sonnet-4-5", ""));
        assert!(supports_structured_output("claude-opus-4-5", ""));
        assert!(!supports_structured_output(models::minimax::MINIMAX_M3, ""));
    }

    #[test]
    fn assistant_prefill_is_rejected_by_claude_4_6_and_later() {
        for model in [
            models::anthropic::CLAUDE_SONNET_5_5,
            models::anthropic::CLAUDE_SONNET_5,
            models::anthropic::CLAUDE_HAIKU_5_5,
            models::anthropic::CLAUDE_FABLE_5,
            models::anthropic::CLAUDE_FABLE_5_1,
            models::anthropic::CLAUDE_OPUS_5,
            models::anthropic::CLAUDE_OPUS_5_5,
            CLAUDE_OPUS_4_8,
            "claude-opus-4-7",
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-sonnet-4-6@20260101",
            "anthropic.claude-opus-4-6-v1:0",
            "claude-mythos-preview",
            "claude-unlisted-model",
        ] {
            assert!(!supports_assistant_prefill(model, ""), "{model}");
        }
        assert!(!supports_assistant_prefill("", models::anthropic::CLAUDE_OPUS_5_5));
    }

    #[test]
    fn assistant_prefill_is_kept_for_older_claude_and_non_claude_backends() {
        for model in [
            "claude-haiku-4-5",
            "claude-sonnet-4-5-20250929",
            "claude-opus-4-1",
            "claude-sonnet-4-20250514",
            "claude-3-7-sonnet-20250219",
            "claude-2.1",
            models::minimax::MINIMAX_M3,
            "glm-4.6",
        ] {
            assert!(supports_assistant_prefill(model, ""), "{model}");
        }
    }
}
