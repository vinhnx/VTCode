use super::*;
use crate::config::constants::models;
use crate::config::constants::tools;
use crate::config::models::{ModelId, Provider};
use crate::llm::provider::ToolDefinition;
use crate::tools::exec_session::ExecSessionManager;
use crate::tools::registry::PtySessionManager;
use anyhow::{Result, anyhow};
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::Notify;
use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
use vtcode_config::{
    HookCommandConfig, HookGroupConfig, HooksConfig, SubagentMcpServer, SubagentMemoryScope,
    SubagentSource, SubagentSpec,
};

fn readonly_agent_permissions() -> AgentPermissionsConfig {
    let mut permissions = AgentPermissionsConfig::new(PermissionDefault::Deny);
    permissions.allow = vec![tools::READ_FILE.to_string()];
    permissions
}

fn test_controller_config(
    workspace_root: PathBuf,
    vt_cfg: VTCodeConfig,
) -> SubagentControllerConfig {
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), vt_cfg.pty.clone());
    let exec_sessions = ExecSessionManager::new(workspace_root.clone(), pty_sessions.clone());
    SubagentControllerConfig {
        workspace_root,
        parent_session_id: "parent-session".to_string(),
        parent_model: models::openai::GPT_5_4.to_string(),
        parent_provider: "openai".to_string(),
        parent_reasoning_effort: ReasoningEffortLevel::Medium,
        api_key: "test-key".to_string(),
        vt_cfg,
        openai_chatgpt_auth: None,
        depth: 0,
        exec_sessions,
        pty_manager: pty_sessions.manager().clone(),
        managed_background_runtime: false,
    }
}

fn test_child_record(
    id: &str,
    parent_thread_id: &str,
    spec: &SubagentSpec,
    status: SubagentStatus,
    depth: usize,
) -> ChildRecord {
    ChildRecord {
        id: id.to_string(),
        session_id: format!("session-{id}"),
        parent_thread_id: parent_thread_id.to_string(),
        spec: spec.clone(),
        display_label: subagent_display_label(spec),
        status,
        background: false,
        depth,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        completed_at: status.is_terminal().then_some(Utc::now()),
        summary: None,
        error: None,
        archive_metadata: None,
        archive_path: None,
        transcript_path: None,
        effective_config: Some(VTCodeConfig::default()),
        stored_messages: Vec::new(),
        last_prompt: Some(format!("prompt-{id}")),
        queued_prompts: VecDeque::new(),
        max_turns: None,
        model_override: None,
        reasoning_override: None,
        thread_handle: None,
        handle: None,
        notify: Arc::new(Notify::new()),
        worktree_path: None,
    }
}

fn write_test_background_subagent(workspace_root: &std::path::Path) {
    let agent_dir = workspace_root.join(".vtcode/agents");
    std::fs::create_dir_all(&agent_dir).expect("agent dir");
    std::fs::write(
        agent_dir.join("background-demo.md"),
        r#"---
name: background-demo
description: Minimal demo agent for the managed background subprocess flow.
tools:
  - command_session
background: true
maxTurns: 2
initialPrompt: Report readiness once.
---

Run the managed background demo.
"#,
    )
    .expect("write background agent");
}

fn write_test_primary_agent(workspace_root: &std::path::Path) {
    let agent_dir = workspace_root.join(".vtcode/agents");
    std::fs::create_dir_all(&agent_dir).expect("agent dir");
    std::fs::write(
        agent_dir.join("duck.md"),
        r#"---
name: duck
description: Discussion controller.
mode: primary
permissions:
  default: ask
---

Discuss before implementation.
"#,
    )
    .expect("write primary agent");
}

fn write_test_read_only_subagent(workspace_root: &std::path::Path) {
    let agent_dir = workspace_root.join(".vtcode/agents");
    std::fs::create_dir_all(&agent_dir).expect("agent dir");
    std::fs::write(
        agent_dir.join("readonly-demo.md"),
        r#"---
name: readonly-demo
description: Read-only test child agent.
tools:
  - code_search
permissions:
  default: ask
---

Inspect the repository.
"#,
    )
    .expect("write read-only agent");
}

#[test]
fn request_prompt_prefers_message() {
    let request = SpawnAgentRequest {
        message: Some("hello".to_string()),
        ..SpawnAgentRequest::default()
    };
    assert_eq!(
        request_prompt(&request.message, &request.items).as_deref(),
        Some("hello")
    );
}

#[test]
fn delegated_task_requires_clarification_for_vague_prompt() {
    assert!(delegated_task_requires_clarification("report"));
    assert!(delegated_task_requires_clarification("report findings"));
    assert!(!delegated_task_requires_clarification(
        "review current code changes"
    ));
}

#[test]
fn resolve_subagent_model_maps_aliases() {
    let cfg = VTCodeConfig::default();
    let resolved = resolve_subagent_model(
        &cfg,
        models::anthropic::CLAUDE_SONNET_4_6,
        "anthropic",
        Some("haiku"),
        "explorer",
    )
    .expect("resolve model");
    assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_HAIKU_4_5);
}

#[test]
fn resolve_subagent_model_defaults_to_parent_when_omitted() {
    let cfg = VTCodeConfig::default();
    let resolved = resolve_subagent_model(
        &cfg,
        models::ollama::GPT_OSS_120B_CLOUD,
        "ollama",
        None,
        "worker",
    )
    .expect("resolve model");
    assert_eq!(resolved.as_str(), models::ollama::GPT_OSS_120B_CLOUD);
}

#[test]
fn resolve_subagent_model_accepts_dotted_claude_aliases_for_anthropic() {
    let cfg = VTCodeConfig::default();
    let resolved = resolve_subagent_model(&cfg, "claude-haiku-4.5", "anthropic", None, "worker")
        .expect("resolve model");
    assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_HAIKU_4_5);
}

