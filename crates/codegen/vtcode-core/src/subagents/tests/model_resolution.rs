use super::*;

#[test]
fn request_prompt_prefers_message() {
    let request = SpawnAgentRequest {
        message: Some("hello".to_string()),
        ..SpawnAgentRequest::default()
    };
    assert_eq!(request_prompt(request.message.as_deref(), &request.items).as_deref(), Some("hello"));
}
#[test]
fn delegated_task_requires_clarification_for_vague_prompt() {
    assert!(delegated_task_requires_clarification("report"));
    assert!(delegated_task_requires_clarification("report findings"));
    assert!(!delegated_task_requires_clarification("review current code changes"));
}
#[test]
fn resolve_subagent_model_maps_aliases() {
    let cfg = VTCodeConfig::default();
    let resolved =
        resolve_subagent_model(&cfg, models::anthropic::CLAUDE_SONNET_5, "anthropic", Some("haiku"), "explorer")
            .expect("resolve model");
    assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_SONNET_5);
}
#[test]
fn resolve_subagent_model_defaults_to_parent_when_omitted() {
    let cfg = VTCodeConfig::default();
    let resolved = resolve_subagent_model(&cfg, models::ollama::GPT_OSS_120B_CLOUD, "ollama", None, "worker")
        .expect("resolve model");
    assert_eq!(resolved.as_str(), models::ollama::GPT_OSS_120B_CLOUD);
}
#[test]
fn resolve_subagent_model_accepts_dotted_claude_aliases_for_anthropic() {
    let cfg = VTCodeConfig::default();
    let resolved =
        resolve_subagent_model(&cfg, "claude-haiku-4.5", "anthropic", None, "worker").expect("resolve model");
    assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_SONNET_5);
}
#[test]
fn resolve_subagent_model_falls_back_to_copilot_default_for_unsupported_inherit_model() {
    let cfg = VTCodeConfig::default();
    let resolved = resolve_subagent_model(&cfg, "claude-haiku-4.5", "copilot", None, "worker").expect("resolve model");
    assert_eq!(resolved, ModelId::default_orchestrator_for_provider(Provider::Copilot));
}
#[test]
fn resolve_effective_subagent_model_uses_explicit_inherit_override() {
    let cfg = VTCodeConfig::default();
    let resolved = resolve_effective_subagent_model(
        &cfg,
        models::anthropic::CLAUDE_SONNET_5,
        "anthropic",
        Some("inherit"),
        Some("haiku"),
        "worker",
    )
    .expect("resolve model");
    assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_SONNET_5);
}
#[test]
fn resolve_effective_subagent_model_falls_back_to_parent_on_invalid_override() {
    // For non-local providers, an unrecognized override must fall back to the
    // parent model rather than being accepted as a custom identifier.
    let cfg = VTCodeConfig::default();
    let resolved = resolve_effective_subagent_model(
        &cfg,
        models::openai::GPT_5_6_SOL,
        "openai",
        Some("not-a-real-model"),
        None,
        "rust-engineer",
    )
    .expect("resolve model");
    assert_eq!(resolved.as_str(), models::openai::GPT_5_6_SOL);
}
#[test]
fn resolve_subagent_model_inherits_local_custom_model() {
    // Local providers expose arbitrary model IDs not in the built-in catalog;
    // inheriting such a model must succeed as a custom identifier.
    let cfg = VTCodeConfig::default();
    let resolved = resolve_subagent_model(&cfg, "qwen3.5-9b-sushi-coder-rl", "lmstudio", None, "wiki-assistant")
        .expect("resolve local inherit model");
    assert_eq!(resolved.as_str(), "qwen3.5-9b-sushi-coder-rl");
    assert_eq!(resolved.provider(), Provider::LmStudio);
}
#[test]
fn resolve_subagent_model_honors_explicit_local_model() {
    let cfg = VTCodeConfig::default();
    let resolved =
        resolve_subagent_model(&cfg, "qwen3.5-9b-sushi-coder-rl", "lmstudio", Some("ornith-1.0-9b"), "wiki-assistant")
            .expect("resolve explicit local model");
    assert_eq!(resolved.as_str(), "ornith-1.0-9b");
    assert_eq!(resolved.provider(), Provider::LmStudio);
}
#[test]
fn resolve_subagent_model_honors_provider_override_model() {
    use vtcode_config::core::ProviderOverrideConfig;

    let mut cfg = VTCodeConfig::default();
    cfg.provider_overrides.insert(
        "openai".to_string(),
        ProviderOverrideConfig {
            models: vec!["my-fine-tuned-gpt".to_string()],
            ..ProviderOverrideConfig::default()
        },
    );
    let resolved =
        resolve_subagent_model(&cfg, models::openai::GPT_5_6_SOL, "openai", Some("my-fine-tuned-gpt"), "reviewer")
            .expect("resolve override model");
    assert_eq!(resolved.as_str(), "my-fine-tuned-gpt");
}
#[test]
fn resolve_effective_subagent_model_ignores_cross_provider_override_model() {
    use vtcode_config::core::ProviderOverrideConfig;

    // An override belonging to a DIFFERENT provider must not be accepted for the
    // active provider; resolution must fall back to the parent model instead.
    let mut cfg = VTCodeConfig::default();
    cfg.provider_overrides.insert(
        "anthropic".to_string(),
        ProviderOverrideConfig {
            models: vec!["not-a-real-model".to_string()],
            ..ProviderOverrideConfig::default()
        },
    );
    let resolved = resolve_effective_subagent_model(
        &cfg,
        models::openai::GPT_5_6_SOL,
        "openai",
        Some("not-a-real-model"),
        None,
        "reviewer",
    )
    .expect("resolve model");
    assert_eq!(resolved.as_str(), models::openai::GPT_5_6_SOL);
}
#[test]
fn resolve_subagent_model_honors_custom_provider_model() {
    use vtcode_config::core::CustomProviderConfig;

    let mut cfg = VTCodeConfig::default();
    cfg.custom_providers.push(CustomProviderConfig {
        name: "mycorp".to_string(),
        display_name: "MyCorp".to_string(),
        base_url: "https://llm.corp.example/v1".to_string(),
        model: "mycorp-special-coder".to_string(),
        ..CustomProviderConfig::default()
    });
    let resolved = resolve_subagent_model(&cfg, "mycorp-special-coder", "mycorp", None, "wiki-assistant")
        .expect("resolve custom provider model");
    assert_eq!(resolved.as_str(), "mycorp-special-coder");
}
#[test]
fn resolve_subagent_small_model_rejects_cross_provider_configured_lightweight_model() {
    let mut cfg = VTCodeConfig::default();
    cfg.agent.small_model.model = models::anthropic::CLAUDE_SONNET_5.to_string();

    let resolved = resolve_subagent_model(&cfg, models::openai::GPT_5_6_SOL, "openai", Some("small"), "worker")
        .expect("resolve model");

    assert_eq!(resolved, ModelId::GPT56Terra);
}
#[test]
fn resolve_effective_subagent_model_falls_back_to_spec_model_on_invalid_override() {
    let cfg = VTCodeConfig::default();
    let resolved = resolve_effective_subagent_model(
        &cfg,
        models::anthropic::CLAUDE_SONNET_5,
        "anthropic",
        Some("not-a-real-model"),
        Some("haiku"),
        "reviewer",
    )
    .expect("resolve model");
    assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_SONNET_5);
}
#[test]
fn background_record_ids_are_stable_and_sanitized() {
    assert_eq!(background_record_id("Rust Engineer"), "background-Rust-Engineer");
    assert_eq!(background_record_id("plugin:reviewer/default"), "background-plugin-reviewer-default");
}
#[test]
fn background_subagent_command_includes_expected_flags() {
    let workspace = std::env::current_dir().expect("workspace");
    let command = build_background_subagent_command(
        &workspace,
        "rust-engineer",
        "session-parent",
        "session-child",
        "Inspect the repo",
        Some(7),
        Some("gpt-5.6-luna"),
        Some("high"),
    )
    .expect("background command");

    assert!(command.len() >= 15);
    assert_eq!(command[1], "background-subagent");
    assert!(command.windows(2).any(|pair| pair == ["--agent-name", "rust-engineer"]));
    assert!(command.windows(2).any(|pair| pair == ["--parent-session-id", "session-parent"]));
    assert!(command.windows(2).any(|pair| pair == ["--session-id", "session-child"]));
    assert!(command.windows(2).any(|pair| pair == ["--prompt", "Inspect the repo"]));
    assert!(command.windows(2).any(|pair| pair == ["--max-turns", "7"]));
    assert!(command.windows(2).any(|pair| pair == ["--model-override", "gpt-5.6-luna"]));
    assert!(command.windows(2).any(|pair| pair == ["--reasoning-override", "high"]));
}
#[test]
fn resolve_effective_subagent_model_still_errors_on_invalid_spec_model() {
    let cfg = VTCodeConfig::default();
    let err = resolve_effective_subagent_model(
        &cfg,
        models::anthropic::CLAUDE_SONNET_5,
        "anthropic",
        None,
        Some("not-a-real-model"),
        "reviewer",
    )
    .expect_err("invalid spec model should fail");
    assert!(err.to_string().contains("Failed to resolve model"));
}
#[test]
fn background_launch_finalization_cannot_resurrect_terminal_or_replaced_record() {
    let spec = read_only_test_spec("background-demo");
    let started_at = Utc::now();
    let updated_at = started_at + chrono::Duration::seconds(1);

    let mut starting =
        test_background_record(&spec, "background-demo", BackgroundSubprocessStatus::Starting, true, "exec-current");
    starting.pid = None;
    starting.started_at = None;
    assert!(finalize_background_launch(&mut starting, "exec-current", Some(99), Some(started_at), updated_at,));
    assert_eq!(starting.status, BackgroundSubprocessStatus::Running);
    assert_eq!(starting.pid, Some(99));

    let mut terminal =
        test_background_record(&spec, "background-demo", BackgroundSubprocessStatus::Stopped, false, "exec-current");
    terminal.summary = Some("Background subprocess completed successfully".to_string());
    assert!(!finalize_background_launch(&mut terminal, "exec-current", Some(100), Some(started_at), updated_at,));
    assert_eq!(terminal.status, BackgroundSubprocessStatus::Stopped);
    assert_eq!(terminal.pid, Some(42));
    assert_eq!(terminal.summary.as_deref(), Some("Background subprocess completed successfully"));

    let mut replaced = test_background_record(
        &spec,
        "background-demo",
        BackgroundSubprocessStatus::Starting,
        true,
        "exec-replacement",
    );
    assert!(!finalize_background_launch(&mut replaced, "exec-stale", Some(101), Some(started_at), updated_at,));
    assert_eq!(replaced.exec_session_id, "exec-replacement");
    assert_eq!(replaced.pid, Some(42));
}
