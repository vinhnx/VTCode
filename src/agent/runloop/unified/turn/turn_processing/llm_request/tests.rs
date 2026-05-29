use super::*;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

struct ScriptedProvider {
    recorded_previous_response_ids: Arc<Mutex<Vec<Option<String>>>>,
    outcomes: Mutex<VecDeque<ScriptedProviderOutcome>>,
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
        recorded_previous_response_ids: Arc<Mutex<Vec<Option<String>>>>,
        outcomes: Vec<ScriptedProviderOutcome>,
    ) -> Self {
        Self {
            recorded_previous_response_ids,
            outcomes: Mutex::new(VecDeque::from(outcomes)),
        }
    }
}

#[async_trait::async_trait]
impl uni::LLMProvider for ScriptedProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    async fn generate(
        &self,
        request: uni::LLMRequest,
    ) -> std::result::Result<uni::LLMResponse, uni::LLMError> {
        self.recorded_previous_response_ids
            .lock()
            .expect("previous_response_id recorder")
            .push(request.previous_response_id.clone());

        match self
            .outcomes
            .lock()
            .expect("provider script")
            .pop_front()
            .expect("provider script should have enough outcomes")
        {
            ScriptedProviderOutcome::Success {
                content,
                request_id,
            } => Ok(uni::LLMResponse {
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

    fn supported_models(&self) -> Vec<String> {
        vec!["noop-model".to_string()]
    }

    fn validate_request(
        &self,
        _request: &uni::LLMRequest,
    ) -> std::result::Result<(), uni::LLMError> {
        Ok(())
    }
}

#[test]
fn retryable_llm_error_includes_internal_server_error_message() {
    assert!(is_retryable_llm_error(
        "Provider error: Internal Server Error"
    ));
}

#[test]
fn retryable_llm_error_excludes_non_transient_messages() {
    assert!(!is_retryable_llm_error("Provider error: Invalid API key"));
}

#[tokio::test]
async fn previous_response_chain_retry_keeps_real_retry_backoff() {
    use vtcode_core::utils::transcript;

    transcript::clear();

    let recorded_previous_response_ids = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        Arc::clone(&recorded_previous_response_ids),
        vec![
            ScriptedProviderOutcome::Error(uni::LLMError::Provider {
                message: "OpenAI error: previous_response_not_found: previous response missing"
                    .to_string(),
                metadata: None,
            }),
            ScriptedProviderOutcome::Error(uni::LLMError::Provider {
                message: "Provider error: 503 Service Unavailable".to_string(),
                metadata: None,
            }),
            ScriptedProviderOutcome::Success {
                content: Some("done"),
                request_id: Some("resp_456"),
            },
        ],
    );

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let prior_messages = vec![uni::Message::user("hello".to_string())];
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    ctx.working_history.extend(prior_messages.clone());
    ctx.working_history
        .push(uni::Message::user("continue".to_string()));
    ctx.session_stats.set_previous_response_chain(
        "openai",
        "noop-model",
        Some("resp_123"),
        &prior_messages,
    );

    let (response, streamed) =
        execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None)
            .await
            .expect("request should recover from stale response chain");

    assert!(!streamed);
    assert_eq!(response.content.as_deref(), Some("done"));
    assert_eq!(
        recorded_previous_response_ids
            .lock()
            .expect("recorded previous_response_ids")
            .as_slice(),
        &[Some("resp_123".to_string()), None, None]
    );

    let retry_label =
        vtcode_commons::classify_error_message("Provider error: 503 Service Unavailable")
            .user_label()
            .to_string();
    let transcript_text = transcript::snapshot().join("\n");

    assert_eq!(
        transcript_text
            .matches("Previous response chain expired; retrying with a fresh provider chain.")
            .count(),
        1
    );
    assert_eq!(
        transcript_text
            .matches(&format!(
                "LLM request failed ({}), retrying in 0.5s... (attempt 2/3)",
                retry_label
            ))
            .count(),
        0
    );

    transcript::clear();
}

#[test]
fn retryable_llm_error_excludes_forbidden_quota_failures() {
    assert!(!is_retryable_llm_error(
        "Provider error: HuggingFace API error (403 Forbidden): {\"error\":\"You have exceeded your monthly spending limit.\"}"
    ));
}

#[test]
fn retryable_llm_error_includes_rate_limit_429() {
    assert!(is_retryable_llm_error(
        "Provider error: 429 Too Many Requests"
    ));
}

#[test]
fn retryable_llm_error_includes_service_unavailable_class() {
    assert!(is_retryable_llm_error(
        "Provider error: 503 Service Unavailable"
    ));
    assert!(is_retryable_llm_error(
        "Provider error: 504 Gateway Timeout"
    ));
}

#[test]
fn previous_response_chain_error_detects_provider_code() {
    assert!(is_previous_response_chain_error(
        "OpenAI error: previous_response_not_found: previous response missing"
    ));
}

#[test]
fn previous_response_chain_error_detects_human_readable_message() {
    assert!(is_previous_response_chain_error(
        "Previous response with id 'resp_cached' not found."
    ));
}

#[test]
fn previous_response_chain_error_ignores_service_unavailable() {
    assert!(!is_previous_response_chain_error(
        "Provider error: 503 Service Unavailable"
    ));
}

#[test]
fn retryable_llm_error_excludes_usage_limit_messages() {
    assert!(!is_retryable_llm_error(
        "Provider error: you have reached your weekly usage limit"
    ));
}

#[test]
fn supports_streaming_timeout_fallback_covers_supported_providers() {
    assert!(supports_streaming_timeout_fallback("huggingface"));
    assert!(supports_streaming_timeout_fallback("ollama"));
    assert!(supports_streaming_timeout_fallback("minimax"));
    assert!(supports_streaming_timeout_fallback("HUGGINGFACE"));
    assert!(!supports_streaming_timeout_fallback("openai"));
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
    assert!(is_stream_timeout_error(
        "Stream request timed out after 75s"
    ));
    assert!(is_stream_timeout_error(
        "Streaming request timed out after configured timeout"
    ));
    assert!(is_stream_timeout_error(
        "LLM request timed out after 120 seconds"
    ));
}

#[test]
fn llm_attempt_timeout_defaults_to_fifth_of_turn_budget() {
    assert_eq!(llm_attempt_timeout_secs(300, false, "openai"), 60);
}

#[test]
fn llm_attempt_timeout_expands_for_plan_mode() {
    assert_eq!(llm_attempt_timeout_secs(300, true, "openai"), 120);
}

#[test]
fn llm_attempt_timeout_plan_mode_respects_smaller_turn_budget() {
    assert_eq!(llm_attempt_timeout_secs(180, true, "openai"), 90);
}

#[test]
fn llm_attempt_timeout_plan_mode_huggingface_uses_higher_floor() {
    assert_eq!(llm_attempt_timeout_secs(150, true, "huggingface"), 90);
}

#[test]
fn llm_timeout_warning_delay_targets_three_quarters_of_budget() {
    assert_eq!(
        llm_timeout_warning_delay(Duration::from_secs(60)),
        Some(Duration::from_secs(45))
    );
}

#[test]
fn llm_attempt_timeout_respects_plan_mode_cap() {
    assert_eq!(llm_attempt_timeout_secs(1_200, true, "huggingface"), 120);
}

#[test]
fn openai_prompt_cache_enablement_requires_provider_and_flags() {
    assert!(is_openai_prompt_cache_enabled("openai", true, true));
    assert!(!is_openai_prompt_cache_enabled("openai", false, true));
    assert!(!is_openai_prompt_cache_enabled("openai", true, false));
    assert!(!is_openai_prompt_cache_enabled("anthropic", true, true));
}

#[test]
fn prompt_cache_shaping_mode_requires_global_opt_in_and_provider_cache() {
    let mut cfg = PromptCachingConfig {
        enabled: true,
        cache_friendly_prompt_shaping: true,
        ..PromptCachingConfig::default()
    };
    cfg.providers.openai.enabled = true;

    assert_eq!(
        resolve_prompt_cache_shaping_mode("openai", &cfg),
        PromptCacheShapingMode::TrailingRuntimeContext
    );

    cfg.cache_friendly_prompt_shaping = false;
    assert_eq!(
        resolve_prompt_cache_shaping_mode("openai", &cfg),
        PromptCacheShapingMode::Disabled
    );
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

    assert_eq!(
        resolve_prompt_cache_shaping_mode("gemini", &cfg),
        PromptCacheShapingMode::Disabled
    );
}

#[test]
fn openai_prompt_cache_key_uses_stable_session_identifier() {
    let lineage_id = "lineage-abc-123";
    let first =
        build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, Some(lineage_id));
    let second =
        build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, Some(lineage_id));

    assert_eq!(first, Some("vtcode:openai:lineage-abc-123".to_string()));
    assert_eq!(first, second);
}

