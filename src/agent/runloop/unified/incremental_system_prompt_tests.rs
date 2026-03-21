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
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
        editor_context_block: None,
        active_instruction_directory: None,
    };

    // First call - should build from scratch (includes context section)
    let prompt1 = prompt_builder
        .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
        .await;
    assert!(prompt1.contains("Test system prompt"));
    assert!(prompt1.contains("[Context]"));

    // Second call with same parameters - should use cache
    let prompt2 = prompt_builder
        .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
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
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
        editor_context_block: None,
        active_instruction_directory: None,
    };
    // Build initial prompt
    let _ = prompt_builder
        .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
        .await;

    // Rebuild with different retry attempts
    let prompt = prompt_builder
        .rebuild_prompt(base_prompt, 1, 1, 1, &context, None)
        .await;

    assert!(prompt.contains("Retry #1"));
    assert!(prompt.contains("task_tracker"));
}

#[tokio::test]
async fn test_prompt_config_hash() {
    let config1 = SystemPromptConfig::new("Test", true, false, 3);
    let config2 = SystemPromptConfig::new("Test", true, false, 3);

    assert_eq!(config1.hash(), config2.hash());
}

#[tokio::test]
async fn test_cache_friendly_mode_moves_volatile_context_to_runtime_section() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "Stable base prompt";

    let context_a = SystemPromptContext {
        conversation_length: 2,
        tool_usage_count: 1,
        error_count: 0,
        token_usage_ratio: 0.12,
        full_auto: false,
        plan_mode: false,
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
        prompt_cache_shaping_mode: PromptCacheShapingMode::TrailingRuntimeContext,
        editor_context_block: None,
        active_instruction_directory: None,
    };
    let context_b = SystemPromptContext {
        conversation_length: 14,
        tool_usage_count: 7,
        error_count: 2,
        token_usage_ratio: 0.71,
        full_auto: false,
        plan_mode: false,
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
        prompt_cache_shaping_mode: PromptCacheShapingMode::TrailingRuntimeContext,
        editor_context_block: None,
        active_instruction_directory: None,
    };

    let prompt_a = prompt_builder
        .get_system_prompt(base_prompt, 1, context_a.hash(), 0, &context_a, None)
        .await;
    let prompt_b = prompt_builder
        .get_system_prompt(base_prompt, 1, context_b.hash(), 0, &context_b, None)
        .await;

    let marker = "\n[Runtime Context]\n";
    let prefix_a = prompt_a.split(marker).next().unwrap_or("");
    let prefix_b = prompt_b.split(marker).next().unwrap_or("");

    assert!(prompt_a.contains("[Runtime Context]"));
    assert!(prompt_b.contains("[Runtime Context]"));
    assert_eq!(prefix_a, prefix_b);
}

#[tokio::test]
async fn test_instruction_appendix_uses_explicit_directory_and_precedes_runtime_context() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let workspace = tempfile::TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join(".git"), "gitdir: /tmp/git").expect("write git");
    std::fs::write(workspace.path().join("AGENTS.md"), "root rule").expect("write root");
    let nested = workspace.path().join("nested/sub");
    std::fs::create_dir_all(&nested).expect("create nested");
    std::fs::write(nested.join("AGENTS.md"), "nested rule").expect("write nested");

    let config = vtcode_config::core::AgentConfig {
        user_instructions: Some("be brief".to_string()),
        instruction_max_bytes: 4096,
        ..Default::default()
    };

    let context = SystemPromptContext {
        conversation_length: 3,
        tool_usage_count: 1,
        error_count: 0,
        token_usage_ratio: 0.1,
        full_auto: false,
        plan_mode: false,
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
        prompt_cache_shaping_mode: PromptCacheShapingMode::TrailingRuntimeContext,
        editor_context_block: None,
        active_instruction_directory: Some(nested.clone()),
    };

    let prompt = prompt_builder
        .get_system_prompt(
            "Stable base prompt",
            1,
            context.hash(),
            0,
            &context,
            Some(&config),
        )
        .await;

    let instructions_pos = prompt.find("# INSTRUCTIONS").expect("instructions");
    let runtime_pos = prompt.find("[Runtime Context]").expect("runtime context");
    assert!(instructions_pos < runtime_pos);
    assert!(prompt.contains("be brief"));
    assert!(prompt.contains("[AGENTS.md]"));
    assert!(prompt.contains("[nested/sub/AGENTS.md]"));
    assert!(prompt.contains("root rule"));
    assert!(prompt.contains("nested rule"));
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
        discovered_skills: Vec::new(),
        context_window_size: Some(200_000),
        current_token_usage: Some(130_000),
        supports_context_awareness: true,
        token_budget_guidance: "WARNING: Update progress docs to preserve context.",
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
        editor_context_block: None,
        active_instruction_directory: None,
    };

    let prompt = prompt_builder
        .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
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
        discovered_skills: Vec::new(),
        context_window_size: Some(200_000),
        current_token_usage: Some(176_000),
        supports_context_awareness: true,
        token_budget_guidance: "HIGH: Summarize key findings and prepare a handoff.",
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
        editor_context_block: None,
        active_instruction_directory: None,
    };

    let prompt = prompt_builder
        .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
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
        discovered_skills: Vec::new(),
        context_window_size: Some(200_000),
        current_token_usage: Some(190_000),
        supports_context_awareness: true,
        token_budget_guidance: "CRITICAL: Update artifacts and consider a new session.",
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
        editor_context_block: None,
        active_instruction_directory: None,
    };

    let prompt = prompt_builder
        .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
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
        discovered_skills: Vec::new(),
        context_window_size: Some(200_000),
        current_token_usage: Some(20_000),
        supports_context_awareness: true,
        token_budget_guidance: "",
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
        editor_context_block: None,
        active_instruction_directory: None,
    };

    let prompt = prompt_builder
        .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
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
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
        editor_context_block: None,
        active_instruction_directory: None,
    };

    let prompt = prompt_builder
        .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
        .await;

    assert!(!prompt.contains("<budget:token_budget>"));
    assert!(!prompt.contains("<system_warning>"));
}

