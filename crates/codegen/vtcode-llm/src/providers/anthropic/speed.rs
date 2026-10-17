//! Resolve requested speed independently of the saved preference and served speed.

use serde_json::Value;
use vtcode_config::AnthropicSpeed;

use crate::provider::{LLMError, LLMRequest};

pub(super) fn resolve_speed(
    request: &LLMRequest,
    configured: Option<AnthropicSpeed>,
    model: &str,
    native_endpoint: bool,
) -> Result<Option<AnthropicSpeed>, LLMError> {
    let explicit = request
        .anthropic_request_overrides
        .as_ref()
        .and_then(|overrides| overrides.speed);
    let eligible = native_endpoint && AnthropicSpeed::supports_model(model);
    if explicit == Some(AnthropicSpeed::Fast) && !eligible {
        return Err(LLMError::InvalidRequest {
            message: "Anthropic fast speed requires native Claude API with claude-opus-5-5, claude-opus-5, or claude-opus-4-8".to_owned(),
            metadata: None,
        });
    }
    Ok(eligible.then_some(explicit.or(configured)).flatten())
}

pub(super) fn apply_speed(body: &mut Value, speed: Option<AnthropicSpeed>) {
    if let Some(object) = body.as_object_mut() {
        object.remove("speed");
        if let Some(speed) = speed {
            object.insert("speed".to_owned(), Value::String(speed.as_str().to_owned()));
        }
    }
}

/// Only server-provided evidence is logged as served speed, at completion.
pub(super) fn log_served_speed(model: &str, speed: Option<AnthropicSpeed>) {
    tracing::info!(
        provider = "anthropic",
        model,
        served_speed = speed.map(AnthropicSpeed::as_str),
        "Anthropic response completed"
    );
}

pub(super) fn parse_served_speed(usage: &Value) -> Option<AnthropicSpeed> {
    match usage.get("speed").and_then(Value::as_str) {
        Some("fast") => Some(AnthropicSpeed::Fast),
        Some("standard") => Some(AnthropicSpeed::Standard),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_speed_served_evidence_is_independent_of_requested_speed() {
        for (usage, expected) in [
            (serde_json::json!({"speed": "fast"}), Some(AnthropicSpeed::Fast)),
            (serde_json::json!({"speed": "standard"}), Some(AnthropicSpeed::Standard)),
            (serde_json::json!({}), None),
            (serde_json::json!({"speed": "unknown"}), None),
        ] {
            let wire: crate::providers::anthropic_types::AnthropicUsage =
                serde_json::from_value(usage.clone()).unwrap();
            let parsed = serde_json::to_value(wire).unwrap();
            assert_eq!(parse_served_speed(&parsed), expected);
        }
    }
}
