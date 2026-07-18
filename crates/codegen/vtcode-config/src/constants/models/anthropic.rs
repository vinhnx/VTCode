// Claude 4.x/5.x series - Latest Anthropic models
pub const DEFAULT_MODEL: &str = "claude-sonnet-5";
pub const SUPPORTED_MODELS: &[&str] = &[
    "claude-sonnet-5",           // Latest balanced flagship with adaptive thinking on by default
    "claude-fable-5",            // Most capable widely released model
    "claude-mythos-5",           // Fable 5-class model without safety classifiers (limited)
    "claude-opus-4-8",           // Opus-tier premium flagship with adaptive thinking
    "claude-sonnet-4-6",         // Previous balanced flagship
    "claude-haiku-4-5",          // Fastest model with near-frontier intelligence
    "claude-haiku-4-5-20251001", // Haiku 4.5 versioned
];

// Convenience constants for versioned models
pub const CLAUDE_HAIKU_4_5_20251001: &str = "claude-haiku-4-5-20251001";

// Convenience constants for alias models
pub const CLAUDE_HAIKU_4_5: &str = "claude-haiku-4-5";
pub const CLAUDE_SONNET_4_6: &str = "claude-sonnet-4-6";
pub const CLAUDE_OPUS_4_8: &str = "claude-opus-4-8";
pub const CLAUDE_SONNET_5: &str = "claude-sonnet-5";
pub const CLAUDE_FABLE_5: &str = "claude-fable-5";
pub const CLAUDE_MYTHOS_5: &str = "claude-mythos-5";

/// Models that accept the reasoning effort parameter or extended thinking
pub const REASONING_MODELS: &[&str] = &[
    CLAUDE_SONNET_5,
    CLAUDE_FABLE_5,
    CLAUDE_MYTHOS_5,
    CLAUDE_SONNET_4_6,
    CLAUDE_OPUS_4_8,
    CLAUDE_HAIKU_4_5,
    CLAUDE_HAIKU_4_5_20251001,
];

/// Minimum advisor model capability: the advisor must be at least Claude Sonnet 4.6.
pub const ADVISOR_MIN_MODEL: &str = CLAUDE_SONNET_4_6;

/// Returns the base model id with any `-YYYYMMDD` version suffix stripped.
pub fn normalize_model_id(model: &str) -> &str {
    // Split off a trailing `-YYYYMMDD` version pin.
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
/// capable as the executor model. Self-advising models (Fable 5, Mythos 5) are
/// handled separately because they may only advise themselves.
fn advisor_tier(model: &str) -> Option<u8> {
    match normalize_model_id(model) {
        CLAUDE_HAIKU_4_5 => Some(1),
        CLAUDE_SONNET_4_6 => Some(2),
        CLAUDE_SONNET_5 => Some(3),
        "claude-opus-4-6" => Some(4),
        "claude-opus-4-7" => Some(5),
        CLAUDE_OPUS_4_8 => Some(6),
        CLAUDE_FABLE_5 => Some(7),
        CLAUDE_MYTHOS_5 => Some(8),
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
    if advisor_base == CLAUDE_MYTHOS_5 && executor_base != CLAUDE_MYTHOS_5 {
        return Err(format!("advisor model {advisor} may only advise {CLAUDE_MYTHOS_5}"));
    }

    let Some(executor_tier) = advisor_tier(executor_base) else {
        return Err(format!("executor model {executor} is not a supported advisor executor"));
    };
    let Some(chosen_advisor_tier) = advisor_tier(advisor_base) else {
        return Err(format!("advisor model {advisor} is not a supported advisor model"));
    };

    if chosen_advisor_tier < advisor_tier(ADVISOR_MIN_MODEL).unwrap_or(2) {
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
/// Falls back to `claude-opus-4-8` (the most broadly compatible advisor) when the
/// executor is unknown or unversioned.
pub fn default_advisor_model(executor: &str) -> &'static str {
    match normalize_model_id(executor) {
        CLAUDE_HAIKU_4_5 => CLAUDE_OPUS_4_8,
        CLAUDE_SONNET_4_6 => CLAUDE_OPUS_4_8,
        CLAUDE_SONNET_5 => CLAUDE_OPUS_4_8,
        "claude-opus-4-6" => CLAUDE_OPUS_4_8,
        "claude-opus-4-7" => CLAUDE_OPUS_4_8,
        CLAUDE_OPUS_4_8 => CLAUDE_OPUS_4_8,
        CLAUDE_FABLE_5 => CLAUDE_FABLE_5,
        CLAUDE_MYTHOS_5 => CLAUDE_MYTHOS_5,
        _ => CLAUDE_OPUS_4_8,
    }
}
