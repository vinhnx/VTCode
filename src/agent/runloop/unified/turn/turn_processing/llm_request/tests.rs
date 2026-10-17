use super::*;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use vtcode_core::config::map_prompt_cache_key_for_provider;

struct ScriptedProvider {
    provider_name: &'static str,
    supports_responses_compaction: bool,
    supports_streaming: bool,
    recorded_previous_response_ids: Arc<Mutex<Vec<Option<String>>>>,
    outcomes: Mutex<VecDeque<ScriptedProviderOutcome>>,
    resume_wake: Option<ResumeWake>,
}

struct ResumeWake {
    notify: Arc<tokio::sync::Notify>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

enum ScriptedProviderOutcome {
    Success {
        content: Option<&'static str>,
        request_id: Option<&'static str>,
    },
    Error(uni::LLMError),
}

impl ScriptedProvider {
    fn new(
        provider_name: &'static str,
        supports_responses_compaction: bool,
        recorded_previous_response_ids: Arc<Mutex<Vec<Option<String>>>>,
        outcomes: Vec<ScriptedProviderOutcome>,
    ) -> Self {
        Self {
            provider_name,
            supports_responses_compaction,
            supports_streaming: false,
            recorded_previous_response_ids,
            outcomes: Mutex::new(VecDeque::from(outcomes)),
            resume_wake: None,
        }
    }

    fn with_streaming(mut self, supports_streaming: bool) -> Self {
        self.supports_streaming = supports_streaming;
        self
    }
}

#[async_trait::async_trait]
impl uni::LLMProvider for ScriptedProvider {
    fn name(&self) -> &str {
        self.provider_name
    }

    fn supports_streaming(&self) -> bool {
        self.supports_streaming
    }

    fn supports_responses_compaction(&self, _model: &str) -> bool {
        self.supports_responses_compaction
    }

    async fn generate(&self, request: uni::LLMRequest) -> std::result::Result<uni::LLMResponse, uni::LLMError> {
        self.recorded_previous_response_ids
            .lock()
            .expect("previous_response_id recorder")
            .push(request.previous_response_id.clone());

        if let Some(wake) = &self.resume_wake {
            wake.entered.notify_one();
            wake.notify.notify_one();
            wake.release.notified().await;
        }

        match self
            .outcomes
            .lock()
            .expect("provider script")
            .pop_front()
            .expect("provider script should have enough outcomes")
        {
            ScriptedProviderOutcome::Success { content, request_id } => Ok(uni::LLMResponse {
                content: content.map(str::to_string),
                model: "noop-model".to_string(),
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: request_id.map(str::to_string),
                tool_references: Vec::new(),
                compaction: None,
            }),
            ScriptedProviderOutcome::Error(error) => Err(error),
        }
    }

