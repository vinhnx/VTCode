use super::*;
use crate::provider::{LLMProvider, Message};
use vtcode_config::constants::models;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn provider() -> OpenAIProvider {
    let mut provider = OpenAIProvider::with_model(String::new(), models::openai::GPT_6_ASTRA.to_string());
    provider.prompt_cache_enabled = true;
    provider.prompt_cache_settings.surface_metrics = true;
    provider
}

fn request(key: &str) -> LLMRequest {
    LLMRequest {
        model: models::openai::GPT_6_ASTRA.to_string(),
        prompt_cache_key: Some(key.to_string()),
        messages: vec![Message::user("Continue.".to_string())].into(),
        ..Default::default()
    }
}

fn completed(id: &str) -> Value {
    json!({ "id": id, "status": "completed", "model": "gpt-6-astra",
        "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "OK"}]}],
        "prompt_cache_diagnostics": {"type": "cache_miss", "reason": "tools_changed", "cache_missed_tokens": 1536} })
}

#[test]
fn comparison_is_key_scoped_bounded_and_independent_of_replay_lineage() {
    let provider = provider();
    let mut request = request("session-a");
    assert!(
        provider
            .convert_to_openai_responses_format(&request)
            .unwrap()
            .pointer("/prompt_cache_options/comparison_response_id")
            .is_none()
    );
    provider
        .cache_comparison_tracker(&request)
        .unwrap()
        .record_completion(&LLMResponse {
            request_id: Some("resp_baseline_a".to_string()),
            ..Default::default()
        });
    request.previous_response_id = None;
    let after = provider.convert_to_openai_responses_format(&request).unwrap();
    assert_eq!(after.pointer("/prompt_cache_options/comparison_response_id"), Some(&json!("resp_baseline_a")));
    assert_eq!(after.pointer("/prompt_cache_options/ttl"), Some(&json!("30m")));
    assert!(after.get("previous_response_id").is_none());
    request.prompt_cache_key = Some("session-b".to_string());
    assert!(
        provider
            .convert_to_openai_responses_format(&request)
            .unwrap()
            .pointer("/prompt_cache_options/comparison_response_id")
            .is_none()
    );
    request.prompt_cache_key = Some("session-a".to_string());
    for id in [
        "chatcmpl_123".to_string(),
        "resp_\nforged".to_string(),
        format!("resp_{}", "x".repeat(257)),
    ] {
        provider
            .cache_comparison_tracker(&request)
            .unwrap()
            .record_completion(&LLMResponse { request_id: Some(id), ..Default::default() });
    }
    assert_eq!(
        provider
            .convert_to_openai_responses_format(&request)
            .unwrap()
            .pointer("/prompt_cache_options/comparison_response_id"),
        Some(&json!("resp_baseline_a"))
    );
}

#[test]
fn comparison_requires_native_supported_model_and_enabled_metrics() {
    let mut provider = provider();
    let mut request = request("session-a");
    request.model = models::openai::GPT_5.to_string();
    assert!(provider.cache_comparison_tracker(&request).is_none());
    request.model = models::openai::GPT_6_ASTRA.to_string();
    provider.prompt_cache_settings.surface_metrics = false;
    assert!(provider.cache_comparison_tracker(&request).is_none());
    provider.prompt_cache_settings.surface_metrics = true;
    provider.prompt_cache_enabled = false;
    assert!(provider.cache_comparison_tracker(&request).is_none());
    provider.prompt_cache_enabled = true;
    provider.provider_key_override = Some(Arc::from("custom"));
    assert!(provider.cache_comparison_tracker(&request).is_none());
}

#[test]
// `PromptCachingConfig` has private fields, so struct-update syntax cannot
// replace the post-construction field assignments here (clippy
// field_reassign_with_default is the accepted trade-off, introduced in
// commit afe2c6367).
#[allow(
    clippy::field_reassign_with_default,
    reason = "private config fields block struct-update syntax; see commit afe2c6367"
)]
fn explicit_cache_opt_out_omits_native_hints_and_accounting_key() {
    for provider_disabled in [false, true] {
        let mut cfg = vtcode_config::core::PromptCachingConfig::default();
        cfg.enabled = provider_disabled;
        cfg.providers.openai.enabled = !provider_disabled;
        let provider = OpenAIProvider::from_config(
            Some(String::new()),
            None,
            Some(models::openai::GPT_6_ASTRA.to_string()),
            None,
            Some(cfg),
            None,
            None,
            None,
            None,
        );
        let mut request = request("session-off");
        request.system_prompt = Some(Arc::from("Stable instructions"));
        let payload = provider.convert_to_openai_responses_format(&request).unwrap();
        assert!(payload.get("prompt_cache_options").is_none());
        assert!(payload.get("prompt_cache_retention").is_none());
        assert!(payload.get("prompt_cache_key").is_none());
        assert!(payload.get("instructions").is_some());
        assert!(payload.pointer("/input/0/content/0/prompt_cache_breakpoint").is_none());
    }
}

#[tokio::test]
async fn foreground_http_completions_advance_comparison_auxiliary_and_errors_do_not() {
    let server = MockServer::start().await;
    let mut provider = provider();
    provider.responses_url = Arc::from(format!("{}/responses", server.uri()));
    let request = request("session-a");
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(completed("resp_foreground")))
        .mount(&server)
        .await;
    provider.generate(request.clone()).await.unwrap();
    server.reset().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(completed("resp_auxiliary")))
        .mount(&server)
        .await;
    provider.generate_auxiliary(request.clone()).await.unwrap();
    let sent = server.received_requests().await.unwrap();
    assert_eq!(
        sent[0]
            .body_json::<Value>()
            .unwrap()
            .pointer("/prompt_cache_options/comparison_response_id"),
        Some(&json!("resp_foreground"))
    );
    server.reset().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"error": {"message": "Unavailable"}})))
        .mount(&server)
        .await;
    assert!(provider.generate(request.clone()).await.is_err());
    assert_eq!(
        provider
            .convert_to_openai_responses_format(&request)
            .unwrap()
            .pointer("/prompt_cache_options/comparison_response_id"),
        Some(&json!("resp_foreground"))
    );
}

#[tokio::test]
async fn both_http_stream_surfaces_advance_only_on_delivered_completion() {
    use futures::StreamExt;
    let server = MockServer::start().await;
    let mut provider = provider();
    provider.responses_url = Arc::from(format!("{}/responses", server.uri()));
    let request = request("session-stream");
    let body = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"type": "response.completed", "response": completed("resp_stream")})
    );
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .mount(&server)
        .await;
    drop(provider.stream(request.clone()).await.unwrap());
    assert!(
        provider
            .convert_to_openai_responses_format(&request)
            .unwrap()
            .pointer("/prompt_cache_options/comparison_response_id")
            .is_none()
    );
    let mut legacy = provider.stream(request.clone()).await.unwrap();
    while let Some(event) = legacy.next().await {
        event.unwrap();
    }
    assert_eq!(
        provider
            .convert_to_openai_responses_format(&request)
            .unwrap()
            .pointer("/prompt_cache_options/comparison_response_id"),
        Some(&json!("resp_stream"))
    );
    server.reset().await;
    let body = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"type": "response.completed", "response": completed("resp_normalized")})
    );
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .mount(&server)
        .await;
    let mut normalized = provider.stream_normalized(request.clone()).await.unwrap();
    while let Some(event) = normalized.next().await {
        event.unwrap();
    }
    assert_eq!(
        provider
            .convert_to_openai_responses_format(&request)
            .unwrap()
            .pointer("/prompt_cache_options/comparison_response_id"),
        Some(&json!("resp_normalized"))
    );
}
