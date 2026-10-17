use super::*;
use std::borrow::Cow;

#[test]
fn normalize_history_for_request_borrows_clean_history() {
    let manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);
    let history = vec![
        uni::Message::user("hello".to_string()),
        uni::Message::assistant("world".to_string()),
    ];

    let normalized = manager.normalize_history_for_request(&history);

    assert!(matches!(normalized, Cow::Borrowed(_)));
}

#[test]
fn normalize_history_for_request_owns_history_needing_normalization() {
    let manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);
    let history = vec![
        uni::Message::assistant("part one".to_string()),
        uni::Message::assistant("part two".to_string()),
    ];

    let normalized = manager.normalize_history_for_request(&history);

    assert!(matches!(normalized, Cow::Owned(_)));
}

#[test]
fn normalize_history_for_request_drops_empty_noop_messages() {
    let manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);
    let history = vec![
        uni::Message::user("hello".to_string()),
        uni::Message::assistant("   ".to_string()),
        uni::Message::assistant("world".to_string()),
    ];

    let normalized = manager.normalize_history_for_request(&history);
    assert_eq!(normalized.len(), 2);
    assert_eq!(normalized[0].content.as_text(), "hello");
    assert_eq!(normalized[1].content.as_text(), "world");
}

#[test]
fn normalize_history_for_request_merges_plain_assistant_text_messages() {
    let manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);
    let history = vec![
        uni::Message::assistant("part one".to_string()),
        uni::Message::assistant("part two".to_string()),
        uni::Message::user("continue".to_string()),
    ];

    let normalized = manager.normalize_history_for_request(&history);
    assert_eq!(normalized.len(), 2);
    assert_eq!(normalized[0].content.as_text(), "part one\npart two");
    assert_eq!(normalized[1].content.as_text(), "continue");
}

#[test]
fn normalize_history_for_request_keeps_different_assistant_phases_separate() {
    let manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);
    let history = vec![
        uni::Message::assistant("working".to_string()).with_phase(Some(uni::AssistantPhase::Commentary)),
        uni::Message::assistant("done".to_string()).with_phase(Some(uni::AssistantPhase::FinalAnswer)),
    ];

    let normalized = manager.normalize_history_for_request(&history);
    assert_eq!(normalized.len(), 2);
    assert_eq!(normalized[0].phase, Some(uni::AssistantPhase::Commentary));
    assert_eq!(normalized[1].phase, Some(uni::AssistantPhase::FinalAnswer));
}

#[test]
fn normalize_history_for_request_keeps_tool_sequences_intact() {
    let manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);
    let history = vec![
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "call_1".to_string(),
                "read_file".to_string(),
                "{}".to_string(),
            )],
        ),
        uni::Message::tool_response("call_1".to_string(), "{\"ok\":true}".to_string()),
        uni::Message::assistant("done".to_string()),
    ];

    let normalized = manager.normalize_history_for_request(&history);
    assert_eq!(normalized.len(), 3);
    assert!(normalized[0].tool_calls.is_some());
    assert_eq!(normalized[1].role, uni::MessageRole::Tool);
}

#[test]
fn normalize_history_for_request_inserts_synthetic_outputs_for_missing_calls() {
    let manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);
    let history = vec![uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            "call_1".to_string(),
            "read_file".to_string(),
            "{}".to_string(),
        )],
    )];

    let normalized = manager.normalize_history_for_request(&history);
    assert_eq!(normalized.len(), 2);
    assert!(normalized[0].tool_calls.is_some());
    assert_eq!(normalized[1].tool_call_id.as_deref(), Some("call_1"));
    assert!(normalized[1].content.as_text().contains("canceled"));
}

#[test]
fn normalize_history_for_request_removes_orphan_outputs() {
    let manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);
    let history = vec![uni::Message::tool_response(
        "orphan_call".to_string(),
        "{\"ok\":true}".to_string(),
    )];

    let normalized = manager.normalize_history_for_request(&history);
    assert!(normalized.is_empty());
}

#[test]
fn normalize_history_for_request_moves_intervening_messages_after_all_batch_results() {
    let manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);
    let history = vec![
        uni::Message::assistant_with_tools(
            String::new(),
            vec![
                uni::ToolCall::function("call_1".to_string(), "read_file".to_string(), "{}".to_string()),
                uni::ToolCall::function("call_2".to_string(), "read_file".to_string(), "{}".to_string()),
            ],
        ),
        uni::Message::system("intervening".to_string()),
        uni::Message::tool_response("call_2".to_string(), "result two".to_string()),
        uni::Message::user("later note".to_string()),
        uni::Message::tool_response("call_1".to_string(), "result one".to_string()),
    ];
    let durable_before = history.clone();

    let normalized = manager.normalize_history_for_request(&history);

    assert_eq!(normalized[1].tool_call_id.as_deref(), Some("call_2"));
    assert_eq!(normalized[2].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(normalized[3].role, uni::MessageRole::System);
    assert_eq!(normalized[4].role, uni::MessageRole::User);
    assert_eq!(history, durable_before);
}