    async fn stream(&self, _request: uni::LLMRequest) -> std::result::Result<uni::LLMStream, uni::LLMError> {
        Err(uni::LLMError::Provider {
            message: "LLM first token timed out after 1 seconds".to_string(),
            metadata: None,
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["noop-model".to_string()]
    }

    fn validate_request(&self, _request: &uni::LLMRequest) -> std::result::Result<(), uni::LLMError> {
        Ok(())
    }
}

#[test]
fn retryable_llm_error_includes_internal_server_error_message() {
    assert!(is_retryable_llm_error("Provider error: Internal Server Error"));
}

#[test]
fn retryable_llm_error_excludes_non_transient_messages() {
    assert!(!is_retryable_llm_error("Provider error: Invalid API key"));
}

#[tokio::test]
async fn retry_progress_preserves_operation_and_clears_after_completion() {
    use vtcode_commons::ui_protocol::{ProgressPhase, ProgressUpdate};
    use vtcode_core::ui::{InlineCommand, InlineHandle};

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let guard = handle.begin_progress(ProgressPhase::PreparingContext);
    let operation = guard.operation();
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.handle = &handle;
    *ctx.provider_client = Box::new(ScriptedProvider::new(
        "mycorp",
        false,
        Arc::new(Mutex::new(Vec::new())),
        vec![
            ScriptedProviderOutcome::Error(uni::LLMError::Provider {
                message: "503 Service Unavailable".into(),
                metadata: None,
            }),
            ScriptedProviderOutcome::Success { content: Some("recovered"), request_id: None },
        ],
    ));
    ctx.working_history.push(uni::Message::user("hello".into()));
    assert!(
        execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None)
            .await
            .is_ok()
    );
    drop(guard);

    let mut phases = Vec::new();
    let mut finished = false;
    while let Ok(command) = receiver.try_recv() {
        match command {
            InlineCommand::UpdateProgress(ProgressUpdate::Phase { operation: current, phase }) => {
                assert_eq!(current, operation);
                phases.push(phase);
            }
            InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation: current }) => {
                assert_eq!(current, operation);
                finished = true;
            }
            _ => {}
        }
    }
    assert_eq!(phases.iter().filter(|phase| **phase == ProgressPhase::WaitingForModel).count(), 2);
    let retry = phases.iter().position(|phase| *phase == ProgressPhase::Retrying).unwrap();
    assert_eq!(phases[retry + 1], ProgressPhase::WaitingForModel);
    assert_eq!(phases.last(), Some(&ProgressPhase::ReceivingResponse));
    assert!(finished);
    assert!(handle.current_progress_operation().is_none());
}

#[tokio::test]
async fn compatible_previous_response_not_found_without_sent_chain_is_not_recovered() {
    use vtcode_core::utils::transcript;

    transcript::clear();

    let recorded_previous_response_ids = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        "mycorp",
        true,
        Arc::clone(&recorded_previous_response_ids),
        vec![ScriptedProviderOutcome::Error(uni::LLMError::Provider {
            message: "Provider error: previous_response_not_found: previous response missing".to_string(),
            metadata: None,
        })],
    );

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let prior_messages = vec![uni::Message::user("hello".to_string())];
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    ctx.working_history.extend(prior_messages.clone());
    ctx.working_history.push(uni::Message::user("continue".to_string()));
    ctx.session_stats
        .set_previous_response_chain("mycorp", "noop-model", Some("resp_123"), &prior_messages);

    let result = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None).await;

    assert!(
        result.is_err(),
        "compatible provider should not recover a stale chain when no previous_response_id was sent"
    );
    assert_eq!(
        recorded_previous_response_ids
            .lock()
            .expect("recorded previous_response_ids")
            .as_slice(),
        &[None]
    );

    let transcript_text = transcript::snapshot().join("\n");

    assert_eq!(
        transcript_text
            .matches("Previous response chain expired; retrying with a fresh provider chain.")
            .count(),
        0
    );

    transcript::clear();
}

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

#[tokio::test]
async fn unmatched_tool_result_noop_repair_fails_closed_without_repeating_the_request() {
    let recorded_previous_response_ids = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        "merge_gateway",
        false,
        Arc::clone(&recorded_previous_response_ids),
        vec![ScriptedProviderOutcome::Error(uni::LLMError::Provider {
            message: "Merge Gateway error (400 Bad Request): unmatched tool result for call_1".to_string(),
            metadata: None,
        })],
    );

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    ctx.working_history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            "call_1".to_string(),
            "read_file".to_string(),
            "{}".to_string(),
        )],
    ));
    ctx.working_history
        .push(uni::Message::tool_response("call_1".to_string(), "result".to_string()));

    let result = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None).await;

    assert!(result.is_err());
    assert_eq!(recorded_previous_response_ids.lock().expect("provider calls").len(), 1);
}

