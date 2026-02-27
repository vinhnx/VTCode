use super::*;

#[tokio::test]
async fn test_incremental_prompt_caching() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "Test system prompt";
    let context = SystemPromptContext {
        conversation_length: 2,
        tool_usage_count: 1,
        error_count: 0,
        token_usage_ratio: 0.0,
        full_auto: false,
        plan_mode: false,
        active_agent_name: String::new(),
        active_agent_prompt: None,
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
    };

    // First call - should build from scratch (includes context section)
    let prompt1 = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            1,
            0,
            PromptAssemblyMode::AppendInstructions,
            &context,
            None,
        )
        .await;
    assert!(prompt1.contains("Test system prompt"));
    assert!(prompt1.contains("[Context]"));

    // Second call with same parameters - should use cache
    let prompt2 = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            1,
            0,
            PromptAssemblyMode::AppendInstructions,
            &context,
            None,
        )
        .await;
    assert_eq!(prompt1, prompt2);

    // Verify cache stats
    let (is_cached, size) = prompt_builder.cache_stats().await;
    assert!(is_cached);
    assert!(size > base_prompt.len());
}

#[tokio::test]
async fn test_incremental_prompt_rebuild() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "Test system prompt";
    let context = SystemPromptContext {
        conversation_length: 1,
        tool_usage_count: 0,
        error_count: 0,
        token_usage_ratio: 0.0,
        full_auto: false,
        plan_mode: false,
        active_agent_name: String::new(),
        active_agent_prompt: None,
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
    };
    // Build initial prompt
    let _ = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            1,
            0,
            PromptAssemblyMode::AppendInstructions,
            &context,
            None,
        )
        .await;

    // Rebuild with different retry attempts
    let prompt = prompt_builder
        .rebuild_prompt(
            base_prompt,
            1,
            1,
            1,
            PromptAssemblyMode::AppendInstructions,
            &context,
            None,
        )
        .await;

    assert!(prompt.contains("Retry #1"));
    assert!(prompt.contains("task_tracker"));
}

#[tokio::test]
async fn test_prompt_config_hash() {
    let config1 = SystemPromptConfig::new(
        "Test",
        true,
        false,
        3,
        PromptAssemblyMode::AppendInstructions,
    );
    let config2 = SystemPromptConfig::new(
        "Test",
        true,
        false,
        3,
        PromptAssemblyMode::AppendInstructions,
    );

    assert_eq!(config1.hash(), config2.hash());
}

#[tokio::test]
async fn test_context_awareness_token_budget_warning() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "You are a helpful assistant.";
    let context = SystemPromptContext {
        conversation_length: 50,
        tool_usage_count: 20,
        error_count: 1,
        token_usage_ratio: 0.65,
        full_auto: false,
        plan_mode: false,
        active_agent_name: String::new(),
        active_agent_prompt: None,
        discovered_skills: Vec::new(),
        context_window_size: Some(200_000),
        current_token_usage: Some(130_000),
        supports_context_awareness: true,
        token_budget_guidance: "WARNING: Update progress docs to preserve context.",
    };

    let prompt = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            1,
            0,
            PromptAssemblyMode::AppendInstructions,
            &context,
            None,
        )
        .await;

    assert!(prompt.contains("<budget:token_budget>200000</budget:token_budget>"));
    assert!(prompt.contains("Token usage: 130000/200000; 70000 remaining"));
    assert!(prompt.contains("WARNING: Update progress docs"));
}

#[tokio::test]
async fn test_context_awareness_token_budget_high() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "You are a helpful assistant.";
    let context = SystemPromptContext {
        conversation_length: 80,
        tool_usage_count: 35,
        error_count: 2,
        token_usage_ratio: 0.88,
        full_auto: true,
        plan_mode: false,
        active_agent_name: String::new(),
        active_agent_prompt: None,
        discovered_skills: Vec::new(),
        context_window_size: Some(200_000),
        current_token_usage: Some(176_000),
        supports_context_awareness: true,
        token_budget_guidance: "HIGH: Summarize key findings and prepare a handoff.",
    };

    let prompt = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            1,
            0,
            PromptAssemblyMode::AppendInstructions,
            &context,
            None,
        )
        .await;

    assert!(prompt.contains("<budget:token_budget>200000</budget:token_budget>"));
    assert!(prompt.contains("Token usage: 176000/200000; 24000 remaining"));
    assert!(prompt.contains("HIGH: Summarize key findings"));
}