#[test]
fn normalize_history_for_request_is_idempotent_after_request_repair() {
    let manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);
    let history = vec![
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "call_1".to_string(),
                "read_file".to_string(),
                "{}".to_string(),
            )],
        ),
        uni::Message::system("intervening".to_string()),
        uni::Message::tool_response("call_1".to_string(), "result".to_string()),
    ];

    let first = manager.normalize_history_for_request(&history).into_owned();
    let second = manager.normalize_history_for_request(&first);

    assert!(matches!(second, Cow::Borrowed(_)));
    assert_eq!(second, first);
}

#[tokio::test]
async fn build_system_prompt_with_empty_base_prompt_fails() {
    let mut manager = ContextManager::new("".to_string(), (), Arc::new(RwLock::new(HashMap::new())), None);

    let params = SystemPromptParams {
        full_auto: false,
        planning_active: false,
        request_user_input_enabled: true,
    };

    let result = manager.build_system_prompt(params).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));
}

#[test]
fn test_update_token_usage_prefers_prompt_pressure() {
    let mut manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);

    // Initial state
    assert_eq!(manager.current_token_usage(), 0);

    // Update with first response: prompt-side pressure becomes authoritative.
    manager.update_token_usage(&Some(uni::Usage {
        prompt_tokens: 1000,
        completion_tokens: 500,
        total_tokens: 1500,
        cached_prompt_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        iterations: None,
    }));
    assert_eq!(manager.current_token_usage(), 1000);

    // Update with second response: usage tracks latest prompt pressure, not cumulative output.
    manager.update_token_usage(&Some(uni::Usage {
        prompt_tokens: 2500,
        completion_tokens: 800,
        total_tokens: 3300,
        cached_prompt_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        iterations: None,
    }));
    assert_eq!(manager.current_token_usage(), 2500);
}

#[test]
fn test_update_token_usage_falls_back_when_prompt_missing() {
    let mut manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);

    manager.update_token_usage(&Some(uni::Usage {
        prompt_tokens: 0,
        completion_tokens: 800,
        total_tokens: 3300,
        cached_prompt_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        iterations: None,
    }));

    // Fallback estimate = total - completion.
    assert_eq!(manager.current_token_usage(), 2500);
}

#[test]
fn test_prompt_estimate_includes_composed_instruction_growth() {
    let mut manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);

    manager.record_prompt_estimate(160_000);
    assert_eq!(manager.current_token_usage(), 160_000);

    // A smaller estimate must not erase pressure observed from a larger
    // assembled prompt before the provider returns authoritative usage.
    manager.record_prompt_estimate(12_000);
    assert_eq!(manager.current_token_usage(), 160_000);
}

#[test]
fn test_compaction_resets_prompt_pressure_for_the_next_request() {
    let mut manager = ContextManager::new("sys".into(), (), Arc::new(RwLock::new(HashMap::new())), None);

    manager.record_prompt_estimate(160_000);
    assert!(manager.mark_compaction_pending_at_soft_threshold(Some(160_000)));

    manager.reset_token_pressure_after_compaction();

    assert_eq!(manager.current_token_usage(), 0);
    assert!(!manager.compaction_pending());
}

#[tokio::test]
async fn build_system_prompt_ignores_token_usage_updates() {
    let mut manager = ContextManager::new("System prompt".to_string(), (), Arc::new(RwLock::new(HashMap::new())), None);

    let params = SystemPromptParams {
        full_auto: false,
        planning_active: false,
        request_user_input_enabled: true,
    };

    let prompt_before = manager
        .build_system_prompt(params.clone())
        .await
        .expect("prompt before token update");

    manager.update_token_usage(&Some(uni::Usage {
        prompt_tokens: 180_000,
        completion_tokens: 2_000,
        total_tokens: 182_000,
        cached_prompt_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        iterations: None,
    }));

    let prompt_after = manager.build_system_prompt(params).await.expect("prompt after token update");

    assert_eq!(prompt_before, prompt_after);
    assert!(!prompt_after.contains("<budget:token_budget>"));
    assert!(!prompt_after.contains("token_usage:"));
    assert!(!prompt_after.contains("[Context]"));
    assert!(!prompt_after.contains("[Runtime Context]"));
}