#[tokio::test]
async fn misconfiguration_after_tool_result_skips_post_tool_retry() {
    let recorded_previous_response_ids = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        "any-provider",
        false,
        Arc::clone(&recorded_previous_response_ids),
        vec![ScriptedProviderOutcome::Error(uni::LLMError::Network {
            message: "network error: invalid endpoint in base_url".to_string(),
            metadata: None,
        })],
    );

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    ctx.working_history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            "call_1".to_string(),
            "read_file".to_string(),
            "{}".to_string(),
        )],
    ));
    ctx.working_history
        .push(uni::Message::tool_response("call_1".to_string(), "result".to_string()));

    let result = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None).await;

    assert!(result.is_err());
    assert_eq!(recorded_previous_response_ids.lock().expect("provider calls").len(), 1);
}

#[test]
fn retryable_llm_error_excludes_usage_limit_messages() {
    assert!(!is_retryable_llm_error("Provider error: you have reached your weekly usage limit"));
}

#[tokio::test]
async fn streaming_timeout_falls_back_to_non_streaming_for_supported_models() {
    use vtcode_core::utils::transcript;

    transcript::clear();

    let recorded_previous_response_ids = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        "any-provider",
        false,
        Arc::clone(&recorded_previous_response_ids),
        vec![ScriptedProviderOutcome::Success { content: Some("recovered"), request_id: None }],
    )
    .with_streaming(true);

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    ctx.working_history.push(uni::Message::user("continue".to_string()));

    let result = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None).await;

    assert!(result.is_ok());
    assert_eq!(result.expect("non-streaming recovery").0.content.as_deref(), Some("recovered"));

    transcript::clear();
}

#[test]
fn post_tool_retry_uses_non_streaming_before_compaction_when_supported() {
    assert_eq!(
        next_post_tool_retry_action(true, true, false, false),
        Some(PostToolRetryAction::SwitchToNonStreaming)
    );
}

#[test]
fn post_tool_retry_skips_non_streaming_when_unsupported() {
    assert_eq!(
        next_post_tool_retry_action(true, false, false, false),
        Some(PostToolRetryAction::CompactToolContext)
    );
}

#[test]
fn post_tool_retry_preserves_structured_context_for_responses_chaining_providers() {
    assert_eq!(next_post_tool_retry_action(false, true, false, true), None);
}

#[test]
fn compact_tool_messages_for_retry_keeps_recent_tool_outputs_only() {
    let messages = vec![
        uni::Message::user("u1".to_string()),
        uni::Message::tool_response("call_1".to_string(), "old tool".to_string()),
        uni::Message::assistant("a1".to_string()),
        uni::Message::tool_response("call_2".to_string(), "new tool".to_string()),
    ];

    let compacted = compact_tool_messages_for_retry(&messages);
    assert_eq!(
        compacted
            .iter()
            .filter(|message| message.role == uni::MessageRole::Tool)
            .count(),
        2
    );
    assert_eq!(compacted.len(), 4);
}

#[test]
fn compact_tool_messages_for_retry_keeps_all_tool_call_ids() {
    let messages = vec![
        uni::Message::tool_response("call_1".to_string(), "first".to_string()),
        uni::Message::assistant("a1".to_string()),
        uni::Message::tool_response("call_2".to_string(), "second".to_string()),
        uni::Message::assistant("a2".to_string()),
        uni::Message::tool_response("call_3".to_string(), "third".to_string()),
    ];

    let compacted = compact_tool_messages_for_retry(&messages);
    let tool_ids = compacted
        .iter()
        .filter(|message| message.role == uni::MessageRole::Tool)
        .filter_map(|message| message.tool_call_id.clone())
        .collect::<Vec<_>>();

    assert_eq!(tool_ids, vec!["call_1", "call_2", "call_3"]);
}

#[test]
fn llm_retry_attempts_uses_default_when_unset() {
    assert_eq!(llm_retry_attempts(None), DEFAULT_LLM_RETRY_ATTEMPTS);
}

#[test]
fn llm_retry_attempts_uses_configured_retries_plus_initial_attempt() {
    assert_eq!(llm_retry_attempts(Some(2)), 3);
}

