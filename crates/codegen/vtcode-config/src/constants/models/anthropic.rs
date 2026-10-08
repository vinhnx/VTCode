// Claude 5.x series - Latest Anthropic models
pub const DEFAULT_MODEL: &str = "claude-sonnet-5";
pub const SUPPORTED_MODELS: &[&str] = &[
    "claude-sonnet-5-5", // Latest Sonnet: best speed/intelligence balance, 1M context, between_tools is the lowest thinking setting
    "claude-sonnet-5",   // Previous Sonnet generation with adaptive thinking on by default
    "claude-haiku-5-5", // High-volume latency-sensitive work: classification, routing, extraction, subagents; adaptive thinking, 1M context, 128K output
    "claude-fable-5",   // Most capable widely released model
    "claude-fable-5-1", // Successor to Fable 5, 1M context, cache reads at 1/4 cost
    "claude-opus-5",    // Opus-tier premium flagship with adaptive thinking, 1M context
    "claude-opus-5-5",  // Opus-tier successor with adaptive thinking always on, 1M context
];

// Convenience constants for alias models
pub const CLAUDE_OPUS_5: &str = "claude-opus-5";
pub const CLAUDE_OPUS_5_5: &str = "claude-opus-5-5";
pub const CLAUDE_SONNET_5: &str = "claude-sonnet-5";
pub const CLAUDE_SONNET_5_5: &str = "claude-sonnet-5-5";
pub const CLAUDE_HAIKU_5_5: &str = "claude-haiku-5-5";
pub const CLAUDE_FABLE_5: &str = "claude-fable-5";
pub const CLAUDE_FABLE_5_1: &str = "claude-fable-5-1";

/// Models that accept the reasoning effort parameter or extended thinking
pub const REASONING_MODELS: &[&str] = &[
    CLAUDE_SONNET_5_5,
    CLAUDE_SONNET_5,
    CLAUDE_HAIKU_5_5,
    CLAUDE_FABLE_5,
    CLAUDE_FABLE_5_1,
    CLAUDE_OPUS_5,
    CLAUDE_OPUS_5_5,
];

/// Minimum advisor model capability: the advisor must be at least Claude Sonnet 5.
const ADVISOR_MIN_MODEL: &str = CLAUDE_SONNET_5;

/// Returns the base model id with any `-YYYYMMDD` version suffix stripped.
pub fn normalize_model_id(model: &str) -> &str {
    if let Some(idx) = model.rfind('-') {
        let suffix = &model[idx + 1..];
        if suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
            return &model[..idx];
        }
    }
    model
}

/// Relative capability tier for advisor compatibility checks.
///
/// Higher is more capable. Used to enforce that the advisor model is at least as
/// capable as the executor model. Self-advising models (Fable 5) are
/// handled separately because they may only advise themselves.
///
/// Claude Sonnet 5.5 sits just above Sonnet 5: a Sonnet 5.5 executor rejects a
/// Sonnet 5 advisor (400), so its tier must stay strictly above Sonnet 5's.
fn advisor_tier(model: &str) -> Option<u8> {
    match normalize_model_id(model) {
        CLAUDE_HAIKU_5_5 => Some(2),
        CLAUDE_SONNET_5 => Some(3),
        CLAUDE_SONNET_5_5 => Some(4),
        CLAUDE_OPUS_5 => Some(6),
        CLAUDE_OPUS_5_5 => Some(7),
        CLAUDE_FABLE_5 => Some(8),
        CLAUDE_FABLE_5_1 => Some(8),
        _ => None,
    }
}

/// Validates that an executor/advisor model pair is permitted by the Anthropic
/// advisor compatibility table.
///
/// Returns `Ok(())` when the pair is valid, or `Err(message)` describing the
/// unsupported combination (matching the API's `400 invalid_request_error`).
pub fn validate_advisor_pair(executor: &str, advisor: &str) -> Result<(), String> {
    let executor_base = normalize_model_id(executor);
    let advisor_base = normalize_model_id(advisor);

    // Self-advising models can only advise themselves.
    if advisor_base == CLAUDE_FABLE_5 && executor_base != CLAUDE_FABLE_5 {
        return Err(format!("advisor model {advisor} may only advise {CLAUDE_FABLE_5}"));
    }
    if advisor_base == CLAUDE_FABLE_5_1 && executor_base != CLAUDE_FABLE_5_1 {
        return Err(format!("advisor model {advisor} may only advise {CLAUDE_FABLE_5_1}"));
    }

    let Some(executor_tier) = advisor_tier(executor_base) else {
        return Err(format!("executor model {executor} is not a supported advisor executor"));
    };
    let Some(chosen_advisor_tier) = advisor_tier(advisor_base) else {
        return Err(format!("advisor model {advisor} is not a supported advisor model"));
    };

    if chosen_advisor_tier < advisor_tier(ADVISOR_MIN_MODEL).unwrap_or(3) {
        return Err(format!(
            "advisor model {advisor} is less capable than the minimum allowed advisor ({ADVISOR_MIN_MODEL})"
        ));
    }

    if chosen_advisor_tier < executor_tier {
        return Err(format!("advisor model {advisor} must be at least as capable as the executor model {executor}"));
    }

    Ok(())
}

/// Returns a reasonable default advisor model for a given executor model.
///
/// Falls back to `claude-opus-5` (the most broadly compatible advisor) when the
/// executor is unknown or unversioned.
pub fn default_advisor_model(executor: &str) -> &'static str {
    match normalize_model_id(executor) {
        CLAUDE_HAIKU_5_5 => CLAUDE_OPUS_5,
        CLAUDE_SONNET_5 => CLAUDE_OPUS_5,
        CLAUDE_SONNET_5_5 => CLAUDE_OPUS_5,
        CLAUDE_OPUS_5 => CLAUDE_OPUS_5,
        CLAUDE_OPUS_5_5 => CLAUDE_OPUS_5_5,
        CLAUDE_FABLE_5 => CLAUDE_FABLE_5,
        CLAUDE_FABLE_5_1 => CLAUDE_FABLE_5_1,
        _ => CLAUDE_OPUS_5,
    }
}
