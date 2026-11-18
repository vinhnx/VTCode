use super::AgentSessionState;
use crate::config::types::ReasoningEffortLevel;
use crate::llm::provider::Message;
use crate::llm::providers::gemini::wire::Part;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// `cache_gap_exceeds`/`note_request_sent` delegate to the shared
/// `RequestGapTracker`, which owns the full behavioral test coverage
/// (see `crate::llm::request_gap`); this just pins the delegation.
#[test]
fn cache_gap_exceeds_delegates_to_request_gap_tracker() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    assert_eq!(state.cache_gap_exceeds(Duration::from_millis(1)), None);

    state.note_request_sent();
    thread::sleep(Duration::from_millis(15));
    let gap = state.cache_gap_exceeds(Duration::from_millis(5));
    assert!(gap.is_some_and(|elapsed| elapsed >= Duration::from_millis(15)));
}

#[test]
fn note_reasoning_effort_change_is_false_on_first_request() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    assert!(!state.note_reasoning_effort_change(Some(ReasoningEffortLevel::Medium)));
}

#[test]
fn note_reasoning_effort_change_is_false_when_unchanged() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    assert!(!state.note_reasoning_effort_change(Some(ReasoningEffortLevel::Medium)));
    assert!(!state.note_reasoning_effort_change(Some(ReasoningEffortLevel::Medium)));
}

#[test]
fn note_reasoning_effort_change_is_true_when_changed() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    assert!(!state.note_reasoning_effort_change(Some(ReasoningEffortLevel::Medium)));
    assert!(state.note_reasoning_effort_change(Some(ReasoningEffortLevel::High)));
    // Baseline is now High; requesting High again is not a change.
    assert!(!state.note_reasoning_effort_change(Some(ReasoningEffortLevel::High)));
}

#[test]
fn note_model_change_is_false_on_first_request() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    assert!(!state.note_model_change("gpt-5.6"));
}

#[test]
fn note_model_change_is_false_when_unchanged() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    assert!(!state.note_model_change("gpt-5.6"));
    assert!(!state.note_model_change("gpt-5.6"));
}

#[test]
fn note_model_change_is_true_when_changed() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    assert!(!state.note_model_change("gpt-5.6"));
    assert!(state.note_model_change("claude-opus-4-6"));
    // Baseline is now Opus; requesting Opus again is not a change.
    assert!(!state.note_model_change("claude-opus-4-6"));
}

#[test]
fn previous_response_chain_is_scoped_to_provider_and_model() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    let messages_52 = vec![Message::user("hello".to_string())];
    let messages_54 = vec![Message::user("continue".to_string())];

    state.set_previous_response_chain("openai", "gpt-5.6", Some("resp_123"), messages_52.clone());
    state.set_previous_response_chain("openai", "gpt-5.6-sol", Some("resp_456"), messages_54.clone());

    assert_eq!(state.previous_response_id_for("openai", "gpt-5.6"), Some("resp_123".to_string()));
    assert_eq!(state.previous_response_id_for("openai", "gpt-5.6-sol"), Some("resp_456".to_string()));
    assert_eq!(state.previous_response_id_for("gemini", "gpt-5.6"), None);

    state.clear_previous_response_chain_for("openai", "gpt-5.6");

    assert_eq!(state.previous_response_id_for("openai", "gpt-5.6"), None);
    assert_eq!(state.previous_response_chain_for("openai", "gpt-5.6"), None);
    assert_eq!(state.previous_response_id_for("openai", "gpt-5.6-sol"), Some("resp_456".to_string()));
    assert_eq!(
        state
            .previous_response_chain_for("openai", "gpt-5.6-sol")
            .map(|chain| chain.messages.as_slice()),
        Some(messages_54.as_slice())
    );

    state.clear_previous_response_chain();
    assert_eq!(state.previous_response_id_for("openai", "gpt-5.6-sol"), None);
    assert_eq!(state.previous_response_chain_for("openai", "gpt-5.6-sol"), None);
}

#[test]
fn shared_previous_response_chain_reuses_history_arc() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    let messages = Arc::new(vec![Message::user("hello".to_string())]);

    state.set_previous_response_chain_shared("openai", "gpt-5.6-sol", Some("resp_123"), Arc::clone(&messages));

    let stored = &state
        .previous_response_chain_for("openai", "gpt-5.6-sol")
        .expect("continuation state")
        .messages;
    assert!(Arc::ptr_eq(&messages, stored));
}

#[test]
fn push_tool_error_preserves_structured_json_for_gemini() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    let payload = serde_json::json!({
        "error": {
            "tool_name": "read_file",
            "message": "missing file",
            "category": "ResourceNotFound"
        }
    });

    state.push_tool_error("call_1".to_string(), "read_file", &payload, true);

    match &state.conversation[0].parts[0] {
        Part::FunctionResponse { function_response, .. } => {
            assert_eq!(function_response.response["error"]["message"], "missing file");
        }
        other => panic!("expected function response, got {other:?}"),
    }
    let expected_serialized = serde_json::to_string(&payload).unwrap();
    assert_eq!(state.messages[0], Message::tool_response("call_1".to_string(), expected_serialized));
}