#[test]
fn llm_retry_attempts_respects_upper_bound() {
    assert_eq!(llm_retry_attempts(Some(16)), MAX_LLM_RETRY_ATTEMPTS);
}

#[test]
fn stream_timeout_error_detection_matches_common_messages() {
    assert!(is_stream_timeout_error("Stream request timed out after 75s"));
    assert!(is_stream_timeout_error("Streaming request timed out after configured timeout"));
    assert!(is_stream_timeout_error("LLM first token timed out after 120 seconds"));
}

#[test]
fn llm_first_progress_timeout_defaults_to_fifth_of_turn_budget() {
    assert_eq!(llm_first_progress_timeout_secs(300, false, true, "openai"), 60);
    assert_eq!(llm_first_progress_timeout_secs(300, false, false, "openai"), 60);
}

#[test]
fn llm_first_progress_timeout_merge_gateway_floors_to_gateway_silence() {
    // Non-streaming merge-gateway turns must not abandon before Gateway's
    // 120s first-frame silence window (double-bill risk).
    assert_eq!(llm_first_progress_timeout_secs(300, false, true, "merge-gateway"), 120);
    // Without non-streaming fallback there is no double-bill retry path.
    assert_eq!(llm_first_progress_timeout_secs(300, false, false, "merge-gateway"), 60);
    // Other providers keep the baseline.
    assert_eq!(llm_first_progress_timeout_secs(300, false, true, "anthropic"), 60);
}

#[test]
fn llm_first_progress_timeout_planning_workflow_uses_capability_floor() {
    assert_eq!(llm_first_progress_timeout_secs(150, true, true, "merge-gateway"), 120);
}

#[test]
fn llm_first_progress_timeout_planning_openai_uses_capability_floor() {
    assert_eq!(llm_first_progress_timeout_secs(150, true, true, "openai"), 90);
    assert_eq!(llm_first_progress_timeout_secs(150, true, false, "openai"), 75);
}

#[test]
fn llm_first_progress_timeout_expands_for_planning_workflow() {
    assert_eq!(llm_first_progress_timeout_secs(300, true, true, "openai"), 150);
}

#[test]
fn llm_first_progress_timeout_planning_workflow_respects_smaller_turn_budget() {
    assert_eq!(llm_first_progress_timeout_secs(180, true, true, "openai"), 90);
    // merge-gateway planning floors at documented 120s even on small budgets.
    assert_eq!(llm_first_progress_timeout_secs(180, true, true, "merge-gateway"), 120);
}

#[test]
fn llm_first_progress_timeout_planning_workflow_cap_remains_180() {
    assert_eq!(llm_first_progress_timeout_secs(1_200, true, true, "merge-gateway"), 180);
}

#[test]
fn llm_first_progress_timeout_planning_workflow_uses_capability_floor_for_openai() {
    assert_eq!(llm_first_progress_timeout_secs(150, true, true, "openai"), 90);
    assert_eq!(llm_first_progress_timeout_secs(150, true, false, "openai"), 75);
}

#[test]
fn llm_first_progress_timeout_respects_planning_workflow_cap() {
    assert_eq!(llm_first_progress_timeout_secs(1_200, true, true, "openai"), 180);
}

#[test]
fn openai_prompt_cache_enablement_requires_provider_and_flags() {
    assert!(is_openai_prompt_cache_enabled("openai", true, true));
    assert!(is_openai_prompt_cache_enabled("merge-gateway", true, true));
    assert!(!is_openai_prompt_cache_enabled("openai", false, true));
    assert!(!is_openai_prompt_cache_enabled("openai", true, false));
    assert!(!is_openai_prompt_cache_enabled("merge-gateway", false, true));
    assert!(!is_openai_prompt_cache_enabled("anthropic", true, true));
    // openrouter/xai use session affinity keys independently of the OpenAI cache flag.
    assert!(!is_openai_prompt_cache_enabled("openrouter", true, true));
    assert!(is_session_affinity_key_enabled("openrouter", true, false));
    assert!(is_session_affinity_key_enabled("xai", true, false));
    assert!(is_session_affinity_key_enabled("openai", true, true));
    assert!(!is_session_affinity_key_enabled("openai", true, false));
    assert!(!is_session_affinity_key_enabled("openrouter", false, true));
    assert!(!is_session_affinity_key_enabled("anthropic", true, true));
}

