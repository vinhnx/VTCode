pub(super) use super::{AnthropicProvider, capabilities, code_execution_beta_name, headers};
pub(super) use crate::provider::{
    ContentPart, LLMProvider, LLMRequest, LLMStreamEvent, Message, MessageContent, ToolDefinition,
};
pub(super) use futures::StreamExt;
pub(super) use serde_json::json;
pub(super) use vtcode_config::constants::models;

fn first_party_provider(model: &str) -> AnthropicProvider {
    AnthropicProvider::new_with_client(
        "test-key".to_string(),
        model.to_string(),
        reqwest::Client::new(),
        vtcode_config::constants::urls::ANTHROPIC_API_BASE.to_string(),
        vtcode_config::TimeoutsConfig::default(),
    )
}

fn split_betas(header: Option<String>) -> Vec<String> {
    header
        .map(|header| header.split(", ").map(str::to_string).collect())
        .unwrap_or_default()
}

fn body_has(request: &wiremock::Request, key: &str, value: Option<&str>) -> bool {
    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
    match value {
        Some(expected) => body.get(key).and_then(serde_json::Value::as_str) == Some(expected),
        None => body.get(key).is_none(),
    }
}

/// An Opus 5.5 provider whose thinking config (`display: "summarized"`)
/// is valid on the recommended Opus 4.8 as sent, so a refusal retry can
/// match the refused request exactly and redeem the credit token.
fn credit_retry_provider(server: &wiremock::MockServer) -> AnthropicProvider {
    let mut provider = AnthropicProvider::new_with_client(
        "test-key".to_string(),
        models::anthropic::CLAUDE_OPUS_5_5.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client should build"),
        format!("{}/v1", server.uri()),
        vtcode_config::TimeoutsConfig::default(),
    );
    provider.anthropic_config.thinking_display = Some(vtcode_config::ThinkingDisplayMode::Summarized);
    provider
}

fn refusal_with_recommendation() -> serde_json::Value {
    json!({
        "content": [],
        "stop_reason": "refusal",
        "stop_details": {
            "type": "refusal",
            "category": "cyber",
            "explanation": "declined",
            "fallback_credit_token": "credit-1",
            "recommended_model": "claude-opus-4-8"
        }
    })
}

mod speed;

mod betas_headers;
mod misc;
mod payload;
mod retry;
mod streaming;