#[test]
fn resolve_subagent_model_falls_back_to_copilot_default_for_unsupported_inherit_model() {
    let cfg = VTCodeConfig::default();
    let resolved = resolve_subagent_model(&cfg, "claude-haiku-4.5", "copilot", None, "worker")
        .expect("resolve model");
    assert_eq!(
        resolved,
        ModelId::default_orchestrator_for_provider(Provider::Copilot)
    );
}

#[test]
fn resolve_effective_subagent_model_uses_explicit_inherit_override() {
    let cfg = VTCodeConfig::default();
    let resolved = resolve_effective_subagent_model(
        &cfg,
        models::anthropic::CLAUDE_SONNET_4_6,
        "anthropic",
        Some("inherit"),
        Some("haiku"),
        "worker",
    )
    .expect("resolve model");
    assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_SONNET_4_6);
}

#[test]
fn resolve_effective_subagent_model_falls_back_to_parent_on_invalid_override() {
    // For non-local providers, an unrecognized override must fall back to the
    // parent model rather than being accepted as a custom identifier.
    let cfg = VTCodeConfig::default();
    let resolved = resolve_effective_subagent_model(
        &cfg,
        models::openai::GPT_5_4,
        "openai",
        Some("not-a-real-model"),
        None,
        "rust-engineer",
    )
    .expect("resolve model");
    assert_eq!(resolved.as_str(), models::openai::GPT_5_4);
}

#[test]
fn resolve_subagent_model_inherits_local_custom_model() {
    // Local providers expose arbitrary model IDs not in the built-in catalog;
    // inheriting such a model must succeed as a custom identifier.
    let cfg = VTCodeConfig::default();
    let resolved = resolve_subagent_model(
        &cfg,
        "qwen3.5-9b-sushi-coder-rl",
        "lmstudio",
        None,
        "wiki-assistant",
    )
    .expect("resolve local inherit model");
    assert_eq!(resolved.as_str(), "qwen3.5-9b-sushi-coder-rl");
    assert_eq!(resolved.provider(), Provider::LmStudio);
}

#[test]
fn resolve_subagent_model_honors_explicit_local_model() {
    let cfg = VTCodeConfig::default();
    let resolved = resolve_subagent_model(
        &cfg,
        "qwen3.5-9b-sushi-coder-rl",
        "lmstudio",
        Some("ornith-1.0-9b"),
        "wiki-assistant",
    )
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
    let resolved = resolve_subagent_model(
        &cfg,
        models::openai::GPT_5_4,
        "openai",
        Some("my-fine-tuned-gpt"),
        "reviewer",
    )
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
        models::openai::GPT_5_4,
        "openai",
        Some("not-a-real-model"),
        None,
        "reviewer",
    )
    .expect("resolve model");
    assert_eq!(resolved.as_str(), models::openai::GPT_5_4);
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
    let resolved = resolve_subagent_model(
        &cfg,
        "mycorp-special-coder",
        "mycorp",
        None,
        "wiki-assistant",
    )
    .expect("resolve custom provider model");
    assert_eq!(resolved.as_str(), "mycorp-special-coder");
}

#[test]
fn resolve_subagent_small_model_rejects_cross_provider_configured_lightweight_model() {
    let mut cfg = VTCodeConfig::default();
    cfg.agent.small_model.model = models::anthropic::CLAUDE_HAIKU_4_5.to_string();

    let resolved = resolve_subagent_model(
        &cfg,
        models::openai::GPT_5_4,
        "openai",
        Some("small"),
        "worker",
    )
    .expect("resolve model");

    assert_eq!(resolved, ModelId::GPT54Mini);
}

#[test]
fn resolve_effective_subagent_model_falls_back_to_spec_model_on_invalid_override() {
    let cfg = VTCodeConfig::default();
    let resolved = resolve_effective_subagent_model(
        &cfg,
        models::anthropic::CLAUDE_SONNET_4_6,
        "anthropic",
        Some("not-a-real-model"),
        Some("haiku"),
        "reviewer",
    )
    .expect("resolve model");
    assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_HAIKU_4_5);
}

#[test]
fn background_record_ids_are_stable_and_sanitized() {
    assert_eq!(
        background_record_id("Rust Engineer"),
        "background-Rust-Engineer"
    );
    assert_eq!(
        background_record_id("plugin:reviewer/default"),
        "background-plugin-reviewer-default"
    );
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
        Some("gpt-5.4-mini"),
        Some("high"),
    )
    .expect("background command");

    assert!(command.len() >= 15);
    assert_eq!(command[1], "background-subagent");
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--agent-name", "rust-engineer"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--parent-session-id", "session-parent"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--session-id", "session-child"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--prompt", "Inspect the repo"])
    );
    assert!(command.windows(2).any(|pair| pair == ["--max-turns", "7"]));
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--model-override", "gpt-5.4-mini"])
    );
    assert!(
        command
            .windows(2)
            .any(|pair| pair == ["--reasoning-override", "high"])
    );
}

#[test]
fn resolve_effective_subagent_model_still_errors_on_invalid_spec_model() {
    let cfg = VTCodeConfig::default();
    let err = resolve_effective_subagent_model(
        &cfg,
        models::anthropic::CLAUDE_SONNET_4_6,
        "anthropic",
        None,
        Some("not-a-real-model"),
        "reviewer",
    )
    .expect_err("invalid spec model should fail");
    assert!(err.to_string().contains("Failed to resolve model"));
}