#[test]
fn harness_notices_are_tagged_synthetic_while_user_messages_are_not() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    state.add_user_message("fix the flaky test".to_string());
    state.add_synthetic_user_message("It looks like you're repeating the same response.".to_string());
    state.add_user_message_with_intent("steer: stop after this".to_string(), Some("intent-1".to_string()));

    let sources: Vec<Option<&str>> = state
        .messages
        .iter()
        .map(|message| message.metadata.as_ref().and_then(|metadata| metadata.source()))
        .collect();
    assert_eq!(sources, vec![Some("user_input"), Some("synthetic"), Some("user_input")]);
    assert!(
        state.messages[1]
            .metadata
            .as_ref()
            .is_some_and(vtcode_commons::message_metadata::MessageMetadata::is_synthetic)
    );
    // Tags survive archive round-trips so reverse history can filter them.
    let archived: Vec<crate::utils::session_archive::SessionMessage> = state.messages.iter().map(Into::into).collect();
    assert!(
        archived[1]
            .metadata
            .as_deref()
            .is_some_and(vtcode_commons::message_metadata::MessageMetadata::is_synthetic)
    );
    assert!(archived[0].metadata.as_deref().is_some_and(|m| !m.is_synthetic()));
}

#[test]
fn cached_total_tokens_matches_direct_computation() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);

    // Add messages through the standard push methods
    state.add_user_message("Hello, how are you?".to_string());
    state.push_tool_result(
        "call_1".to_string(),
        "read_file",
        &serde_json::json!({"content": "test file content"}),
        false,
    );
    state.push_tool_error(
        "call_2".to_string(),
        "write_file",
        &serde_json::json!({"error": "permission denied"}),
        false,
    );

    // Cached value should match direct computation
    let direct = state.messages.iter().map(|m| m.estimate_tokens()).sum::<usize>();
    assert_eq!(state.total_tokens(), direct);
    assert!(state.total_tokens() > 0);
}

#[test]
fn reconcile_token_count_resyncs_after_external_mutation() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    state.add_user_message("test message".to_string());
    let before = state.total_tokens();

    // Simulate external mutation (bypassing push methods)
    state.messages_mut().push(Message::assistant("extra response".to_string()));
    assert_ne!(state.total_tokens(), before + Message::assistant("extra response".to_string()).estimate_tokens());

    // Reconcile should fix it
    state.reconcile_token_count();
    let expected = state.messages.iter().map(|m| m.estimate_tokens()).sum::<usize>();
    assert_eq!(state.total_tokens(), expected);
}

#[test]
fn clear_conversation_history_resets_all_state() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    state.messages_mut().push(Message::user("hello".to_string()));
    state.messages_mut().push(Message::assistant("hi".to_string()));
    state.conversation.push(crate::llm::providers::gemini::wire::Content {
        role: "user".to_string(),
        parts: vec![Part::Text { text: "hello".to_string(), thought_signature: None }],
    });
    state.reconcile_token_count();
    state.last_processed_message_idx = 2;
    state.progress_hashes.push_back(123);
    state.stagnant_turns = 3;
    state.set_previous_response_chain("openai", "gpt-5", Some("resp_1"), vec![]);

    assert!(!state.messages.is_empty());
    assert!(!state.conversation.is_empty());
    assert!(state.total_tokens() > 0);

    state.clear_conversation_history();

    assert!(state.messages.is_empty());
    assert!(state.conversation.is_empty());
    assert_eq!(state.total_tokens(), 0);
    assert_eq!(state.last_processed_message_idx, 0);
    assert!(state.progress_hashes.is_empty());
    assert_eq!(state.stagnant_turns, 0);
    assert!(state.previous_response_chains.is_empty());
}

#[test]
fn push_warning_skips_exact_duplicates() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    state.push_warning("rate limited");
    state.push_warning("rate limited");
    state.push_warning("rate limited");
    assert_eq!(state.warnings.len(), 1);
    assert_eq!(state.warnings[0], "rate limited");
}

#[test]
fn push_warning_retains_distinct_warnings() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    state.push_warning("first warning");
    state.push_warning("second warning");
    state.push_warning("third warning");
    assert_eq!(state.warnings.len(), 3);
    assert_eq!(state.warnings[0], "first warning");
    assert_eq!(state.warnings[1], "second warning");
    assert_eq!(state.warnings[2], "third warning");
}

#[test]
fn push_warning_inserts_elision_marker_at_cap() {
    let mut state = AgentSessionState::new("session".to_string(), 4, 16_000);
    // Fill up to the cap with unique warnings.
    for i in 0..super::MAX_SESSION_WARNINGS {
        state.push_warning(format!("warning {i}"));
    }
    assert_eq!(state.warnings.len(), super::MAX_SESSION_WARNINGS);
    assert!(!state.warnings.iter().any(|w| w == super::WARNINGS_ELIDED_MARKER));

    // Next unique warning triggers the marker exactly once.
    state.push_warning("overflow A");
    assert_eq!(state.warnings.len(), super::MAX_SESSION_WARNINGS + 1);
    assert_eq!(state.warnings.last().unwrap(), super::WARNINGS_ELIDED_MARKER);

    // Further unique warnings are silently dropped — marker stays as the
    // last entry and the count does not grow.
    state.push_warning("overflow B");
    state.push_warning("overflow C");
    assert_eq!(state.warnings.len(), super::MAX_SESSION_WARNINGS + 1);
    assert_eq!(state.warnings.last().unwrap(), super::WARNINGS_ELIDED_MARKER);
    // The overflow warnings themselves were not stored.
    assert!(!state.warnings.iter().any(|w| w == "overflow A"));
    assert!(!state.warnings.iter().any(|w| w == "overflow B"));
}