#[tokio::test]
async fn test_context_awareness_token_budget_critical() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "You are a helpful assistant.";
    let context = SystemPromptContext {
        conversation_length: 120,
        tool_usage_count: 50,
        error_count: 3,
        token_usage_ratio: 0.95,
        full_auto: false,
        plan_mode: false,
        active_agent_name: String::new(),
        active_agent_prompt: None,
        discovered_skills: Vec::new(),
        context_window_size: Some(200_000),
        current_token_usage: Some(190_000),
        supports_context_awareness: true,
        token_budget_guidance: "CRITICAL: Update artifacts and consider a new session.",
    };

    let prompt = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            1,
            0,
            PromptAssemblyMode::AppendInstructions,
            &context,
            None,
        )
        .await;

    assert!(prompt.contains("<budget:token_budget>200000</budget:token_budget>"));
    assert!(prompt.contains("Token usage: 190000/200000; 10000 remaining"));
    assert!(prompt.contains("CRITICAL: Update artifacts"));
}

#[tokio::test]
async fn test_context_awareness_normal_no_guidance() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "You are a helpful assistant.";
    let context = SystemPromptContext {
        conversation_length: 10,
        tool_usage_count: 5,
        error_count: 0,
        token_usage_ratio: 0.10,
        full_auto: false,
        plan_mode: false,
        active_agent_name: String::new(),
        active_agent_prompt: None,
        discovered_skills: Vec::new(),
        context_window_size: Some(200_000),
        current_token_usage: Some(20_000),
        supports_context_awareness: true,
        token_budget_guidance: "",
    };

    let prompt = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            1,
            0,
            PromptAssemblyMode::AppendInstructions,
            &context,
            None,
        )
        .await;

    assert!(prompt.contains("<budget:token_budget>200000</budget:token_budget>"));
    assert!(prompt.contains("Token usage: 20000/200000; 180000 remaining"));
    assert!(
        !prompt.contains("WARNING:") && !prompt.contains("HIGH:") && !prompt.contains("CRITICAL:")
    );
}

#[tokio::test]
async fn test_non_context_aware_model_no_budget_tags() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "You are a helpful assistant.";
    let context = SystemPromptContext {
        conversation_length: 10,
        tool_usage_count: 5,
        error_count: 0,
        token_usage_ratio: 0.10,
        full_auto: false,
        plan_mode: false,
        active_agent_name: String::new(),
        active_agent_prompt: None,
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
    };

    let prompt = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            1,
            0,
            PromptAssemblyMode::AppendInstructions,
            &context,
            None,
        )
        .await;

    assert!(!prompt.contains("<budget:token_budget>"));
    assert!(!prompt.contains("<system_warning>"));
}

#[tokio::test]
async fn test_plan_mode_notice_appended_with_active_agent_prompt() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "You are a helpful assistant.";
    let context = SystemPromptContext {
        conversation_length: 0,
        tool_usage_count: 0,
        error_count: 0,
        token_usage_ratio: 0.0,
        full_auto: false,
        plan_mode: true,
        active_agent_name: "planner".to_string(),
        active_agent_prompt: Some("Custom planner agent prompt.".to_string()),
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
    };

    let prompt = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            1,
            0,
            PromptAssemblyMode::AppendInstructions,
            &context,
            None,
        )
        .await;

    assert!(prompt.contains("Custom planner agent prompt."));
    assert!(prompt.contains(vtcode_core::prompts::system::PLAN_MODE_READ_ONLY_HEADER));
    assert!(prompt.contains(vtcode_core::prompts::system::PLAN_MODE_EXIT_INSTRUCTION_LINE));
}

#[tokio::test]
async fn test_full_auto_is_constrained_in_plan_mode() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "You are a helpful assistant.";
    let context = SystemPromptContext {
        conversation_length: 0,
        tool_usage_count: 0,
        error_count: 0,
        token_usage_ratio: 0.0,
        full_auto: true,
        plan_mode: true,
        active_agent_name: "planner".to_string(),
        active_agent_prompt: None,
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
    };

    let prompt = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            1,
            0,
            PromptAssemblyMode::AppendInstructions,
            &context,
            None,
        )
        .await;

    assert!(
        prompt.contains("# FULL-AUTO (PLAN MODE): Work autonomously within Plan Mode constraints.")
    );
    assert!(!prompt.contains("# FULL-AUTO: Complete task autonomously until done or blocked."));
}