async fn wait_for_effective_model(controller: &SubagentController, target: &str) -> Result<String> {
    for _ in 0..50 {
        if let Ok(snapshot) = controller.snapshot_for_thread(target).await {
            return Ok(snapshot.effective_config.agent.default_model);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    Err(anyhow!(
        "Subagent {target} did not capture an effective runtime configuration in time"
    ))
}

fn read_only_test_spec(name: &str) -> SubagentSpec {
    SubagentSpec {
        name: name.to_string(),
        description: "test".to_string(),
        prompt: String::new(),
        tools: Some(vec![tools::READ_FILE.to_string()]),
        disallowed_tools: Vec::new(),
        model: None,
        color: None,
        reasoning_effort: None,
        permissions: readonly_agent_permissions(),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: vtcode_config::AgentMode::Subagent,
        max_turns: None,
        nickname_candidates: Vec::new(),
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: Vec::new(),
        source: SubagentSource::Builtin,
        file_path: None,
        warnings: Vec::new(),
        tool_policy_overrides: BTreeMap::new(),
    }
}

#[test]
fn filter_child_tools_keeps_public_read_tools_and_removes_mutation_tools() {
    let defs = vec![
        ToolDefinition::function(
            tools::SPAWN_AGENT.to_string(),
            "Spawn".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::CODE_SEARCH.to_string(),
            "Search".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::EXEC_COMMAND.to_string(),
            "Exec".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::APPLY_PATCH.to_string(),
            "Patch".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::WRITE_STDIN.to_string(),
            "Continue".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::REQUEST_USER_INPUT.to_string(),
            "Ask".to_string(),
            serde_json::json!({"type": "object"}),
        ),
    ];
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");
    let filtered = filter_child_tools(&spec, defs, true);
    let names = filtered
        .iter()
        .map(ToolDefinition::function_name)
        .collect::<Vec<_>>();
    assert_eq!(names, vec![tools::CODE_SEARCH]);
}

#[test]
fn filter_child_tools_keeps_command_session_for_shell_capable_agents() {
    let defs = vec![
        ToolDefinition::function(
            tools::UNIFIED_EXEC.to_string(),
            "Exec".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::CODE_SEARCH.to_string(),
            "Search".to_string(),
            serde_json::json!({"type": "object"}),
        ),
    ];
    let spec = SubagentSpec {
        name: "shell-demo".to_string(),
        description: "test".to_string(),
        prompt: String::new(),
        tools: Some(vec![
            tools::UNIFIED_EXEC.to_string(),
            tools::CODE_SEARCH.to_string(),
        ]),
        disallowed_tools: Vec::new(),
        model: None,
        color: None,
        reasoning_effort: None,
        permissions: AgentPermissionsConfig::new(PermissionDefault::Ask),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: vtcode_config::AgentMode::Subagent,
        max_turns: None,
        nickname_candidates: Vec::new(),
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: Vec::new(),
        source: SubagentSource::Builtin,
        file_path: None,
        warnings: Vec::new(),
        tool_policy_overrides: BTreeMap::new(),
    };

    let filtered = filter_child_tools(&spec, defs, spec.is_read_only());
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].function_name(), tools::UNIFIED_EXEC);
    assert_eq!(filtered[1].function_name(), tools::CODE_SEARCH);
}

#[test]
fn build_child_config_intersects_allowed_tools_and_preserves_global_denies() {
    let mut parent = VTCodeConfig::default();
    parent.permissions.allow = vec![tools::READ_FILE.to_string(), tools::CODE_SEARCH.to_string()];
    parent.permissions.deny = vec![tools::UNIFIED_EXEC.to_string()];

    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.permissions = AgentPermissionsConfig {
        auto: vec!["Bash(*)".to_string()],
        ..AgentPermissionsConfig::new(PermissionDefault::Auto)
    };
    spec.tools = Some(vec![
        tools::SPAWN_AGENT.to_string(),
        tools::CODE_SEARCH.to_string(),
        tools::READ_FILE.to_string(),
    ]);

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);
    assert_eq!(
        child.runtime_agent_permissions.as_ref(),
        Some(&spec.permissions)
    );
    assert_eq!(
        child.permissions.allow,
        vec![tools::READ_FILE.to_string(), tools::CODE_SEARCH.to_string()]
    );
    assert!(
        child
            .permissions
            .deny
            .contains(&tools::UNIFIED_EXEC.to_string())
    );
    assert!(
        child
            .permissions
            .deny
            .contains(&tools::SPAWN_AGENT.to_string())
    );
}

#[test]
fn build_child_config_preserves_subagent_lifecycle_stripping_and_hook_merging() {
    let mut parent = VTCodeConfig::default();
    parent.permissions.allow = vec![
        tools::SPAWN_AGENT.to_string(),
        tools::CODE_SEARCH.to_string(),
        tools::UNIFIED_EXEC.to_string(),
    ];

    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.tools = Some(parent.permissions.allow.clone());
    let mut hooks = HooksConfig::default();
    hooks.lifecycle.pre_tool_use.push(HookGroupConfig {
        matcher: Some("*".to_string()),
        hooks: vec![HookCommandConfig {
            command: "echo child".to_string(),
            ..HookCommandConfig::default()
        }],
    });
    spec.hooks = Some(hooks);

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);

    assert_eq!(
        child.permissions.allow,
        vec![
            tools::CODE_SEARCH.to_string(),
            tools::UNIFIED_EXEC.to_string()
        ]
    );
    assert!(
        child
            .permissions
            .deny
            .contains(&tools::SPAWN_AGENT.to_string())
    );
    assert_eq!(child.hooks.lifecycle.pre_tool_use.len(), 1);
    assert_eq!(
        child.hooks.lifecycle.pre_tool_use[0].hooks[0].command,
        "echo child"
    );
}

#[test]
fn prepare_child_runtime_config_uses_shared_view_for_model_and_reasoning() {
    let parent = VTCodeConfig::default();
    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.model = Some(models::openai::GPT_5_4_MINI.to_string());
    spec.reasoning_effort = Some(ReasoningEffortLevel::High);

    let (resolved_model, child_reasoning_effort, child_cfg) = prepare_child_runtime_config(
        &parent,
        &spec,
        models::openai::GPT_5_4,
        "openai",
        ReasoningEffortLevel::Low,
        None,
        None,
        None,
        |_, parent_model, parent_provider, model_override, spec_model, agent_name| {
            assert_eq!(parent_model, models::openai::GPT_5_4);
            assert_eq!(parent_provider, "openai");
            assert_eq!(model_override, None);
            assert_eq!(spec_model, Some(models::openai::GPT_5_4_MINI));
            assert_eq!(agent_name, "worker");
            Ok(models::openai::GPT_5_4_MINI
                .parse::<ModelId>()
                .expect("valid model"))
        },
    )
    .expect("prepared child runtime config");

    assert_eq!(resolved_model.as_str(), models::openai::GPT_5_4_MINI);
    assert_eq!(child_cfg.agent.default_model, models::openai::GPT_5_4_MINI);
    assert_eq!(child_reasoning_effort, ReasoningEffortLevel::High);
    assert_eq!(child_cfg.agent.reasoning_effort, ReasoningEffortLevel::High);
}

