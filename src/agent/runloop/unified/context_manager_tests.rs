use super::*;

#[tokio::test]
async fn pre_request_check_returns_proceed() {
    let manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    let history = vec![uni::Message::user("hello".to_string())];
    assert_eq!(
        manager.pre_request_check(&history, 200_000),
        super::PreRequestAction::Proceed
    );
}

#[test]
fn test_pre_request_check_ignores_conversation_length() {
    use vtcode_config::core::AgentConfig;
    let manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        Some(AgentConfig {
            max_conversation_turns: 50,
            ..Default::default()
        }),
    );

    let mut history = Vec::new();
    for _ in 0..40 {
        history.push(uni::Message::user("test".to_string()));
    }

    assert_eq!(
        manager.pre_request_check(&history, 200_000),
        super::PreRequestAction::Proceed
    );
}

#[test]
fn test_pre_request_check_compacts_on_threshold() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );
    manager.cached_stats.total_token_usage = 170_000;

    let history = vec![uni::Message::user("hello".to_string())];
    assert!(matches!(
        manager.pre_request_check(&history, 200_000),
        super::PreRequestAction::Compact(_)
    ));
}

#[test]
fn test_token_budget_status_thresholds() {
    let manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Context window of 200K tokens
    let context_size = 200_000;

    // Zero usage should be Normal
    assert_eq!(
        manager.get_token_budget_status(context_size),
        TokenBudgetStatus::Normal
    );
}

#[test]
fn test_token_budget_status_with_zero_context() {
    let manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Zero context window should return Normal (avoid division by zero)
    assert_eq!(
        manager.get_token_budget_status(0),
        TokenBudgetStatus::Normal
    );
}

#[tokio::test]
async fn build_system_prompt_with_empty_base_prompt_fails() {
    let mut manager = ContextManager::new(
        "".to_string(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    let params = SystemPromptParams {
        full_auto: false,
        plan_mode: false,
        context_window_size: None,
        active_agent_name: None,
        active_agent_prompt: None,
    };

    let result = manager.build_system_prompt(&[], 0, params).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));
}

#[test]
fn test_token_budget_status_warning_threshold() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Set token usage to 70% (140000/200000)
    manager.cached_stats.total_token_usage = 140_000;

    assert_eq!(
        manager.get_token_budget_status(200_000),
        TokenBudgetStatus::Warning
    );
    assert_eq!(
        manager.get_token_budget_guidance(200_000),
        "WARNING: Update progress docs to preserve context."
    );
}

#[test]
fn test_token_budget_status_high_threshold() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Set token usage to 85% (170000/200000)
    manager.cached_stats.total_token_usage = 170_000;

    assert_eq!(
        manager.get_token_budget_status(200_000),
        TokenBudgetStatus::High
    );
    assert_eq!(
        manager.get_token_budget_guidance(200_000),
        "HIGH: Summarize key findings and prepare a handoff."
    );
}

#[test]
fn test_token_budget_status_critical_threshold() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Set token usage to 90% (180000/200000)
    manager.cached_stats.total_token_usage = 180_000;

    assert_eq!(
        manager.get_token_budget_status(200_000),
        TokenBudgetStatus::Critical
    );
    assert_eq!(
        manager.get_token_budget_guidance(200_000),
        "CRITICAL: Update artifacts and consider a new session."
    );
}

#[test]
fn test_token_budget_status_normal() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Set token usage to 50% (100000/200000)
    manager.cached_stats.total_token_usage = 100_000;

    assert_eq!(
        manager.get_token_budget_status(200_000),
        TokenBudgetStatus::Normal
    );
    assert_eq!(manager.get_token_budget_guidance(200_000), "");
}

#[test]
fn test_token_budget_status_and_guidance_together() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    // Test critical threshold
    manager.cached_stats.total_token_usage = 185_000;
    let (status, guidance) = manager.get_token_budget_status_and_guidance(200_000);
    assert_eq!(status, TokenBudgetStatus::Critical);
    assert!(guidance.contains("CRITICAL"));

    // Test high threshold
    manager.cached_stats.total_token_usage = 175_000;
    let (status, guidance) = manager.get_token_budget_status_and_guidance(200_000);
    assert_eq!(status, TokenBudgetStatus::High);
    assert!(guidance.contains("HIGH"));

    // Test warning threshold
    manager.cached_stats.total_token_usage = 145_000;
    let (status, guidance) = manager.get_token_budget_status_and_guidance(200_000);
    assert_eq!(status, TokenBudgetStatus::Warning);
    assert!(guidance.contains("WARNING"));

    // Test normal
    manager.cached_stats.total_token_usage = 50_000;
    let (status, guidance) = manager.get_token_budget_status_and_guidance(200_000);
    assert_eq!(status, TokenBudgetStatus::Normal);
    assert!(guidance.is_empty());
}

#[test]
fn test_update_token_usage_prefers_prompt_pressure() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

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
    }));
    assert_eq!(manager.current_token_usage(), 2500);
}

#[test]
fn test_update_token_usage_falls_back_when_prompt_missing() {
    let mut manager = ContextManager::new(
        "sys".into(),
        (),
        Arc::new(RwLock::new(HashMap::new())),
        None,
    );

    manager.update_token_usage(&Some(uni::Usage {
        prompt_tokens: 0,
        completion_tokens: 800,
        total_tokens: 3300,
        cached_prompt_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
    }));

    // Fallback estimate = total - completion.
    assert_eq!(manager.current_token_usage(), 2500);
}
