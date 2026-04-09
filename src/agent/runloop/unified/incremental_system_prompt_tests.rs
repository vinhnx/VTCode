use super::*;

fn test_context() -> SystemPromptContext {
    SystemPromptContext {
        full_auto: false,
        auto_mode: false,
        plan_mode: false,
        discovered_skills: Vec::new(),
        active_instruction_directory: None,
        instruction_context_paths: Vec::new(),
    }
}

#[tokio::test]
async fn test_incremental_prompt_caching() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "Test system prompt";
    let context = test_context();

    let prompt1 = prompt_builder
        .get_system_prompt(base_prompt, 1, context.hash(), &context, None)
        .await;
    let prompt2 = prompt_builder
        .get_system_prompt(base_prompt, 1, context.hash(), &context, None)
        .await;

    assert_eq!(prompt1, prompt2);
    assert!(prompt1.contains("Test system prompt"));
    assert!(!prompt1.contains("[Context]"));
    assert!(!prompt1.contains("[Runtime Context]"));

    let (is_cached, size) = prompt_builder.cache_stats().await;
    assert!(is_cached);
    assert!(size >= base_prompt.len());
}

#[tokio::test]
async fn test_base_prompt_hash_is_stable() {
    assert_eq!(
        hash_base_system_prompt("Test"),
        hash_base_system_prompt("Test")
    );
}

#[tokio::test]
async fn test_instruction_appendix_uses_explicit_directory() {
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
        include_temporal_context: true,
        temporal_context_use_utc: true,
        ..Default::default()
    };

    let context = SystemPromptContext {
        active_instruction_directory: Some(nested.clone()),
        instruction_context_paths: vec![nested.join("file.rs")],
        ..test_context()
    };

    let prompt = prompt_builder
        .get_system_prompt(
            "Stable base prompt",
            1,
            context.hash(),
            &context,
            Some(&config),
        )
        .await;

    assert!(prompt.contains("be brief"));
    assert!(prompt.contains("### Instruction map"));
    assert!(prompt.contains("AGENTS.md (workspace AGENTS)"));
    assert!(prompt.contains("nested/sub/AGENTS.md (workspace AGENTS)"));
    assert!(prompt.contains("root rule"));
    assert!(prompt.contains("nested rule"));
    assert!(!prompt.contains("[Runtime Context]"));
    assert!(!prompt.contains("Time (UTC):"));
}

#[tokio::test]
async fn test_prompt_omits_runtime_context_sections() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let agent_config = vtcode_config::core::AgentConfig {
        include_temporal_context: true,
        temporal_context_use_utc: true,
        ..Default::default()
    };
    let context = test_context();
    let prompt = prompt_builder
        .get_system_prompt(
            "You are a helpful assistant.",
            1,
            context.hash(),
            &context,
            Some(&agent_config),
        )
        .await;

    assert!(!prompt.contains("[Context]"));
    assert!(!prompt.contains("[Runtime Context]"));
    assert!(!prompt.contains("Retry #"));
    assert!(!prompt.contains("task_tracker"));
    assert!(!prompt.contains("Time (UTC):"));
    assert!(!prompt.contains("<budget:token_budget>"));
    assert!(!prompt.contains("<system_warning>"));
    assert!(!prompt.contains("token_usage:"));
}

#[tokio::test]
async fn test_plan_mode_notice_appended() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let context = SystemPromptContext {
        plan_mode: true,
        ..test_context()
    };

    let prompt = prompt_builder
        .get_system_prompt(
            "You are a helpful assistant.",
            1,
            context.hash(),
            &context,
            None,
        )
        .await;

    assert!(prompt.contains(vtcode_core::prompts::system::PLAN_MODE_READ_ONLY_HEADER));
    assert!(prompt.contains(vtcode_core::prompts::system::PLAN_MODE_EXIT_INSTRUCTION_LINE));
    assert!(prompt.contains(vtcode_core::prompts::system::PLAN_MODE_PLAN_QUALITY_LINE));
    assert!(prompt.contains("<proposed_plan>"));
    assert!(prompt.contains("Next open decision"));
    assert!(!prompt.contains("Scope checkpoint"));
    assert!(prompt.contains(vtcode_core::prompts::system::PLAN_MODE_NO_AUTO_EXIT_LINE));
    assert!(prompt.contains(vtcode_core::prompts::system::PLAN_MODE_TASK_TRACKER_LINE));
    assert!(!prompt.contains("[Context]"));
}

#[tokio::test]
async fn test_full_auto_is_constrained_in_plan_mode() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let context = SystemPromptContext {
        full_auto: true,
        plan_mode: true,
        ..test_context()
    };

    let prompt = prompt_builder
        .get_system_prompt(
            "You are a helpful assistant.",
            1,
            context.hash(),
            &context,
            None,
        )
        .await;

    assert!(
        prompt.contains("# FULL-AUTO (PLAN MODE): Work autonomously within Plan Mode constraints.")
    );
    assert!(!prompt.contains("# FULL-AUTO: Complete task autonomously until done or blocked."));
}

#[tokio::test]
async fn test_mode_changes_invalidate_cached_prompt() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "Base prompt";
    let base_context = test_context();
    let full_auto_context = SystemPromptContext {
        full_auto: true,
        ..test_context()
    };

    let base = prompt_builder
        .get_system_prompt(base_prompt, 1, base_context.hash(), &base_context, None)
        .await;
    let full_auto = prompt_builder
        .get_system_prompt(
            base_prompt,
            1,
            full_auto_context.hash(),
            &full_auto_context,
            None,
        )
        .await;

    assert_ne!(base, full_auto);
    assert!(!base.contains("# FULL-AUTO:"));
    assert!(full_auto.contains("# FULL-AUTO: Complete task autonomously until done or blocked."));
}