#[tokio::test]
async fn test_non_context_aware_model_with_context_window_no_budget_tags() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "You are a helpful assistant.";
    let context = SystemPromptContext {
        conversation_length: 10,
        tool_usage_count: 5,
        error_count: 0,
        token_usage_ratio: 0.10,
        full_auto: false,
        plan_mode: false,
        discovered_skills: Vec::new(),
        context_window_size: Some(1_000_000),
        current_token_usage: Some(250_000),
        supports_context_awareness: false,
        token_budget_guidance: "WARNING: should not be shown",
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
        editor_context_block: None,
        active_instruction_directory: None,
    };

    let prompt = prompt_builder
        .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
        .await;

    assert!(!prompt.contains("<budget:token_budget>1000000</budget:token_budget>"));
    assert!(!prompt.contains("Token usage: 250000/1000000; 750000 remaining"));
    assert!(!prompt.contains("should not be shown"));
}

#[tokio::test]
async fn test_plan_mode_notice_appended() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "You are a helpful assistant.";
    let context = SystemPromptContext {
        conversation_length: 0,
        tool_usage_count: 0,
        error_count: 0,
        token_usage_ratio: 0.0,
        full_auto: false,
        plan_mode: true,
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
        editor_context_block: None,
        active_instruction_directory: None,
    };

    let prompt = prompt_builder
        .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
        .await;

    assert!(prompt.contains(vtcode_core::prompts::system::PLAN_MODE_READ_ONLY_HEADER));
    assert!(prompt.contains(vtcode_core::prompts::system::PLAN_MODE_EXIT_INSTRUCTION_LINE));
    assert!(prompt.contains(vtcode_core::prompts::system::PLAN_MODE_PLAN_QUALITY_LINE));
    assert!(prompt.contains("<proposed_plan>"));
    assert!(prompt.contains("Next open decision"));
    assert!(!prompt.contains("Scope checkpoint"));
    assert!(prompt.contains(vtcode_core::prompts::system::PLAN_MODE_NO_AUTO_EXIT_LINE));
    assert!(prompt.contains(vtcode_core::prompts::system::PLAN_MODE_TASK_TRACKER_LINE));
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
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
        editor_context_block: None,
        active_instruction_directory: None,
    };

    let prompt = prompt_builder
        .get_system_prompt(base_prompt, 1, 1, 0, &context, None)
        .await;

    assert!(
        prompt.contains("# FULL-AUTO (PLAN MODE): Work autonomously within Plan Mode constraints.")
    );
    assert!(!prompt.contains("# FULL-AUTO: Complete task autonomously until done or blocked."));
}

#[tokio::test]
async fn test_editor_context_block_is_appended() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let context = SystemPromptContext {
        conversation_length: 1,
        tool_usage_count: 0,
        error_count: 0,
        token_usage_ratio: 0.0,
        full_auto: false,
        plan_mode: false,
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
        editor_context_block: Some(
            "## Active Editor Context\n- Active file: src/main.rs\n- Language: Rust".to_string(),
        ),
        active_instruction_directory: None,
    };

    let prompt = prompt_builder
        .get_system_prompt("Base prompt", 1, context.hash(), 0, &context, None)
        .await;

    assert!(prompt.contains("## Active Editor Context"));
    assert!(prompt.contains("- Active file: src/main.rs"));
}

#[tokio::test]
async fn test_editor_context_changes_invalidate_cached_prompt() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "Base prompt";
    let context_without_editor = SystemPromptContext {
        conversation_length: 1,
        tool_usage_count: 0,
        error_count: 0,
        token_usage_ratio: 0.0,
        full_auto: false,
        plan_mode: false,
        discovered_skills: Vec::new(),
        context_window_size: None,
        current_token_usage: None,
        supports_context_awareness: false,
        token_budget_guidance: "",
        prompt_cache_shaping_mode: PromptCacheShapingMode::Disabled,
        editor_context_block: None,
        active_instruction_directory: None,
    };
    let context_with_editor = SystemPromptContext {
        editor_context_block: Some(
            "## Active Editor Context\n- Active file: src/lib.rs\n- Language: Rust".to_string(),
        ),
        ..context_without_editor.clone()
    };

    let prompt_without_editor = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            context_without_editor.hash(),
            0,
            &context_without_editor,
            None,
        )
        .await;
    let prompt_with_editor = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            context_with_editor.hash(),
            0,
            &context_with_editor,
            None,
        )
        .await;

    assert_ne!(prompt_without_editor, prompt_with_editor);
    assert!(!prompt_without_editor.contains("## Active Editor Context"));
    assert!(prompt_with_editor.contains("## Active Editor Context"));
}