#[test]
fn prompt_cache_shaping_mode_requires_global_opt_in_and_provider_cache() {
    let mut cfg = PromptCachingConfig {
        enabled: true,
        cache_friendly_prompt_shaping: true,
        ..PromptCachingConfig::default()
    };
    cfg.providers.openai.enabled = true;

    assert_eq!(resolve_prompt_cache_shaping_mode("openai", &cfg), PromptCacheShapingMode::TrailingRuntimeContext);

    cfg.cache_friendly_prompt_shaping = false;
    assert_eq!(resolve_prompt_cache_shaping_mode("openai", &cfg), PromptCacheShapingMode::Disabled);
}

#[test]
fn prompt_cache_shaping_mode_uses_block_mode_for_anthropic_family() {
    let mut cfg = PromptCachingConfig {
        enabled: true,
        cache_friendly_prompt_shaping: true,
        ..PromptCachingConfig::default()
    };
    cfg.providers.anthropic.enabled = true;

    assert_eq!(
        resolve_prompt_cache_shaping_mode("anthropic", &cfg),
        PromptCacheShapingMode::AnthropicBlockRuntimeContext
    );
    assert_eq!(
        resolve_prompt_cache_shaping_mode("minimax", &cfg),
        PromptCacheShapingMode::AnthropicBlockRuntimeContext
    );
}

#[test]
fn prompt_cache_shaping_mode_respects_gemini_mode_off() {
    let mut cfg = PromptCachingConfig {
        enabled: true,
        cache_friendly_prompt_shaping: true,
        ..PromptCachingConfig::default()
    };
    cfg.providers.gemini.enabled = true;
    cfg.providers.gemini.mode = vtcode_core::config::core::GeminiPromptCacheMode::Off;

    assert_eq!(resolve_prompt_cache_shaping_mode("gemini", &cfg), PromptCacheShapingMode::Disabled);
}

#[test]
fn openai_prompt_cache_key_uses_stable_session_identifier() {
    let lineage_id = "lineage-abc-123";
    let first = build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, Some(lineage_id));
    let second = build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, Some(lineage_id));

    assert_eq!(first, Some("vtcode:openai:lineage-abc-123".to_string()));
    assert_eq!(first, second);
}

#[test]
fn session_affinity_prompt_cache_key_is_populated_for_openrouter_and_xai() {
    use vtcode_core::config::build_session_affinity_prompt_cache_key;

    // Production gate: OpenRouter/xAI get lineage when global prompt-cache is on.
    assert!(is_session_affinity_key_enabled("openrouter", true, false));
    assert!(is_session_affinity_key_enabled("xai", true, false));

    let key = build_session_affinity_prompt_cache_key(
        "openrouter",
        is_session_affinity_key_enabled("openrouter", true, false),
        &OpenAIPromptCacheKeyMode::Off,
        Some("session-lineage-9"),
    );
    assert_eq!(key.as_deref(), Some("vtcode:openrouter:session-lineage-9"));

    let key = build_session_affinity_prompt_cache_key(
        "xai",
        is_session_affinity_key_enabled("xai", true, false),
        &OpenAIPromptCacheKeyMode::Off,
        Some("session-lineage-9"),
    );
    assert_eq!(key.as_deref(), Some("vtcode:xai:session-lineage-9"));

    // Global prompt-cache off leaves production lineage unset.
    assert_eq!(
        build_session_affinity_prompt_cache_key(
            "openrouter",
            is_session_affinity_key_enabled("openrouter", false, true),
            &OpenAIPromptCacheKeyMode::Session,
            Some("session-lineage-9"),
        ),
        None
    );
}