#[test]
fn subagent_instruction_composition_uses_shared_runtime_prompt_and_skill_appendix() {
    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.prompt = "Worker instructions".to_string();
    spec.skills = vec!["rust".to_string(), "repo".to_string()];

    let instructions = compose_subagent_instructions(&spec, Some("Memory appendix".to_string()));

    assert!(instructions.contains("Worker instructions"));
    assert!(instructions.contains("Preloaded skill names: rust, repo."));
    assert!(instructions.contains("Memory appendix"));
    assert!(instructions.contains("Return your final response using this exact Markdown contract"));
}

#[test]
fn build_child_config_preserves_matching_rule_and_exact_tool_ids() {
    let mut parent = VTCodeConfig::default();
    parent.permissions.allow = vec![
        "Read(/docs/**)".to_string(),
        "mcp::context7::search".to_string(),
        tools::READ_FILE.to_string(),
    ];

    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.tools = Some(vec![
        "mcp::context7::search".to_string(),
        tools::UNIFIED_EXEC.to_string(),
        tools::READ_FILE.to_string(),
    ]);

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);

    assert_eq!(
        child.permissions.allow,
        vec![
            "Read(/docs/**)".to_string(),
            "mcp::context7::search".to_string(),
            tools::READ_FILE.to_string()
        ]
    );
}

#[test]
fn build_child_config_preserves_parent_rule_shaped_allowlist() {
    let mut parent = VTCodeConfig::default();
    parent.permissions.allow = vec!["Read".to_string()];

    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.tools = Some(vec![
        tools::READ_FILE.to_string(),
        tools::CODE_SEARCH.to_string(),
        tools::UNIFIED_EXEC.to_string(),
    ]);

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);

    assert_eq!(child.permissions.allow, vec!["Read".to_string()]);
}

#[test]
fn build_child_config_promotes_single_turn_budget_to_recovery_budget() {
    let parent = VTCodeConfig::default();
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, Some(1));

    assert_eq!(child.automation.full_auto.max_turns, SUBAGENT_MIN_MAX_TURNS);
}

#[test]
fn background_children_get_a_higher_turn_floor() {
    assert_eq!(normalize_background_child_max_turns(Some(2), true), Some(4));
    assert_eq!(normalize_background_child_max_turns(Some(3), true), Some(4));
    assert_eq!(normalize_background_child_max_turns(Some(4), true), Some(4));
}

#[test]
fn foreground_children_keep_the_existing_turn_floor() {
    assert_eq!(
        normalize_background_child_max_turns(Some(1), false),
        Some(SUBAGENT_MIN_MAX_TURNS)
    );
    assert_eq!(
        normalize_background_child_max_turns(Some(2), false),
        Some(2)
    );
    assert_eq!(normalize_background_child_max_turns(None, true), None);
}

#[test]
fn build_child_config_merges_inline_mcp_provider() {
    let parent = VTCodeConfig::default();
    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "default")
        .expect("default");
    spec.mcp_servers = vec![SubagentMcpServer::Inline(BTreeMap::from([(
        "playwright".to_string(),
        serde_json::json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "@playwright/mcp@latest"],
        }),
    )]))];

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);
    let provider = child
        .mcp
        .providers
        .iter()
        .find(|provider| provider.name == "playwright")
        .expect("playwright provider");
    assert_eq!(provider.name, "playwright");
}

#[test]
fn explicit_delegation_request_detects_mentions_and_keywords() {
    let direct_mentions = extract_explicit_agent_mentions("@agent-worker fix the issue", &[]);
    assert!(contains_explicit_delegation_request(
        "@agent-worker fix the issue",
        direct_mentions.as_slice()
    ));
    let no_mentions = extract_explicit_agent_mentions("delegate this in parallel", &[]);
    assert!(contains_explicit_delegation_request(
        "delegate this in parallel",
        no_mentions.as_slice()
    ));
    let empty_mentions = extract_explicit_agent_mentions("review the repository", &[]);
    assert!(!contains_explicit_delegation_request(
        "review the repository",
        empty_mentions.as_slice()
    ));
}

#[test]
fn explicit_agent_mentions_detect_natural_language_selection() {
    let rust_engineer = read_only_test_spec("rust-engineer");
    assert_eq!(
        extract_explicit_agent_mentions(
            "use rust-engineer agent to review current code",
            &[rust_engineer]
        ),
        vec!["rust-engineer".to_string()]
    );
}

#[test]
fn explicit_agent_mentions_detect_looser_subagent_selection() {
    let background_demo = read_only_test_spec("background-demo");
    assert_eq!(
        extract_explicit_agent_mentions(
            "use background-demo and run the subagent",
            &[background_demo]
        ),
        vec!["background-demo".to_string()]
    );
}

#[test]
fn explicit_agent_mentions_detect_run_subagent_selection() {
    let rust_engineer = read_only_test_spec("rust-engineer");
    assert_eq!(
        extract_explicit_agent_mentions(
            "run rust-engineer subagent and review changes",
            &[rust_engineer]
        ),
        vec!["rust-engineer".to_string()]
    );
}

