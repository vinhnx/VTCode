use super::*;

#[tokio::test]
async fn openai_stale_previous_response_retry_is_disabled() {
    use vtcode_core::utils::transcript;

    transcript::clear();

    let recorded_previous_response_ids = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        "openai",
        false,
        Arc::clone(&recorded_previous_response_ids),
        vec![
            ScriptedProviderOutcome::Error(uni::LLMError::Provider {
                message: "OpenAI error: previous_response_not_found: previous response missing".to_string(),
                metadata: None,
            }),
            ScriptedProviderOutcome::Success {
                content: Some("unexpected retry"),
                request_id: Some("resp_456"),
            },
        ],
    );

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let prior_messages = vec![uni::Message::user("hello".to_string())];
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    ctx.working_history.extend(prior_messages.clone());
    ctx.working_history.push(uni::Message::user("continue".to_string()));
    ctx.session_stats
        .set_previous_response_chain("openai", "noop-model", Some("resp_123"), &prior_messages);

    let result = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None).await;

    assert!(result.is_err(), "stale OpenAI chain should not be retried");
    assert_eq!(
        recorded_previous_response_ids
            .lock()
            .expect("recorded previous_response_ids")
            .as_slice(),
        &[None]
    );
    assert_eq!(
        transcript::snapshot()
            .join("\n")
            .matches("Previous response chain expired; retrying with a fresh provider chain.")
            .count(),
        0
    );

    transcript::clear();
}
#[tokio::test]
async fn llm_misconfiguration_does_not_retry_network_category() {
    let recorded_previous_response_ids = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        "mycorp",
        false,
        Arc::clone(&recorded_previous_response_ids),
        vec![
            ScriptedProviderOutcome::Error(uni::LLMError::Network {
                message: "network error: invalid endpoint in provider_overrides.mycorp.base_url".to_string(),
                metadata: None,
            }),
            ScriptedProviderOutcome::Success {
                content: Some("unexpected retry"),
                request_id: None,
            },
        ],
    );

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    ctx.working_history.push(uni::Message::user("continue".to_string()));

    let result = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None).await;

    assert!(result.is_err(), "an invalid endpoint must fail before a provider retry");
}
#[tokio::test]
async fn llm_metadata_misconfiguration_does_not_retry_generic_provider_failure() {
    let recorded_previous_response_ids = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        "mycorp",
        false,
        Arc::clone(&recorded_previous_response_ids),
        vec![
            ScriptedProviderOutcome::Error(uni::LLMError::Provider {
                message: "HTTP 503 Service Unavailable".to_string(),
                metadata: Some(uni::LLMErrorMetadata::new(
                    "mycorp",
                    Some(404),
                    Some("model_not_found".to_string()),
                    None,
                    None,
                    None,
                    Some("The requested model does not exist".to_string()),
                )),
            }),
            ScriptedProviderOutcome::Success {
                content: Some("unexpected retry"),
                request_id: None,
            },
        ],
    );

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    ctx.working_history.push(uni::Message::user("continue".to_string()));

    let result = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None).await;

    assert!(result.is_err(), "provider metadata must block a retryable HTTP classification");
    assert_eq!(recorded_previous_response_ids.lock().expect("provider calls").len(), 1);
}
#[test]
fn retryable_llm_error_excludes_forbidden_quota_failures() {
    assert!(!is_retryable_llm_error(
        "Provider error: HuggingFace API error (403 Forbidden): {\"error\":\"You have exceeded your monthly spending limit.\"}"
    ));
}
#[test]
fn retryable_llm_error_includes_rate_limit_429() {
    assert!(is_retryable_llm_error("Provider error: 429 Too Many Requests"));
}
#[test]
fn retryable_llm_error_includes_service_unavailable_class() {
    assert!(is_retryable_llm_error("Provider error: 503 Service Unavailable"));
    assert!(is_retryable_llm_error("Provider error: 504 Gateway Timeout"));
}
#[test]
fn previous_response_chain_error_detects_provider_code() {
    assert!(is_previous_response_chain_error(
        "OpenAI error: previous_response_not_found: previous response missing"
    ));
}
#[test]
fn previous_response_chain_error_detects_human_readable_message() {
    assert!(is_previous_response_chain_error("Previous response with id 'resp_cached' not found."));
}
#[test]
fn previous_response_chain_error_ignores_service_unavailable() {
    assert!(!is_previous_response_chain_error("Provider error: 503 Service Unavailable"));
}
#[test]
fn unmatched_tool_result_error_requires_the_specific_bad_request_shape() {
    assert!(is_unmatched_tool_result_error(
        "Merge Gateway error (400 Bad Request): unmatched tool result for call_123"
    ));
    assert!(is_unmatched_tool_result_error("HTTP 400: tool_result is not associated with an earlier tool call"));
    assert!(!is_unmatched_tool_result_error("Merge Gateway error (400 Bad Request): invalid model"));
    assert!(!is_unmatched_tool_result_error("Merge Gateway error (500): unmatched tool result"));
}