#[test]
fn merge_gateway_prompt_cache_key_is_stable_and_unsuffixed() {
    // Interactive request assembly must pass this mapped key through unchanged
    // (no `{stable_prefix_hash}` suffix). Prefix identity is tracked in
    // tool_catalog_hash / system_prompt_prefix_hash, not the wire key —
    // same contract as AgentRunner after c714e4593.
    let key = build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, Some("lineage-abc-123"))
        .map(|key| map_prompt_cache_key_for_provider("merge-gateway", key));

    assert_eq!(key, Some("vtcode:merge:lineage-abc-123".to_string()));
}

#[test]
fn openai_prompt_cache_key_honors_off_mode_or_disabled_cache() {
    assert_eq!(build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Off, Some("lineage-1"),), None);
    assert_eq!(build_openai_prompt_cache_key(false, &OpenAIPromptCacheKeyMode::Session, Some("lineage-1"),), None);
    assert_eq!(build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, None,), None);
}

#[test]
fn harness_streaming_bridge_emits_incremental_agent_and_reasoning_items() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("harness.jsonl");
    let emitter = crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
        .expect("harness emitter");

    // The second output delta must cross MIN_OUTPUT_UPDATE_BYTES (1024) for the
    // throttled incremental `item.updated` event to fire.
    let tail = " world".repeat(200);
    let full_output = format!("hello{tail}");

    let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_123", 1, 1);
    bridge.on_progress(StreamProgressEvent::ReasoningStage("analysis".to_string()));
    bridge.on_progress(StreamProgressEvent::ReasoningDelta("think".to_string()));
    bridge.on_progress(StreamProgressEvent::OutputDelta("hello".to_string()));
    bridge.on_progress(StreamProgressEvent::OutputDelta(tail.clone()));
    bridge.complete_open_items();

    let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
    let mut saw_assistant_started = false;
    let mut saw_assistant_updated = false;
    let mut saw_assistant_completed = false;
    let mut saw_reasoning_started = false;
    let mut saw_reasoning_completed = false;

    for line in payload.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("json");
        let event = value.get("event").expect("event");
        let event_type = event.get("type").and_then(|kind| kind.as_str()).unwrap_or_default();
        let item_type = event
            .get("item")
            .and_then(|item| item.get("type"))
            .and_then(|kind| kind.as_str())
            .unwrap_or_default();
        let item_text = event
            .get("item")
            .and_then(|item| item.get("text"))
            .and_then(|text| text.as_str())
            .unwrap_or_default();

        if event_type == "item.started" && item_type == "agent_message" {
            saw_assistant_started = item_text == "hello";
        }
        if event_type == "item.updated" && item_type == "agent_message" {
            saw_assistant_updated = item_text == full_output;
        }
        if event_type == "item.completed" && item_type == "agent_message" {
            saw_assistant_completed = item_text == full_output;
        }
        if event_type == "item.started" && item_type == "reasoning" {
            saw_reasoning_started = item_text == "think";
        }
        if event_type == "item.completed" && item_type == "reasoning" {
            let stage = event
                .get("item")
                .and_then(|item| item.get("stage"))
                .and_then(|stage| stage.as_str())
                .unwrap_or_default();
            saw_reasoning_completed = item_text == "think" && stage == "analysis";
        }
    }

    assert!(saw_assistant_started);
    assert!(saw_assistant_updated);
    assert!(saw_assistant_completed);
    assert!(saw_reasoning_started);
    assert!(saw_reasoning_completed);
}