#[test]
fn explicit_agent_mentions_ignore_primary_only_agents() {
    let mut duck = read_only_test_spec("duck");
    duck.mode = vtcode_config::AgentMode::Primary;

    assert_eq!(
        extract_explicit_agent_mentions("@agent-duck discuss the task", &[duck.clone()]),
        Vec::<String>::new()
    );
    assert_eq!(
        extract_explicit_agent_mentions("run duck agent and discuss the task", &[duck]),
        Vec::<String>::new()
    );
}

#[test]
fn explicit_model_request_detects_aliases_and_full_ids() {
    assert!(contains_explicit_model_request(
        "delegate this using gpt-5.4-mini",
        "gpt-5.4-mini"
    ));
    assert!(contains_explicit_model_request(
        "use the worker subagent with haiku",
        "haiku"
    ));
    assert!(contains_explicit_model_request(
        "run this with the small model",
        "small"
    ));
    assert!(!contains_explicit_model_request(
        "delegate this small cleanup task",
        "small"
    ));
    assert!(!contains_explicit_model_request(
        "delegate this task",
        "gpt-5.4-mini"
    ));
}

#[test]
fn normalize_requested_model_override_drops_default_like_values() {
    assert_eq!(
        normalize_requested_model_override(Some("default".to_string()), "delegate this task"),
        None
    );
    assert_eq!(
        normalize_requested_model_override(Some(" inherit ".to_string()), "delegate this task"),
        None
    );
    assert_eq!(
        normalize_requested_model_override(
            Some(" inherit ".to_string()),
            "delegate this task using inherit"
        ),
        Some("inherit".to_string())
    );
}

#[test]
fn sanitize_subagent_input_items_drops_empty_fields() {
    let mut items = vec![
        SubagentInputItem {
            item_type: Some("text".to_string()),
            text: Some("  Workspace: /tmp/repo  ".to_string()),
            path: Some(String::new()),
            name: Some(" ".to_string()),
            image_url: None,
        },
        SubagentInputItem {
            item_type: Some("text".to_string()),
            text: Some("   ".to_string()),
            path: Some(String::new()),
            name: None,
            image_url: None,
        },
    ];

    sanitize_subagent_input_items(&mut items);

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].text.as_deref(), Some("Workspace: /tmp/repo"));
    assert!(items[0].path.is_none());
    assert!(items[0].name.is_none());
}

#[tokio::test]
async fn controller_exposes_builtin_specs() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");
    let specs = controller.effective_specs().await;
    assert!(specs.iter().any(|spec| spec.name == "explorer"));
    assert!(specs.iter().any(|spec| spec.name == "worker"));
}

#[tokio::test]
async fn spawn_defaults_to_single_explicit_mention() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("@agent-explorer inspect the codebase")
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            message: Some("Inspect the codebase.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    assert_eq!(spawned.agent_name, "explorer");
    controller.close(&spawned.id).await.expect("close");
}

