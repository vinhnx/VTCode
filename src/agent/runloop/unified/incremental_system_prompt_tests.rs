use super::*;

fn test_context() -> SystemPromptContext {
    SystemPromptContext {
        full_auto: false,
        planning_active: false,
        request_user_input_enabled: true,
        discovered_skills: Vec::new(),
        active_instruction_directory: None,
        instruction_context_paths: Vec::new(),
    }
}

#[tokio::test]
async fn interactive_planning_contract_survives_tool_density_replacements() {
    use vtcode_core::config::types::{ResolvedShellPromptProfile, ShellPromptProfile, SystemPromptMode};
    use vtcode_core::config::{
        VTCodeConfig,
        constants::{models, tools},
    };
    use vtcode_core::core::agent::harness_kernel::SessionToolCatalogSnapshot;
    use vtcode_core::llm::{provider::ToolDefinition, providers::OpenAIProvider};
    use vtcode_core::prompts::system::*;
    use vtcode_core::prompts::{PromptContext, append_runtime_tool_prompt_sections_for_model};

    let workspace = tempfile::TempDir::new().expect("workspace");
    let provider = OpenAIProvider::new("offline-fixture".into());
    for mode in [
        SystemPromptMode::Default,
        SystemPromptMode::Minimal,
        SystemPromptMode::Lightweight,
        SystemPromptMode::Specialized,
    ] {
        for environment in [false, true] {
            for request_user_input_enabled in [false, true] {
                let mut names = vec![tools::EXEC_COMMAND, tools::CODE_SEARCH, tools::TASK_TRACKER];
                if request_user_input_enabled {
                    names.push(tools::REQUEST_USER_INPUT);
                }
                let snapshot = SessionToolCatalogSnapshot::new(
                    7,
                    9,
                    true,
                    request_user_input_enabled,
                    Some(Arc::new(
                        names
                            .iter()
                            .map(|name| {
                                ToolDefinition::function(
                                    (*name).to_string(),
                                    "Fixture tool".to_string(),
                                    serde_json::json!({"type": "object"}),
                                )
                            })
                            .collect(),
                    )),
                    false,
                );
                let mut config = VTCodeConfig::default();
                config.agent.system_prompt_mode = mode;
                config.agent.include_temporal_context = false;
                config.agent.include_working_directory = environment;
                config.agent.instruction_max_bytes = 0;
                config.agent.shell_prompt_profile = ShellPromptProfile::UnixLike;
                let mut context = PromptContext {
                    available_tools: names.iter().map(|name| (*name).to_string()).collect(),
                    ..Default::default()
                };
                context.set_current_directory(PathBuf::from("/workspace"));
                let base = compose_system_instruction_text(workspace.path(), Some(&config), Some(&context)).await;
                let mut runtime_context = test_context();
                runtime_context.planning_active = true;
                runtime_context.request_user_input_enabled = request_user_input_enabled;
                let builder = IncrementalSystemPrompt::new();
                let initial = builder
                    .get_system_prompt(
                        &base,
                        hash_base_system_prompt(&base),
                        runtime_context.hash(),
                        &runtime_context,
                        None,
                    )
                    .await;
                for budget in [100_000, 1] {
                    config.agent.max_system_prompt_tokens = budget;
                    let mut prompt = initial.clone();
                    let replace = |prompt: &mut String| {
                        append_runtime_tool_prompt_sections_for_model(
                            prompt,
                            &snapshot,
                            true,
                            ResolvedShellPromptProfile::UnixLike,
                            &provider,
                            models::openai::DEFAULT_MODEL,
                            Some(&config),
                        )
                    };
                    replace(&mut prompt);
                    let once = prompt.clone();
                    replace(&mut prompt);
                    assert_eq!(
                        prompt, once,
                        "{mode:?}, environment={environment}, input={request_user_input_enabled}, budget={budget}"
                    );
                    for line in [
                        PLANNING_WORKFLOW_PLAN_PERSISTENCE_POLICY_LINE,
                        PLANNING_WORKFLOW_PLAN_QUALITY_LINE,
                        PLANNING_WORKFLOW_RESEARCH_SCOPE_LINE,
                        PLANNING_WORKFLOW_PLAN_POLICY_LINE,
                    ] {
                        assert_eq!(prompt.matches(line).count(), 1, "missing or repeated canonical planning line");
                    }
                    assert_eq!(prompt.matches("## Active Tools").count(), 1);
                    assert_eq!(prompt.matches("## Environment").count(), usize::from(environment));
                    assert_eq!(
                        prompt.contains(PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE),
                        !request_user_input_enabled
                    );
                    for duplicate in [
                        "Monitor the available planning tool-loop budget",
                        "Every implementation step in the final plan must",
                        "emit only one `<proposed_plan>` block",
                        "Stop research when the plan is specified or the budget is near",
                    ] {
                        assert!(!prompt.contains(duplicate), "repeated planning guidance: {duplicate}");
                    }
                    assert!(prompt.contains("index 0 is invalid while planning"));
                    assert!(prompt.contains("omit unused filters"));
                    assert_eq!(prompt.contains("- Planning is read-only."), budget == 1);
                }
            }
        }
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
    assert_eq!(hash_base_system_prompt("Test"), hash_base_system_prompt("Test"));
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
        .get_system_prompt("Stable base prompt", 1, context.hash(), &context, Some(&config))
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
        .get_system_prompt("You are a helpful assistant.", 1, context.hash(), &context, Some(&agent_config))
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
async fn test_planning_workflow_notice_appended() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let context = SystemPromptContext { planning_active: true, ..test_context() };

    let prompt = prompt_builder
        .get_system_prompt("You are a helpful assistant.", 1, context.hash(), &context, None)
        .await;

    assert!(prompt.contains(vtcode_core::prompts::system::PLANNING_WORKFLOW_READ_ONLY_HEADER));
    assert!(prompt.contains(vtcode_core::prompts::system::PLANNING_WORKFLOW_EXIT_INSTRUCTION_LINE));
    assert!(prompt.contains(vtcode_core::prompts::system::PLANNING_WORKFLOW_PLAN_QUALITY_LINE));
    assert!(prompt.contains("<proposed_plan>"));
    assert!(prompt.contains("Next open decision"));
    assert!(!prompt.contains("Scope checkpoint"));
    assert!(prompt.contains(vtcode_core::prompts::system::PLANNING_WORKFLOW_NO_AUTO_EXIT_LINE));
    assert!(prompt.contains(vtcode_core::prompts::system::PLANNING_WORKFLOW_TASK_TRACKER_LINE));
    assert!(!prompt.contains("[Context]"));
}

#[tokio::test]
async fn test_planning_workflow_uses_plan_notice_unconditionally() {
    for request_user_input_enabled in [true, false] {
        let prompt_builder = IncrementalSystemPrompt::new();
        let context = SystemPromptContext {
            planning_active: true,
            request_user_input_enabled,
            ..test_context()
        };

        let prompt = prompt_builder
            .get_system_prompt("You are a helpful assistant.", 1, context.hash(), &context, None)
            .await;

        assert!(prompt.contains(vtcode_core::prompts::system::PLANNING_WORKFLOW_PLAN_POLICY_LINE));
    }
}

#[tokio::test]
async fn test_full_auto_is_constrained_in_planning_workflow() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let context = SystemPromptContext {
        full_auto: true,
        planning_active: true,
        ..test_context()
    };

    let prompt = prompt_builder
        .get_system_prompt("You are a helpful assistant.", 1, context.hash(), &context, None)
        .await;

    assert!(
        prompt.contains("# FULL-AUTO (PLANNING WORKFLOW): Work autonomously within planning workflow constraints.")
    );
    assert!(!prompt.contains("# FULL-AUTO: Complete task autonomously until done or blocked."));
}

#[tokio::test]
async fn test_mode_changes_invalidate_cached_prompt() {
    let prompt_builder = IncrementalSystemPrompt::new();
    let base_prompt = "Base prompt";
    let base_context = test_context();
    let full_auto_context = SystemPromptContext { full_auto: true, ..test_context() };

    let base = prompt_builder
        .get_system_prompt(base_prompt, 1, base_context.hash(), &base_context, None)
        .await;
    let full_auto = prompt_builder
        .get_system_prompt(base_prompt, 1, full_auto_context.hash(), &full_auto_context, None)
        .await;

    assert_ne!(base, full_auto);
    assert!(!base.contains("# FULL-AUTO:"));
    assert!(full_auto.contains("# FULL-AUTO: Complete task autonomously until done or blocked."));
}