#[test]
fn openai_prompt_cache_key_honors_off_mode_or_disabled_cache() {
    assert_eq!(
        build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Off, Some("lineage-1"),),
        None
    );
    assert_eq!(
        build_openai_prompt_cache_key(false, &OpenAIPromptCacheKeyMode::Session, Some("lineage-1"),),
        None
    );
    assert_eq!(
        build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, None,),
        None
    );
}

#[test]
fn harness_streaming_bridge_emits_incremental_agent_and_reasoning_items() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("harness.jsonl");
    let emitter =
        crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
            .expect("harness emitter");

    let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_123", 1, 1);
    bridge.on_progress(StreamProgressEvent::ReasoningStage("analysis".to_string()));
    bridge.on_progress(StreamProgressEvent::ReasoningDelta("think".to_string()));
    bridge.on_progress(StreamProgressEvent::OutputDelta("hello".to_string()));
    bridge.on_progress(StreamProgressEvent::OutputDelta(" world".to_string()));
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
        let event_type = event
            .get("type")
            .and_then(|kind| kind.as_str())
            .unwrap_or_default();
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
            saw_assistant_updated = item_text == "hello world";
        }
        if event_type == "item.completed" && item_type == "agent_message" {
            saw_assistant_completed = item_text == "hello world";
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
    let emitter =
        crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
            .expect("harness emitter");

    let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_789", 2, 1);
    bridge.on_progress(StreamProgressEvent::ReasoningStage("analysis".to_string()));
    bridge.on_progress(StreamProgressEvent::ReasoningDelta("seed".to_string()));
    for _ in 0..12 {
        bridge.on_progress(StreamProgressEvent::ReasoningDelta("tiny".to_string()));
    }
    bridge.on_progress(StreamProgressEvent::ReasoningStage(
        "diagnosing".to_string(),
    ));
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

    assert!(
        reasoning_updates <= 2,
        "expected throttled reasoning updates, got {reasoning_updates}"
    );
    assert!(
        reasoning_updates >= 1,
        "expected at least one meaningful reasoning update, got {reasoning_updates}"
    );
}