#[test]
fn harness_streaming_bridge_throttles_reasoning_update_events() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("harness.jsonl");
    let emitter = crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
        .expect("harness emitter");

    let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_789", 2, 1);
    bridge.on_progress(StreamProgressEvent::ReasoningStage("analysis".to_string()));
    bridge.on_progress(StreamProgressEvent::ReasoningDelta("seed".to_string()));
    for _ in 0..12 {
        bridge.on_progress(StreamProgressEvent::ReasoningDelta("tiny".to_string()));
    }
    bridge.on_progress(StreamProgressEvent::ReasoningStage("diagnosing".to_string()));
    bridge.on_progress(StreamProgressEvent::ReasoningDelta("x".repeat(200)));
    bridge.on_progress(StreamProgressEvent::ReasoningStage("final".to_string()));
    bridge.complete_open_items();

    let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
    let reasoning_updates = payload
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| {
            value
                .get("event")
                .and_then(|event| event.get("type"))
                .and_then(|kind| kind.as_str())
                == Some("item.updated")
                && value
                    .get("event")
                    .and_then(|event| event.get("item"))
                    .and_then(|item| item.get("type"))
                    .and_then(|kind| kind.as_str())
                    == Some("reasoning")
        })
        .count();

    assert!(reasoning_updates <= 2, "expected throttled reasoning updates, got {reasoning_updates}");
    assert!(reasoning_updates >= 1, "expected at least one meaningful reasoning update, got {reasoning_updates}");
}

#[test]
fn harness_streaming_bridge_abort_closes_open_items() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("harness.jsonl");
    let emitter = crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
        .expect("harness emitter");

    let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_456", 3, 2);
    bridge.on_progress(StreamProgressEvent::OutputDelta("partial".to_string()));
    bridge.abort();

    let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
    let completed_count = payload
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| {
            value
                .get("event")
                .and_then(|event| event.get("type"))
                .and_then(|kind| kind.as_str())
                == Some("item.completed")
        })
        .count();
    assert_eq!(completed_count, 1, "abort should close active stream item");
}

#[test]
fn harness_streaming_bridge_emits_tool_invocation_items() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("harness.jsonl");
    let emitter = crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
        .expect("harness emitter");

    let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_tool", 4, 1);
    bridge.on_progress(StreamProgressEvent::ToolCallStarted {
        call_id: "call_1".to_string(),
        name: Some("shell".to_string()),
    });
    bridge.on_progress(StreamProgressEvent::ToolCallDelta {
        call_id: "call_1".to_string(),
        delta: "{\"cmd\":\"ec".to_string(),
    });
    bridge.on_progress(StreamProgressEvent::ToolCallDelta {
        call_id: "call_1".to_string(),
        delta: "ho hi\"}".to_string(),
    });
    bridge.complete_open_items();

    let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
    let mut saw_started = false;
    let mut saw_updated = false;
    let mut saw_completed = false;

    for line in payload.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("json");
        let event = value.get("event").expect("event");
        let event_type = event.get("type").and_then(|kind| kind.as_str()).unwrap_or_default();
        let item = event.get("item").expect("item");
        let item_type = item.get("type").and_then(|kind| kind.as_str()).unwrap_or_default();

        if item_type != "tool_invocation" {
            continue;
        }

        let tool_name = item.get("tool_name").and_then(|name| name.as_str()).unwrap_or_default();
        let tool_call_id = item.get("tool_call_id").and_then(|id| id.as_str()).unwrap_or_default();
        let status = item.get("status").and_then(|status| status.as_str()).unwrap_or_default();
        let arguments = item.get("arguments");

        if event_type == "item.started" {
            saw_started = tool_name == "shell" && tool_call_id == "call_1" && status == "in_progress";
        }
        if event_type == "item.updated" {
            saw_updated = tool_name == "shell"
                && tool_call_id == "call_1"
                && status == "in_progress"
                && arguments.and_then(|value| value.get("cmd")).and_then(|cmd| cmd.as_str()) == Some("echo hi");
        }
        if event_type == "item.completed" {
            saw_completed = tool_name == "shell"
                && tool_call_id == "call_1"
                && status == "completed"
                && arguments.and_then(|value| value.get("cmd")).and_then(|cmd| cmd.as_str()) == Some("echo hi");
        }
    }

    assert!(saw_started);
    assert!(saw_updated);
    assert!(!saw_completed);
}