#[tokio::test]
async fn spawn_defaults_to_single_natural_language_selection() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    let mentions = controller
        .set_turn_delegation_hints_from_input("use explorer agent to inspect the codebase")
        .await;
    assert_eq!(mentions, vec!["explorer".to_string()]);

    let spawned = controller
        .spawn(SpawnAgentRequest {
            message: Some("Inspect the codebase.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    assert_eq!(spawned.agent_name, "explorer");
    controller.close(&spawned.id).await.expect("close");
}

#[tokio::test]
async fn spawn_rejects_mismatched_explicit_mention() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("@agent-explorer inspect the codebase")
        .await;

    let err = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement a change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("mismatched mention should fail");

    assert!(
        err.to_string()
            .contains("user explicitly selected 'explorer'")
    );
}

#[tokio::test]
async fn spawn_rejects_write_capable_agent_without_explicit_request_or_agent_type() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    let err = controller
        .spawn(SpawnAgentRequest {
            message: Some("Implement a change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("write-capable agent should require explicit request or agent_type");

    assert!(
        err.to_string()
            .contains("cannot launch write-capable agent")
    );
}

#[tokio::test]
async fn spawn_allows_write_capable_agent_with_explicit_agent_type() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement a change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("explicit agent_type should allow write-capable agent");

    controller.close(&spawned.id).await.expect("close");
}

#[tokio::test]
async fn spawn_rejects_primary_only_agent_as_child() {
    let temp = TempDir::new().expect("tempdir");
    write_test_primary_agent(temp.path());
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    let mentions = controller
        .set_turn_delegation_hints_from_input("@agent-duck discuss the task")
        .await;
    assert!(mentions.is_empty());

    let err = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("duck".to_string()),
            message: Some("Discuss the task.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("primary-only agent should not spawn as child");

    assert!(err.to_string().contains("Unknown subagent type duck"));
}

#[tokio::test]
async fn spawn_accepts_background_flag_outside_managed_background_runtime() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("delegate this task")
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("explorer".to_string()),
            message: Some("Inspect the codebase.".to_string()),
            background: true,
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("background child spawn should succeed");

    assert!(spawned.background);
    controller.close(&spawned.id).await.expect("close");
}

#[tokio::test]
async fn spawn_allows_background_capable_spec_as_foreground_child() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("run background-demo subagent and demo")
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("background-demo".to_string()),
            message: Some("Run the demo.".to_string()),
            background: false,
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("foreground background-capable spawn should succeed");

    assert_eq!(spawned.agent_name, "background-demo");
    assert!(!spawned.background);
    controller.close(&spawned.id).await.expect("close");
}

#[tokio::test]
async fn spawn_rejects_vague_task_even_with_explicit_request() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("run worker subagent and report")
        .await;

    let err = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("report".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("vague task should require clarification");

    assert!(err.to_string().contains("too vague ('report')"));
}

#[tokio::test]
async fn spawn_defaults_to_write_capable_run_subagent_selection() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    let mentions = controller
        .set_turn_delegation_hints_from_input("run worker subagent and implement the change")
        .await;
    assert_eq!(mentions, vec!["worker".to_string()]);

    let spawned = controller
        .spawn(SpawnAgentRequest {
            message: Some("Implement the change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    assert_eq!(spawned.agent_name, "worker");
    controller.close(&spawned.id).await.expect("close");
}

#[tokio::test]
async fn spawn_rejects_read_only_agent_when_auto_delegate_is_disabled() {
    let temp = TempDir::new().expect("tempdir");
    write_test_read_only_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.auto_delegate_read_only = false;
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
            .await
            .expect("controller");

    let err = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("readonly-demo".to_string()),
            message: Some("Inspect the repository.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("read-only agent should require explicit delegation");

    assert!(
        err.to_string()
            .contains("cannot proactively launch read-only agent 'readonly-demo'")
    );
}

#[test]
fn load_memory_appendix_renders_compact_summary() {
    let temp = TempDir::new().expect("tempdir");
    let memory_dir = temp.path().join(".vtcode/agent-memory/reviewer");
    std::fs::create_dir_all(&memory_dir).expect("memory dir");
    std::fs::write(
            memory_dir.join("MEMORY.md"),
            "# Reviewer Memory\n\n## Preferences\n- Keep diffs surgical.\n- Run focused tests before broad checks.\n- Prefer repo docs for orientation.\n- Ask only when a decision is materially blocked.\n- Additional long-form notes that should stay out of the prompt body.\n",
        )
        .expect("write memory");

    let appendix =
        load_memory_appendix(temp.path(), "reviewer", Some(SubagentMemoryScope::Project))
            .expect("appendix")
            .expect("memory appendix");

    assert!(appendix.contains("Persistent memory file:"));
    assert!(appendix.contains("Key points:"));
    assert!(appendix.contains("Keep diffs surgical."));
    assert!(appendix.contains("Open `MEMORY.md` when exact wording or more detail matters."));
    assert!(!appendix.contains("Current MEMORY.md excerpt"));
    assert!(!appendix.contains("## Preferences"));
}

#[test]
fn load_primary_memory_appendix_reads_existing_memory_without_write_guidance() {
    let temp = TempDir::new().expect("tempdir");
    let memory_dir = temp.path().join(".vtcode/agent-memory/reviewer");
    std::fs::create_dir_all(&memory_dir).expect("memory dir");
    std::fs::write(
            memory_dir.join("MEMORY.md"),
            "# Reviewer Memory\n\n## Preferences\n- Keep diffs surgical.\n- Run focused tests before broad checks.\n",
        )
        .expect("write memory");

    let appendix =
        load_primary_memory_appendix(temp.path(), "reviewer", Some(SubagentMemoryScope::Project))
            .expect("appendix")
            .expect("memory appendix");

    assert!(appendix.contains("Primary-agent memory file:"));
    assert!(appendix.contains("Loaded read-only for this request."));
    assert!(appendix.contains("Key points:"));
    assert!(appendix.contains("Keep diffs surgical."));
    assert!(!appendix.contains("Read and maintain `MEMORY.md`"));
    assert!(!appendix.contains("Create or update `MEMORY.md`"));
    assert!(!appendix.contains("Open `MEMORY.md` when exact wording or more detail matters."));
}

#[test]
fn load_primary_memory_appendix_missing_memory_is_noop_without_directory_creation() {
    let temp = TempDir::new().expect("tempdir");
    let memory_dir = temp.path().join(".vtcode/agent-memory/reviewer");

    let appendix =
        load_primary_memory_appendix(temp.path(), "reviewer", Some(SubagentMemoryScope::Project))
            .expect("appendix");

    assert!(appendix.is_none());
    assert!(!memory_dir.exists());
}

#[tokio::test]
async fn spawn_honors_model_override_when_user_explicitly_requests_it() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("delegate this task using gpt-5.4-mini")
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement the change.".to_string()),
            model: Some(models::openai::GPT_5_4_MINI.to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    let effective_model = wait_for_effective_model(&controller, &spawned.id)
        .await
        .expect("effective model");
    assert_eq!(effective_model, models::openai::GPT_5_4_MINI);
    controller.close(&spawned.id).await.expect("close");
}

#[tokio::test]
async fn spawn_background_subprocess_rejects_non_background_agent() {
    let temp = TempDir::new().expect("tempdir");
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
            .await
            .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("delegate this task")
        .await;

    let err = controller
        .spawn_background_subprocess(SpawnBackgroundSubprocessRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement a change.".to_string()),
            ..SpawnBackgroundSubprocessRequest::default()
        })
        .await
        .expect_err("non-background agent should be rejected");

    assert!(err.to_string().contains("background: true"));
    assert!(err.to_string().contains("Use spawn_agent instead"));
}

#[tokio::test]
async fn spawn_background_subprocess_returns_active_record_when_settings_match() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
            .await
            .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("delegate this task")
        .await;

    let spec = controller
        .resolve_requested_spec(Some("background-demo"))
        .await
        .expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let created_at = Utc::now();
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            BackgroundRecord {
                id: record_id.clone(),
                agent_name: spec.name.clone(),
                display_label: subagent_display_label(&spec),
                description: spec.description.clone(),
                source: spec.source.label(),
                color: spec.color.clone(),
                session_id: "session-background-demo".to_string(),
                exec_session_id: "exec-session-background-demo".to_string(),
                desired_enabled: true,
                status: BackgroundSubprocessStatus::Running,
                created_at,
                updated_at: created_at,
                started_at: Some(created_at),
                ended_at: None,
                pid: Some(42),
                prompt: "Report readiness once.".to_string(),
                summary: Some("ready".to_string()),
                error: None,
                archive_path: None,
                transcript_path: None,
                max_turns: Some(4),
                model_override: None,
                reasoning_override: None,
                restart_attempts: 0,
            },
        );
    }

    let entry = controller
        .spawn_background_subprocess(SpawnBackgroundSubprocessRequest {
            agent_type: Some("background-demo".to_string()),
            ..SpawnBackgroundSubprocessRequest::default()
        })
        .await
        .expect("matching active record should be returned");

    assert_eq!(entry.id, record_id);
    assert_eq!(entry.status, BackgroundSubprocessStatus::Running);
    assert_eq!(entry.pid, Some(42));
}