#[test]
fn harness_streaming_bridge_abort_closes_open_items() {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join("harness.jsonl");
    let emitter =
        crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
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
    let emitter =
        crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
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
        let event_type = event
            .get("type")
            .and_then(|kind| kind.as_str())
            .unwrap_or_default();
        let item = event.get("item").expect("item");
        let item_type = item
            .get("type")
            .and_then(|kind| kind.as_str())
            .unwrap_or_default();

        if item_type != "tool_invocation" {
            continue;
        }

        let tool_name = item
            .get("tool_name")
            .and_then(|name| name.as_str())
            .unwrap_or_default();
        let tool_call_id = item
            .get("tool_call_id")
            .and_then(|id| id.as_str())
            .unwrap_or_default();
        let status = item
            .get("status")
            .and_then(|status| status.as_str())
            .unwrap_or_default();
        let arguments = item.get("arguments");

        if event_type == "item.started" {
            saw_started =
                tool_name == "shell" && tool_call_id == "call_1" && status == "in_progress";
        }
        if event_type == "item.updated" {
            saw_updated = tool_name == "shell"
                && tool_call_id == "call_1"
                && status == "in_progress"
                && arguments
                    .and_then(|value| value.get("cmd"))
                    .and_then(|cmd| cmd.as_str())
                    == Some("echo hi");
        }
        if event_type == "item.completed" {
            saw_completed = tool_name == "shell"
                && tool_call_id == "call_1"
                && status == "completed"
                && arguments
                    .and_then(|value| value.get("cmd"))
                    .and_then(|cmd| cmd.as_str())
                    == Some("echo hi");
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
        item_ids.get("call_42").map(String::as_str),
        Some("turn_tool_map-step-5-assistant-stream-2-tool-call-call_42")
    );
}