#[test]
fn harness_streaming_bridge_tracks_streamed_tool_call_item_ids() {
    let mut bridge = HarnessStreamingBridge::new(None, "turn_tool_map", 5, 2);
    bridge.on_progress(StreamProgressEvent::ToolCallStarted {
        call_id: "call_42".to_string(),
        name: Some("shell".to_string()),
    });

    let item_ids = bridge.take_streamed_tool_call_items();
    assert_eq!(
        item_ids.get("call_42").map(|(item_id, _)| item_id.as_str()),
        Some("turn_tool_map-step-5-assistant-stream-2-tool-call-call_42")
    );
    assert_eq!(item_ids.get("call_42").map(|(_, name)| name.as_str()), Some("shell"));
}

#[tokio::test]
async fn resume_wake_during_non_streaming_request_preserves_fresh_planning() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.activate_planning_for_test();
    let mut ctx = backing.turn_processing_context();
    ctx.ctrl_c_state.request_local_cancel();
    ctx.ctrl_c_state.mark_cancel_handled();
    ctx.ctrl_c_state.reset();
    let mut provider = ScriptedProvider::new(
        "mycorp",
        false,
        Arc::new(Mutex::new(Vec::new())),
        vec![ScriptedProviderOutcome::Success {
            content: Some("continued planning"),
            request_id: None,
        }],
    );
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    provider.resume_wake = Some(ResumeWake {
        notify: ctx.ctrl_c_notify.clone(),
        entered: entered.clone(),
        release: release.clone(),
    });
    *ctx.provider_client = Box::new(provider);
    ctx.working_history
        .push(uni::Message::user("Continue drafting the proposal.".into()));
    let request = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None);
    let finish = async {
        entered.notified().await;
        task::yield_now().await;
        release.notify_one();
    };
    let (response, ()) = tokio::join!(request, finish);
    let response = response.expect("fresh planning request");
    assert_eq!(response.0.content.as_deref(), Some("continued planning"));
    assert!(ctx.is_planning_active());
    assert!(!ctx.ctrl_c_state.is_cancel_requested());
}

#[tokio::test]
async fn anthropic_fast_capacity_errors_use_existing_bounded_retries_without_clearing_preference() {
    for status in [429, 529] {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let provider = ScriptedProvider::new(
            "anthropic",
            false,
            Arc::clone(&calls),
            vec![
                ScriptedProviderOutcome::Error(uni::LLMError::Provider {
                    message: "capacity unavailable".to_owned(),
                    metadata: Some(uni::LLMErrorMetadata::new(
                        "anthropic",
                        Some(status),
                        None,
                        None,
                        None,
                        Some("7".to_owned()),
                        None,
                    )),
                }),
                ScriptedProviderOutcome::Success {
                    content: Some("fast capacity recovered"),
                    request_id: None,
                },
            ],
        );
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut cfg = vtcode_config::VTCodeConfig::default();
        cfg.provider.anthropic.speed = Some(vtcode_config::AnthropicSpeed::Fast);
        backing.set_vt_cfg_for_test(cfg);
        let mut ctx = backing.turn_processing_context();
        *ctx.provider_client = Box::new(provider);
        ctx.working_history.push(uni::Message::user("continue".to_owned()));
        execute_llm_request(&mut ctx, 1, "claude-opus-5-5", Some(320), false, None)
            .await
            .unwrap();
        assert_eq!(calls.lock().unwrap().len(), 2, "status {status}");
        assert_eq!(ctx.vt_cfg.unwrap().provider.anthropic.speed, Some(vtcode_config::AnthropicSpeed::Fast));
    }
}