#[tokio::test]
async fn spawn_background_subprocess_rejects_conflicting_active_record_settings() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
            .await
            .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("delegate this task")
        .await;

    let spec = controller
        .resolve_requested_spec(Some("background-demo"))
        .await
        .expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let created_at = Utc::now();
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id,
            BackgroundRecord {
                id: background_record_id(spec.name.as_str()),
                agent_name: spec.name.clone(),
                display_label: subagent_display_label(&spec),
                description: spec.description.clone(),
                source: spec.source.label(),
                color: spec.color.clone(),
                session_id: "session-background-demo".to_string(),
                exec_session_id: "exec-session-background-demo".to_string(),
                desired_enabled: true,
                status: BackgroundSubprocessStatus::Running,
                created_at,
                updated_at: created_at,
                started_at: Some(created_at),
                ended_at: None,
                pid: Some(42),
                prompt: "Report readiness once.".to_string(),
                summary: Some("ready".to_string()),
                error: None,
                archive_path: None,
                transcript_path: None,
                max_turns: Some(4),
                model_override: None,
                reasoning_override: None,
                restart_attempts: 0,
            },
        );
    }

    let err = controller
        .spawn_background_subprocess(SpawnBackgroundSubprocessRequest {
            agent_type: Some("background-demo".to_string()),
            message: Some("Run a different task.".to_string()),
            ..SpawnBackgroundSubprocessRequest::default()
        })
        .await
        .expect_err("conflicting active record should be rejected");

    assert!(err.to_string().contains("different prompt"));
    assert!(err.to_string().contains("Stop or restart"));
}

#[tokio::test]
async fn resume_preserves_captured_runtime_overrides() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("delegate this task using gpt-5.4-mini")
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement the change.".to_string()),
            model: Some(models::openai::GPT_5_4_MINI.to_string()),
            max_turns: Some(2),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    let initial_model = wait_for_effective_model(&controller, &spawned.id)
        .await
        .expect("initial effective model");
    assert_eq!(initial_model, models::openai::GPT_5_4_MINI);

    let closed = controller.close(&spawned.id).await.expect("close");
    assert_eq!(closed.status, SubagentStatus::Closed);

    controller.resume(&spawned.id).await.expect("resume");

    for _ in 0..100 {
        let status = controller.status_for(&spawned.id).await.expect("status");
        if status.updated_at > closed.updated_at && status.status != SubagentStatus::Closed {
            let snapshot = controller
                .snapshot_for_thread(&spawned.id)
                .await
                .expect("snapshot");
            assert_eq!(
                snapshot.effective_config.agent.default_model,
                models::openai::GPT_5_4_MINI
            );
            controller.close(&spawned.id).await.expect("final close");
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("resumed subagent did not capture runtime config in time");
}

#[tokio::test]
async fn spawn_captures_runtime_config_before_first_child_turn() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("delegate this task")
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement the change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    let snapshot = controller
        .snapshot_for_thread(&spawned.id)
        .await
        .expect("snapshot");

    assert_eq!(snapshot.id, spawned.id);
    assert!(
        !snapshot
            .effective_config
            .agent
            .default_model
            .trim()
            .is_empty()
    );

    controller.close(&spawned.id).await.expect("close");
}

#[tokio::test]
async fn spawn_custom_uses_explicit_spec_without_delegation_hints() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");
    spec.name = "init-grounding-explorer".to_string();
    spec.description = "VT Code /init grounding explorer.".to_string();
    spec.source = SubagentSource::ProjectVtcode;

    let spawned = controller
        .spawn_custom(
            spec,
            SpawnAgentRequest {
                message: Some(
                    "Inspect the repository and report agent-facing findings.".to_string(),
                ),
                max_turns: Some(2),
                ..SpawnAgentRequest::default()
            },
        )
        .await
        .expect("spawn");

    assert_eq!(spawned.agent_name, "init-grounding-explorer");
    assert_eq!(spawned.source, SubagentSource::ProjectVtcode.label());
    controller.close(&spawned.id).await.expect("close");
}

#[tokio::test]
async fn spawn_custom_rejects_write_capable_spec() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");

    let err = controller
        .spawn_custom(
            spec,
            SpawnAgentRequest {
                message: Some("Implement a change.".to_string()),
                ..SpawnAgentRequest::default()
            },
        )
        .await
        .expect_err("write-capable custom spec should be rejected");

    assert!(
        err.to_string()
            .contains("custom subagent spawn only supports read-only specs")
    );
}

#[tokio::test]
async fn spawn_custom_rejects_primary_only_spec() {
    let temp = TempDir::new().expect("tempdir");
    write_test_primary_agent(temp.path());
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    let spec = controller
        .effective_specs()
        .await
        .into_iter()
        .find(|spec| spec.name == "duck")
        .expect("duck primary agent");

    let err = controller
        .spawn_custom(
            spec,
            SpawnAgentRequest {
                message: Some("Discuss the task.".to_string()),
                ..SpawnAgentRequest::default()
            },
        )
        .await
        .expect_err("primary-only custom spec should be rejected");

    assert!(
        err.to_string()
            .contains("custom subagent spawn only supports subagent-capable specs")
    );
}

#[tokio::test]
async fn close_marks_child_closed() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");
    controller
        .set_turn_delegation_hints_from_input("delegate this task")
        .await;
    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("default".to_string()),
            message: Some("Summarize the repository.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");
    let closed = controller.close(&spawned.id).await.expect("close");
    assert_eq!(closed.status, SubagentStatus::Closed);
}

#[tokio::test]
async fn close_is_idempotent_for_closed_agents() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");
    controller
        .set_turn_delegation_hints_from_input("delegate this task")
        .await;
    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("default".to_string()),
            message: Some("Summarize the repository.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    let closed = controller.close(&spawned.id).await.expect("first close");
    let closed_again = controller.close(&spawned.id).await.expect("second close");

    assert_eq!(closed_again.status, SubagentStatus::Closed);
    assert_eq!(closed_again.updated_at, closed.updated_at);
    assert_eq!(closed_again.completed_at, closed.completed_at);
}

#[tokio::test]
async fn close_and_resume_cascade_through_spawn_tree() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");

    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");

    {
        let mut state = controller.state.write().await;
        state.children.insert(
            "parent".to_string(),
            test_child_record("parent", "session-root", &spec, SubagentStatus::Running, 1),
        );
        state.children.insert(
            "child".to_string(),
            test_child_record("child", "parent", &spec, SubagentStatus::Running, 2),
        );
        state.children.insert(
            "grandchild".to_string(),
            test_child_record("grandchild", "child", &spec, SubagentStatus::Running, 3),
        );
    }

    let closed = controller.close("parent").await.expect("close");
    assert_eq!(closed.status, SubagentStatus::Closed);
    assert_eq!(
        controller.status_for("child").await.expect("child").status,
        SubagentStatus::Closed
    );
    assert_eq!(
        controller
            .status_for("grandchild")
            .await
            .expect("grandchild")
            .status,
        SubagentStatus::Closed
    );

    let subtree_ids = controller
        .collect_spawn_subtree_ids("parent")
        .await
        .expect("collect subtree");
    assert_eq!(
        subtree_ids,
        vec![
            "parent".to_string(),
            "child".to_string(),
            "grandchild".to_string()
        ]
    );

    let mut restart_ids = Vec::new();
    for node_id in subtree_ids {
        if controller
            .reopen_single(node_id.as_str())
            .await
            .expect("reopen subtree node")
        {
            restart_ids.push(node_id);
        }
    }

    assert_eq!(
        restart_ids,
        vec![
            "parent".to_string(),
            "child".to_string(),
            "grandchild".to_string()
        ]
    );
    assert_eq!(
        controller
            .status_for("parent")
            .await
            .expect("parent")
            .status,
        SubagentStatus::Queued
    );
    assert_eq!(
        controller.status_for("child").await.expect("child").status,
        SubagentStatus::Queued
    );
    assert_eq!(
        controller
            .status_for("grandchild")
            .await
            .expect("grandchild")
            .status,
        SubagentStatus::Queued
    );
}

#[tokio::test]
async fn spawn_rejects_fourth_active_subagent() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");
    controller
        .set_turn_delegation_hints_from_input("delegate this task")
        .await;

    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");

    {
        let mut state = controller.state.write().await;
        for idx in 0..SUBAGENT_HARD_CONCURRENCY_LIMIT {
            let id = format!("active-{idx}");
            state.children.insert(
                id.clone(),
                ChildRecord {
                    id: id.clone(),
                    session_id: format!("session-{id}"),
                    parent_thread_id: "parent-session".to_string(),
                    spec: spec.clone(),
                    display_label: subagent_display_label(&spec),
                    status: SubagentStatus::Running,
                    background: false,
                    depth: 1,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    completed_at: None,
                    summary: None,
                    error: None,
                    archive_metadata: None,
                    archive_path: None,
                    transcript_path: None,
                    effective_config: None,
                    stored_messages: Vec::new(),
                    last_prompt: Some("Inspect the codebase.".to_string()),
                    queued_prompts: VecDeque::new(),
                    max_turns: None,
                    model_override: None,
                    reasoning_override: None,
                    thread_handle: None,
                    handle: None,
                    notify: Arc::new(Notify::new()),
                    worktree_path: None,
                },
            );
        }
    }

    let err = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("explorer".to_string()),
            message: Some("Inspect another codepath.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("fourth active subagent should be rejected");

    assert!(err.to_string().contains(&format!(
            "Subagent concurrency limit reached (max_concurrent={})",
            controller.config.vt_cfg.subagents.max_concurrent.min(
                SUBAGENT_HARD_CONCURRENCY_LIMIT
            )
        )));
}

#[tokio::test]
async fn wait_returns_first_terminal_child() {
    let temp = TempDir::new().expect("tempdir");
    let controller = SubagentController::new(test_controller_config(
        temp.path().to_path_buf(),
        VTCodeConfig::default(),
    ))
    .await
    .expect("controller");
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "default")
        .expect("default");

    {
        let mut state = controller.state.write().await;
        for id in ["first", "second"] {
            state.children.insert(
                id.to_string(),
                ChildRecord {
                    id: id.to_string(),
                    session_id: format!("session-{id}"),
                    parent_thread_id: "parent-session".to_string(),
                    spec: spec.clone(),
                    display_label: subagent_display_label(&spec),
                    status: SubagentStatus::Running,
                    background: false,
                    depth: 1,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    completed_at: None,
                    summary: None,
                    error: None,
                    archive_metadata: None,
                    archive_path: None,
                    transcript_path: None,
                    effective_config: None,
                    stored_messages: Vec::new(),
                    last_prompt: None,
                    queued_prompts: VecDeque::new(),
                    max_turns: None,
                    model_override: None,
                    reasoning_override: None,
                    thread_handle: None,
                    handle: None,
                    notify: Arc::new(Notify::new()),
                    worktree_path: None,
                },
            );
        }
    }

    let controller_clone = controller.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let mut state = controller_clone.state.write().await;
        let record = state.children.get_mut("second").expect("second child");
        record.status = SubagentStatus::Completed;
        record.summary = Some("done".to_string());
        record.completed_at = Some(Utc::now());
        record.updated_at = Utc::now();
        record.notify.notify_waiters();
    });

    let result = controller
        .wait(&["first".to_string(), "second".to_string()], Some(500))
        .await
        .expect("wait result")
        .expect("terminal child");
    assert_eq!(result.id, "second");
    assert_eq!(result.status, SubagentStatus::Completed);
}
