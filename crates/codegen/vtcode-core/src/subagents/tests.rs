use super::controller_spawn_run::finalize_background_launch;
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
    HookCommandConfig, HookGroupConfig, HooksConfig, IsolationMode, SubagentMcpServer, SubagentMemoryScope,
    SubagentSource, SubagentSpec,
};

fn readonly_agent_permissions() -> AgentPermissionsConfig {
    let mut permissions = AgentPermissionsConfig::new(PermissionDefault::Deny);
    permissions.allow = vec![tools::READ_FILE.to_string()];
    permissions
}

pub(super) fn test_controller_config(workspace_root: PathBuf, vt_cfg: VTCodeConfig) -> SubagentControllerConfig {
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), vt_cfg.pty.clone());
    let exec_sessions = ExecSessionManager::new(workspace_root.clone(), pty_sessions.clone());
    SubagentControllerConfig {
        workspace_root,
        parent_session_id: "parent-session".to_string(),
        parent_model: models::openai::GPT_5_6_SOL.to_string(),
        parent_provider: "openai".to_string(),
        parent_reasoning_effort: ReasoningEffortLevel::Medium,
        api_key: "test-key".to_string(),
        vt_cfg,
        openai_chatgpt_auth: None,
        depth: 0,
        workspace_gated: false,
        exec_sessions,
        pty_manager: pty_sessions.manager().clone(),
        managed_background_runtime: false,
    }
}

fn test_child_record(
    id: &str,
    session_id: &str,
    parent_thread_id: &str,
    spec: &SubagentSpec,
    status: SubagentStatus,
    depth: usize,
    child_controller: Option<Arc<SubagentController>>,
) -> ChildRecord {
    ChildRecord {
        id: id.to_string(),
        session_id: session_id.to_string(),
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
        child_controller,
    }
}

pub(super) fn test_background_record(
    spec: &SubagentSpec,
    id: &str,
    status: BackgroundSubprocessStatus,
    desired_enabled: bool,
    exec_session_id: &str,
) -> BackgroundRecord {
    let now = Utc::now();
    BackgroundRecord {
        exit_code: None,
        termination_requested: false,
        id: id.to_string(),
        agent_name: spec.name.clone(),
        display_label: subagent_display_label(spec),
        description: spec.description.clone(),
        source: spec.source.label(),
        color: spec.color.clone(),
        session_id: "session-background-demo".to_string(),
        exec_session_id: exec_session_id.to_string(),
        desired_enabled,
        status,
        created_at: now,
        updated_at: now,
        started_at: Some(now),
        ended_at: None,
        pid: Some(42),
        prompt: "Report readiness once.".to_string(),
        summary: None,
        error: None,
        archive_path: None,
        transcript_path: None,
        max_turns: Some(4),
        model_override: None,
        reasoning_override: None,
        restart_attempts: 0,
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
    assert_eq!(request_prompt(&request.message, &request.items).as_deref(), Some("hello"));
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

async fn wait_for_effective_model(controller: &SubagentController, target: &str) -> Result<String> {
    for _ in 0..50 {
        if let Ok(snapshot) = controller.snapshot_for_thread(target).await {
            return Ok(snapshot.effective_config.agent.default_model);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    Err(anyhow!("Subagent {target} did not capture an effective runtime configuration in time"))
}

pub(super) fn read_only_test_spec(name: &str) -> SubagentSpec {
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
    let filtered = filter_child_tools(&spec, defs, true, false);
    let names = filtered.iter().map(ToolDefinition::function_name).collect::<Vec<_>>();
    assert_eq!(names, vec![tools::CODE_SEARCH]);
}

#[test]
fn filter_child_tools_keeps_delegation_tools_when_nested_delegation_allowed() {
    let defs = vec![
        ToolDefinition::function(tools::AGENT.to_string(), "Agent".to_string(), serde_json::json!({"type": "object"})),
        ToolDefinition::function(
            tools::SPAWN_AGENT.to_string(),
            "Spawn".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::SEND_INPUT.to_string(),
            "Send".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::WAIT_AGENT.to_string(),
            "Wait".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::SPAWN_BACKGROUND_SUBPROCESS.to_string(),
            "Bg".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::CODE_SEARCH.to_string(),
            "Search".to_string(),
            serde_json::json!({"type": "object"}),
        ),
    ];
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");

    let filtered = filter_child_tools(&spec, defs, false, true);
    let names = filtered.iter().map(ToolDefinition::function_name).collect::<Vec<_>>();

    assert!(names.contains(&tools::AGENT), "agent tool stays exposed when nested delegation is allowed");
    assert!(names.contains(&tools::SPAWN_AGENT));
    assert!(names.contains(&tools::SEND_INPUT));
    assert!(names.contains(&tools::WAIT_AGENT));
    assert!(
        !names.contains(&tools::SPAWN_BACKGROUND_SUBPROCESS),
        "background subprocess alias stays blocked for children"
    );
    assert!(names.contains(&tools::CODE_SEARCH));
}

#[test]
fn filter_child_tools_removes_delegation_tools_by_default() {
    let defs = vec![
        ToolDefinition::function(tools::AGENT.to_string(), "Agent".to_string(), serde_json::json!({"type": "object"})),
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
    ];
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");

    let filtered = filter_child_tools(&spec, defs, false, false);
    let names = filtered.iter().map(ToolDefinition::function_name).collect::<Vec<_>>();
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
        tools: Some(vec![tools::UNIFIED_EXEC.to_string(), tools::CODE_SEARCH.to_string()]),
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

    let filtered = filter_child_tools(&spec, defs, spec.is_read_only(), false);
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

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);
    assert_eq!(child.runtime_agent_permissions.as_ref(), Some(&spec.permissions));
    assert_eq!(child.permissions.allow, vec![tools::READ_FILE.to_string(), tools::CODE_SEARCH.to_string()]);
    assert!(child.permissions.deny.contains(&tools::UNIFIED_EXEC.to_string()));
    assert!(child.permissions.deny.contains(&tools::SPAWN_AGENT.to_string()));
}

#[test]
fn build_child_config_allows_nested_delegation_keeps_agent_tools_out_of_deny() {
    let mut parent = VTCodeConfig::default();
    parent.permissions.allow = vec![
        tools::SPAWN_AGENT.to_string(),
        tools::WAIT_AGENT.to_string(),
        tools::CODE_SEARCH.to_string(),
    ];

    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.tools = Some(parent.permissions.allow.clone());

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, true);

    assert_eq!(
        child.permissions.allow,
        vec![
            tools::SPAWN_AGENT.to_string(),
            tools::WAIT_AGENT.to_string(),
            tools::CODE_SEARCH.to_string()
        ],
        "nested delegation keeps delegation tools in the allow-list"
    );
    assert!(
        !child.permissions.deny.contains(&tools::SPAWN_AGENT.to_string()),
        "spawn_agent must not be denied when nested delegation is allowed"
    );
    assert!(
        !child.permissions.deny.contains(&tools::AGENT.to_string()),
        "agent must not be denied when nested delegation is allowed"
    );
    assert!(
        child.permissions.deny.contains(&tools::SPAWN_BACKGROUND_SUBPROCESS.to_string()),
        "background subprocess alias stays blocked for children"
    );
}

#[test]
fn build_child_config_default_denies_all_subagent_tools() {
    let parent = VTCodeConfig::default();
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);

    assert!(child.permissions.deny.contains(&tools::SPAWN_AGENT.to_string()));
    assert!(child.permissions.deny.contains(&tools::AGENT.to_string()));
    assert!(child.permissions.deny.contains(&tools::SEND_INPUT.to_string()));
    assert!(child.permissions.deny.contains(&tools::WAIT_AGENT.to_string()));
    assert!(child.permissions.deny.contains(&tools::RESUME_AGENT.to_string()));
    assert!(child.permissions.deny.contains(&tools::CLOSE_AGENT.to_string()));
    assert!(child.permissions.deny.contains(&tools::SPAWN_BACKGROUND_SUBPROCESS.to_string()));
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

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);

    assert_eq!(child.permissions.allow, vec![tools::CODE_SEARCH.to_string(), tools::UNIFIED_EXEC.to_string()]);
    assert!(child.permissions.deny.contains(&tools::SPAWN_AGENT.to_string()));
    assert_eq!(child.hooks.lifecycle.pre_tool_use.len(), 1);
    assert_eq!(child.hooks.lifecycle.pre_tool_use[0].hooks[0].command, "echo child");
}

#[test]
fn prepare_child_runtime_config_uses_shared_view_for_model_and_reasoning() {
    let parent = VTCodeConfig::default();
    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");
    spec.model = Some(models::openai::GPT_5_6_LUNA.to_string());
    spec.reasoning_effort = Some(ReasoningEffortLevel::High);

    let (resolved_model, child_reasoning_effort, child_cfg) = prepare_child_runtime_config(
        &parent,
        &spec,
        models::openai::GPT_5_6_SOL,
        "openai",
        ReasoningEffortLevel::Low,
        None,
        None,
        None,
        false,
        |_, parent_model, parent_provider, model_override, spec_model, agent_name| {
            assert_eq!(parent_model, models::openai::GPT_5_6_SOL);
            assert_eq!(parent_provider, "openai");
            assert_eq!(model_override, None);
            assert_eq!(spec_model, Some(models::openai::GPT_5_6_LUNA));
            assert_eq!(agent_name, "worker");
            Ok(models::openai::GPT_5_6_LUNA.parse::<ModelId>().expect("valid model"))
        },
    )
    .expect("prepared child runtime config");

    assert_eq!(resolved_model.as_str(), models::openai::GPT_5_6_LUNA);
    assert_eq!(child_cfg.agent.default_model, models::openai::GPT_5_6_LUNA);
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
    // Writable children state the loop detector's real subagent limits.
    assert!(instructions.contains(&format!(
        "{} read-only calls in total",
        crate::core::loop_detector::SUBAGENT_MAX_TOTAL_READONLY_CALLS
    )));
    assert!(instructions.contains(&format!(
        "{} consecutive reads/searches",
        crate::core::loop_detector::SUBAGENT_NAVIGATION_HARD_STOP_STREAK
    )));
    assert!(!instructions.contains("CRITICAL"));
}

#[test]
fn final_response_contract_yields_to_agent_defined_format() {
    // The verifier's own format ends with a `Decision:` line the harness
    // parses; the generic contract must not compete with it.
    let mut spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");
    spec.name = "verifier".to_string();
    spec.prompt = "End with exactly one line, `Decision: APPROVED` or `Decision: REJECTED`.".to_string();

    let instructions = compose_subagent_instructions(&spec, None);

    assert!(instructions.contains(
        "If your agent instructions or the task define their own response format, follow that format instead."
    ));
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

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);

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

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);

    assert_eq!(child.permissions.allow, vec!["Read".to_string()]);
}

#[test]
fn build_child_config_promotes_single_turn_budget_to_recovery_budget() {
    let parent = VTCodeConfig::default();
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, Some(1), false);

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
    assert_eq!(normalize_background_child_max_turns(Some(1), false), Some(SUBAGENT_MIN_MAX_TURNS));
    assert_eq!(normalize_background_child_max_turns(Some(2), false), Some(2));
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

    let child = build_child_config(&parent, &spec, models::openai::GPT_5_6_SOL, None, false);
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
    assert!(contains_explicit_delegation_request("@agent-worker fix the issue", direct_mentions.as_slice()));
    let no_mentions = extract_explicit_agent_mentions("delegate this in parallel", &[]);
    assert!(contains_explicit_delegation_request("delegate this in parallel", no_mentions.as_slice()));
    let empty_mentions = extract_explicit_agent_mentions("review the repository", &[]);
    assert!(!contains_explicit_delegation_request("review the repository", empty_mentions.as_slice()));
}

#[test]
fn explicit_agent_mentions_detect_natural_language_selection() {
    let rust_engineer = read_only_test_spec("rust-engineer");
    assert_eq!(
        extract_explicit_agent_mentions("use rust-engineer agent to review current code", &[rust_engineer]),
        vec!["rust-engineer".to_string()]
    );
}

#[test]
fn explicit_agent_mentions_detect_looser_subagent_selection() {
    let background_demo = read_only_test_spec("background-demo");
    assert_eq!(
        extract_explicit_agent_mentions("use background-demo and run the subagent", &[background_demo]),
        vec!["background-demo".to_string()]
    );
}

#[test]
fn explicit_agent_mentions_detect_run_subagent_selection() {
    let rust_engineer = read_only_test_spec("rust-engineer");
    assert_eq!(
        extract_explicit_agent_mentions("run rust-engineer subagent and review changes", &[rust_engineer]),
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
    assert!(contains_explicit_model_request("delegate this using gpt-5.6-luna", "gpt-5.6-luna"));
    assert!(contains_explicit_model_request("use the worker subagent with haiku", "haiku"));
    assert!(contains_explicit_model_request("run this with the small model", "small"));
    assert!(!contains_explicit_model_request("delegate this small cleanup task", "small"));
    assert!(!contains_explicit_model_request("delegate this task", "gpt-5.6-luna"));
}

#[test]
fn normalize_requested_model_override_drops_default_like_values() {
    assert_eq!(normalize_requested_model_override(Some("default".to_string()), "delegate this task"), None);
    assert_eq!(normalize_requested_model_override(Some(" inherit ".to_string()), "delegate this task"), None);
    assert_eq!(
        normalize_requested_model_override(Some(" inherit ".to_string()), "delegate this task using inherit"),
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

#[test]
fn subagent_input_item_reads_schema_type_field() {
    let item: SubagentInputItem =
        serde_json::from_value(serde_json::json!({"type": "path", "path": "src/lib.rs"})).expect("item");
    assert_eq!(item.item_type.as_deref(), Some("path"));
    assert_eq!(serde_json::to_value(&item).expect("serialize")["type"], serde_json::json!("path"));

    let legacy: SubagentInputItem =
        serde_json::from_value(serde_json::json!({"item_type": "text", "text": "x"})).expect("legacy item");
    assert_eq!(legacy.item_type.as_deref(), Some("text"));
}

#[test]
fn agent_schema_reasoning_effort_enum_matches_parser() {
    let schema = vtcode_utility_tool_specs::agent_parameters();
    let listed = schema["properties"]["reasoning_effort"]["enum"]
        .as_array()
        .expect("enum")
        .iter()
        .map(|value| value.as_str().expect("string enum value").to_string())
        .collect::<Vec<_>>();
    // Schema -> parser: every advertised value parses back to itself.
    for value in &listed {
        let parsed = ReasoningEffortLevel::parse(value).unwrap_or_else(|| panic!("schema value {value} must parse"));
        assert_eq!(parsed.as_str(), value);
    }
    // Parser -> schema: every named level is advertised. The exhaustive match
    // makes a new variant fail to compile here until it is listed.
    let named = [
        ReasoningEffortLevel::None,
        ReasoningEffortLevel::Minimal,
        ReasoningEffortLevel::Low,
        ReasoningEffortLevel::Medium,
        ReasoningEffortLevel::High,
        ReasoningEffortLevel::XHigh,
        ReasoningEffortLevel::Max,
    ];
    for level in named {
        match level {
            ReasoningEffortLevel::None
            | ReasoningEffortLevel::Minimal
            | ReasoningEffortLevel::Low
            | ReasoningEffortLevel::Medium
            | ReasoningEffortLevel::High
            | ReasoningEffortLevel::XHigh
            | ReasoningEffortLevel::Max
            | ReasoningEffortLevel::Unknown => {}
        }
        assert!(listed.iter().any(|value| value == level.as_str()), "{level} missing from schema enum");
    }
    assert_eq!(listed.len(), named.len());
}

#[tokio::test]
async fn controller_exposes_builtin_specs() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    let specs = controller.effective_specs().await;
    assert!(specs.iter().any(|spec| spec.name == "explorer"));
    assert!(specs.iter().any(|spec| spec.name == "worker"));
}

#[tokio::test]
async fn spawn_defaults_to_single_explicit_mention() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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

    assert!(err.to_string().contains("user explicitly selected 'explorer'"));
}

#[tokio::test]
async fn spawn_rejects_write_capable_agent_without_explicit_request_or_agent_type() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    let err = controller
        .spawn(SpawnAgentRequest {
            message: Some("Implement a change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("write-capable agent should require explicit request or agent_type");

    assert!(err.to_string().contains("cannot launch write-capable agent"));
}

#[tokio::test]
async fn spawn_allows_write_capable_agent_with_explicit_agent_type() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller.set_turn_delegation_hints_from_input("delegate this task").await;

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
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
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

    let appendix = load_memory_appendix(temp.path(), "reviewer", Some(SubagentMemoryScope::Project))
        .expect("appendix")
        .expect("memory appendix");

    assert!(appendix.contains("Persistent memory file:"));
    assert!(appendix.contains("Key points:"));
    assert!(appendix.contains("Keep diffs surgical."));
    assert!(appendix.contains("Open `MEMORY.md` when exact wording or more detail matters."));
    assert!(!appendix.contains("Current MEMORY.md excerpt"));
    assert!(!appendix.contains("## Preferences"));
}

#[tokio::test]
async fn async_load_memory_appendix_matches_sync_output() {
    let temp = TempDir::new().expect("tempdir");
    let memory_dir = temp.path().join(".vtcode/agent-memory/reviewer");
    std::fs::create_dir_all(&memory_dir).expect("memory dir");
    std::fs::write(
        memory_dir.join("MEMORY.md"),
        "# Reviewer Memory\n\n- Keep the patch focused.\n- Run nextest before the workspace gate.\n",
    )
    .expect("write memory");

    let sync =
        load_memory_appendix(temp.path(), "reviewer", Some(SubagentMemoryScope::Project)).expect("sync appendix");
    let asynchronous = load_memory_appendix_async(temp.path(), "reviewer", Some(SubagentMemoryScope::Project))
        .await
        .expect("async appendix");

    assert_eq!(asynchronous, sync);
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

    let appendix = load_primary_memory_appendix(temp.path(), "reviewer", Some(SubagentMemoryScope::Project))
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
        load_primary_memory_appendix(temp.path(), "reviewer", Some(SubagentMemoryScope::Project)).expect("appendix");

    assert!(appendix.is_none());
    assert!(!memory_dir.exists());
}

#[tokio::test]
async fn spawn_honors_model_override_when_user_explicitly_requests_it() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("delegate this task using gpt-5.4-mini")
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement the change.".to_string()),
            model: Some(models::openai::GPT_5_6_LUNA.to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    let effective_model = wait_for_effective_model(&controller, &spawned.id)
        .await
        .expect("effective model");
    assert_eq!(effective_model, models::openai::GPT_5_6_LUNA);
    controller.close(&spawned.id).await.expect("close");
}

#[tokio::test]
async fn spawn_background_subprocess_rejects_non_background_agent() {
    let temp = TempDir::new().expect("tempdir");
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    controller.set_turn_delegation_hints_from_input("delegate this task").await;

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
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    controller.set_turn_delegation_hints_from_input("delegate this task").await;

    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let created_at = Utc::now();
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            BackgroundRecord {
                exit_code: None,
                termination_requested: false,
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
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    controller.set_turn_delegation_hints_from_input("delegate this task").await;

    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let created_at = Utc::now();
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id,
            BackgroundRecord {
                exit_code: None,
                termination_requested: false,
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
async fn wait_for_background_returns_stopped_record_immediately() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Stopped, false, ""),
        );
    }

    let entry = controller
        .wait_for_background(std::slice::from_ref(&record_id), Some(50))
        .await
        .expect("wait")
        .expect("stopped record should complete immediately");
    assert_eq!(entry.id, record_id);
    assert_eq!(entry.status, BackgroundSubprocessStatus::Stopped);
}

#[tokio::test]
async fn wait_for_background_times_out_on_running_record() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(
                &spec,
                &record_id,
                BackgroundSubprocessStatus::Running,
                true,
                "exec-session-missing",
            ),
        );
    }

    let entry = controller
        .wait_for_background(std::slice::from_ref(&record_id), Some(50))
        .await
        .expect("wait");
    assert!(entry.is_none(), "running record should time out, not hallucinate completion");
}

#[tokio::test]
async fn wait_for_background_is_fail_closed_for_unknown_and_empty_targets() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    assert!(
        controller
            .wait_for_background(&["background-missing".to_string()], Some(10))
            .await
            .expect("wait")
            .is_none()
    );
    assert!(controller.wait_for_background(&[], Some(10)).await.expect("wait").is_none());
}

#[tokio::test]
#[cfg(unix)]
async fn managed_background_completion_persists_before_delivery() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");
    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let exec_session_id = "exec-managed-clean-event";
    let mut completions = controller.subscribe_background_completions();

    controller
        .config
        .exec_sessions
        .create_pipe_session_for_managed_background(
            exec_session_id.to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 0.1; exit 0".to_string()],
            temp.path().to_path_buf(),
            Default::default(),
        )
        .await
        .expect("managed session");
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Running, true, exec_session_id),
        );
    }

    let event = tokio::time::timeout(Duration::from_secs(3), completions.recv())
        .await
        .expect("completion should arrive")
        .expect("completion channel should remain open");
    assert_eq!(event.task_id, record_id);
    assert!(!event.termination_requested);
    assert_eq!(event.status, BackgroundSubprocessStatus::Stopped);
    assert_eq!(event.exit_code, Some(0));
    let status = controller
        .background_status_entries()
        .await
        .into_iter()
        .find(|entry| entry.id == record_id)
        .expect("terminal record remains visible without refresh");
    assert_eq!(status.status, BackgroundSubprocessStatus::Stopped);

    let persisted = load_background_state(temp.path()).await.expect("persisted state");
    let persisted_record = persisted
        .records
        .iter()
        .find(|record| record.id == record_id)
        .expect("record persisted");
    let persisted_json = serde_json::to_value(persisted_record).expect("persisted record serializes");
    assert_eq!(persisted_json["status"], "stopped");
    assert!(
        persisted_json["summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("successfully"))
    );

    controller.signal_shutdown().await;
    controller.config.exec_sessions.close_session(exec_session_id).await.ok();
}

#[tokio::test]
#[cfg(unix)]
async fn managed_background_graceful_stop_delivers_one_terminal_completion() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");
    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let exec_session_id = "exec-managed-graceful-stop-event";
    let mut completions = controller.subscribe_background_completions();

    controller
        .config
        .exec_sessions
        .create_pipe_session_for_managed_background(
            exec_session_id.to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            temp.path().to_path_buf(),
            Default::default(),
        )
        .await
        .expect("managed session");
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Running, true, exec_session_id),
        );
    }

    let stopped = controller.graceful_stop_background(&record_id).await.expect("graceful stop");
    assert_eq!(stopped.status, BackgroundSubprocessStatus::Stopped);
    assert!(!stopped.desired_enabled);

    let event = tokio::time::timeout(Duration::from_secs(3), completions.recv())
        .await
        .expect("stop completion should arrive")
        .expect("completion channel should remain open");
    assert_eq!(event.task_id, record_id);
    assert!(event.termination_requested);
    assert_eq!(event.status, BackgroundSubprocessStatus::Stopped);
    assert!(event.error.is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), completions.recv())
            .await
            .is_err()
    );

    controller.signal_shutdown().await;
    controller.config.exec_sessions.close_session(exec_session_id).await.ok();
}

#[tokio::test]
#[cfg(unix)]
async fn managed_background_force_cancel_delivers_one_terminal_completion() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");
    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let exec_session_id = "exec-managed-force-cancel-event";
    let mut completions = controller.subscribe_background_completions();

    controller
        .config
        .exec_sessions
        .create_pipe_session_for_managed_background(
            exec_session_id.to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            temp.path().to_path_buf(),
            Default::default(),
        )
        .await
        .expect("managed session");
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Running, true, exec_session_id),
        );
    }

    let stopped = controller.force_cancel_background(&record_id).await.expect("force cancel");
    assert_eq!(stopped.status, BackgroundSubprocessStatus::Stopped);
    assert!(!stopped.desired_enabled);

    let event = tokio::time::timeout(Duration::from_secs(3), completions.recv())
        .await
        .expect("force-cancel completion should arrive")
        .expect("completion channel should remain open");
    assert_eq!(event.task_id, record_id);
    assert!(event.termination_requested);
    assert_eq!(event.status, BackgroundSubprocessStatus::Stopped);
    assert_eq!(event.exit_code, None);
    assert!(event.error.is_none());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), completions.recv())
            .await
            .is_err()
    );

    controller.signal_shutdown().await;
}

#[tokio::test]
#[cfg(unix)]
async fn parent_subscription_replays_completion_published_before_receiver() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");
    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let exec_session_id = "exec-managed-completed-before-parent-subscribe";

    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Running, true, exec_session_id),
        );
    }
    controller
        .config
        .exec_sessions
        .create_pipe_session_for_managed_background(
            exec_session_id.to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 0.1; exit 0".to_string()],
            temp.path().to_path_buf(),
            Default::default(),
        )
        .await
        .expect("managed session");

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let stopped = controller
                .background_status_entries()
                .await
                .into_iter()
                .any(|entry| entry.id == record_id && entry.status == BackgroundSubprocessStatus::Stopped);
            if stopped {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("monitor should persist terminal state before subscription");

    let mut completions = controller.subscribe_parent_background_completions();
    let event = tokio::time::timeout(Duration::from_secs(1), completions.recv())
        .await
        .expect("parent subscription should replay completion")
        .expect("completion channel should remain open");
    assert_eq!(event.task_id, record_id);
    assert!(!event.termination_requested);
    assert_eq!(event.status, BackgroundSubprocessStatus::Stopped);
    assert_eq!(event.exit_code, Some(0));

    controller.signal_shutdown().await;
    controller.config.exec_sessions.close_session(exec_session_id).await.ok();
}

#[tokio::test]
async fn background_completion_monitor_shutdown_cancels_and_joins_task() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    assert!(controller.background_completion_monitor.lock().await.is_some());
    controller.signal_shutdown().await;
    assert!(controller.background_completion_shutdown.is_cancelled());
    assert!(controller.background_completion_monitor.lock().await.is_none());
}

#[tokio::test]
async fn background_completion_monitor_is_cancelled_when_last_owner_drops() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    let shutdown = controller.background_completion_shutdown.clone();
    let monitor_slot = Arc::clone(&controller.background_completion_monitor);
    let controller_clone = controller.clone();

    drop(controller);
    assert!(!shutdown.is_cancelled(), "a live clone must keep the monitor running");

    drop(controller_clone);
    assert!(shutdown.is_cancelled(), "dropping the final owner must cancel the monitor");
    assert!(monitor_slot.lock().await.is_none(), "the monitor handle must be reclaimed");
}

#[tokio::test]
#[cfg(unix)]
async fn managed_background_nonzero_completion_is_delivered_as_error() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    cfg.subagents.background.auto_restore = false;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");
    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let exec_session_id = "exec-managed-error-event";
    let mut completions = controller.subscribe_background_completions();

    controller
        .config
        .exec_sessions
        .create_pipe_session_for_managed_background(
            exec_session_id.to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 0.1; exit 9".to_string()],
            temp.path().to_path_buf(),
            Default::default(),
        )
        .await
        .expect("managed session");
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Running, true, exec_session_id),
        );
    }

    let event = tokio::time::timeout(Duration::from_secs(3), completions.recv())
        .await
        .expect("completion should arrive")
        .expect("completion channel should remain open");
    assert_eq!(event.task_id, record_id);
    assert_eq!(event.status, BackgroundSubprocessStatus::Error);
    assert_eq!(event.exit_code, Some(9));
    assert!(event.error.as_deref().is_some_and(|error| error.contains('9')));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), completions.recv())
            .await
            .is_err()
    );

    controller.signal_shutdown().await;
    controller.config.exec_sessions.close_session(exec_session_id).await.ok();
}

#[tokio::test]
#[cfg(unix)]
async fn wait_for_background_races_completion_notification() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    cfg.subagents.background.auto_restore = false;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");
    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let record_id = background_record_id(spec.name.as_str());
    let exec_session_id = "exec-managed-wait-notification";

    controller
        .config
        .exec_sessions
        .create_pipe_session_for_managed_background(
            exec_session_id.to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 0.2; exit 0".to_string()],
            temp.path().to_path_buf(),
            Default::default(),
        )
        .await
        .expect("managed session");
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            record_id.clone(),
            test_background_record(&spec, &record_id, BackgroundSubprocessStatus::Running, true, exec_session_id),
        );
    }

    let started = std::time::Instant::now();
    let entry = controller
        .wait_for_background(std::slice::from_ref(&record_id), Some(2_000))
        .await
        .expect("wait");
    assert_eq!(entry.expect("completion").status, BackgroundSubprocessStatus::Stopped);
    assert!(started.elapsed() < Duration::from_secs(2));

    controller.signal_shutdown().await;
    controller.config.exec_sessions.close_session(exec_session_id).await.ok();
}

/// The unified `agent wait` surface must return a settled target in either
/// scope promptly, instead of blocking out a still-running target in the
/// other. A running delegated child plus an already-stopped background
/// subprocess must resolve on the background entry well before the child's
/// (5s) timeout would expire.
#[tokio::test]
async fn agent_wait_returns_settled_background_entry_without_waiting_out_delegated_child() {
    let temp = TempDir::new().expect("tempdir");
    write_test_background_subagent(temp.path());
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = Arc::new(
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
            .await
            .expect("controller"),
    );

    let spec = controller.resolve_requested_spec(Some("background-demo")).await.expect("spec");
    let background_id = background_record_id(spec.name.as_str());
    let delegated_spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "default")
        .expect("default");
    {
        let mut state = controller.state.write().await;
        state.children.insert(
            "delegated-running".to_string(),
            test_child_record(
                "delegated-running",
                "session-delegated",
                "parent-session",
                &delegated_spec,
                SubagentStatus::Running,
                1,
                None,
            ),
        );
        state.background_children.insert(
            background_id.clone(),
            test_background_record(&spec, &background_id, BackgroundSubprocessStatus::Stopped, false, ""),
        );
    }

    let registry = crate::tools::registry::ToolRegistry::new(temp.path().to_path_buf()).await;
    registry.set_subagent_controller(Arc::clone(&controller));

    let started = std::time::Instant::now();
    let response = registry
        .agent_executor(serde_json::json!({
            "action": "wait",
            "ids": ["delegated-running", background_id],
            "timeout_ms": 5_000
        }))
        .await
        .expect("agent wait");
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_secs(2),
        "settled background target must not wait out the delegated child: {elapsed:?}"
    );
    assert_eq!(response["completed"], serde_json::json!(true));
    assert_eq!(response["entry"]["id"], serde_json::json!(background_id));
    assert_eq!(response["entry"]["status"], serde_json::json!("stopped"));
}

#[tokio::test]
async fn resume_preserves_captured_runtime_overrides() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller
        .set_turn_delegation_hints_from_input("delegate this task using gpt-5.4-mini")
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement the change.".to_string()),
            model: Some(models::openai::GPT_5_6_LUNA.to_string()),
            max_turns: Some(2),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    let initial_model = wait_for_effective_model(&controller, &spawned.id)
        .await
        .expect("initial effective model");
    assert_eq!(initial_model, models::openai::GPT_5_6_LUNA);

    let closed = controller.close(&spawned.id).await.expect("close");
    assert_eq!(closed.status, SubagentStatus::Closed);

    controller.resume(&spawned.id).await.expect("resume");

    for _ in 0..100 {
        let status = controller.status_for(&spawned.id).await.expect("status");
        if status.updated_at > closed.updated_at && status.status != SubagentStatus::Closed {
            let snapshot = controller.snapshot_for_thread(&spawned.id).await.expect("snapshot");
            assert_eq!(snapshot.effective_config.agent.default_model, models::openai::GPT_5_6_LUNA);
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
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller.set_turn_delegation_hints_from_input("delegate this task").await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement the change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn");

    let snapshot = controller.snapshot_for_thread(&spawned.id).await.expect("snapshot");

    assert_eq!(snapshot.id, spawned.id);
    assert!(!snapshot.effective_config.agent.default_model.trim().is_empty());

    controller.close(&spawned.id).await.expect("close");
}

#[tokio::test]
async fn spawn_custom_uses_explicit_spec_without_delegation_hints() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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
                message: Some("Inspect the repository and report agent-facing findings.".to_string()),
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
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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

    assert!(err.to_string().contains("custom subagent spawn only supports read-only specs"));
}

#[tokio::test]
async fn spawn_custom_rejects_primary_only_spec() {
    let temp = TempDir::new().expect("tempdir");
    write_test_primary_agent(temp.path());
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    controller.set_turn_delegation_hints_from_input("delegate this task").await;
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
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    controller.set_turn_delegation_hints_from_input("delegate this task").await;
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
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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
            test_child_record("parent", "session-parent", "session-root", &spec, SubagentStatus::Running, 1, None),
        );
        state.children.insert(
            "child".to_string(),
            test_child_record("child", "session-child", "parent", &spec, SubagentStatus::Running, 2, None),
        );
        state.children.insert(
            "grandchild".to_string(),
            test_child_record("grandchild", "session-grandchild", "child", &spec, SubagentStatus::Running, 3, None),
        );
    }

    let closed = controller.close("parent").await.expect("close");
    assert_eq!(closed.status, SubagentStatus::Closed);
    assert_eq!(controller.status_for("child").await.expect("child").status, SubagentStatus::Closed);
    assert_eq!(controller.status_for("grandchild").await.expect("grandchild").status, SubagentStatus::Closed);

    let subtree_ids = controller.collect_spawn_subtree_ids("parent").await.expect("collect subtree");
    assert_eq!(subtree_ids, vec!["parent".to_string(), "child".to_string(), "grandchild".to_string()]);

    let mut restart_ids = Vec::new();
    for node_id in subtree_ids {
        if controller.reopen_single(node_id.as_str()).await.expect("reopen subtree node") {
            restart_ids.push(node_id);
        }
    }

    assert_eq!(restart_ids, vec!["parent".to_string(), "child".to_string(), "grandchild".to_string()]);
    assert_eq!(controller.status_for("parent").await.expect("parent").status, SubagentStatus::Queued);
    assert_eq!(controller.status_for("child").await.expect("child").status, SubagentStatus::Queued);
    assert_eq!(controller.status_for("grandchild").await.expect("grandchild").status, SubagentStatus::Queued);
}

#[tokio::test]
async fn spawn_rejects_fourth_active_subagent() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    controller.set_turn_delegation_hints_from_input("delegate this task").await;

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
                    child_controller: None,
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
async fn spawn_from_child_controller_respects_depth_limit() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.subagents.max_depth = 2;

    // Child controller runs at depth 1: it may spawn a grandchild (depth 2),
    // but the grandchild itself (depth 2) may not spawn further.
    let mut child_config = test_controller_config(temp.path().to_path_buf(), vt_cfg.clone());
    child_config.depth = 1;
    let child_controller = SubagentController::new(child_config).await.expect("child controller");

    child_controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("explorer".to_string()),
            message: Some("Inspect the codebase.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("grandchild spawn should be allowed at depth 1 with max_depth=2");

    // Now a controller at depth 2 must refuse another spawn.
    let mut grandchild_config = test_controller_config(temp.path().to_path_buf(), vt_cfg);
    grandchild_config.depth = 2;
    let grandchild_controller = SubagentController::new(grandchild_config).await.expect("grandchild controller");

    let err = grandchild_controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("explorer".to_string()),
            message: Some("Inspect yet more.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("spawn at depth == max_depth should hit the depth limit");

    assert!(err.to_string().contains("Subagent depth limit reached (max_depth=2)"));

    // Abort queued child tasks so they cannot run provider work after the
    // temp workspace is removed.
    child_controller.signal_shutdown().await;
    grandchild_controller.signal_shutdown().await;
}

#[tokio::test]
async fn nested_spawn_rejects_worktree_isolation() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.subagents.max_depth = 2;
    let mut child_config = test_controller_config(temp.path().to_path_buf(), vt_cfg);
    child_config.depth = 1;
    let child_controller = SubagentController::new(child_config).await.expect("child controller");
    child_controller
        .set_turn_delegation_hints_from_input("delegate this task")
        .await;

    // Force the discovered "worker" spec to request worktree isolation so the
    // nested-spawn guard can be exercised.
    {
        let mut state = child_controller.state.write().await;
        if let Some(spec) = state.discovered.effective.iter_mut().find(|spec| spec.name == "worker") {
            spec.isolation = Some(IsolationMode::Worktree);
        }
    }

    let err = child_controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("worker".to_string()),
            message: Some("Implement a change.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("nested worktree isolation should be rejected");

    assert!(err.to_string().contains("nested worktree isolation is not supported"));
}

#[tokio::test]
async fn close_cascades_to_child_scoped_grandchildren() {
    let temp = TempDir::new().expect("tempdir");
    let parent = SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
        .await
        .expect("parent controller");

    // Child-scoped controller that already has a grandchild tracked under the
    // child's session id.
    let child_controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("child controller");
    let child_session_id = "child-session".to_string();
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");
    {
        let mut state = child_controller.state.write().await;
        state.children.insert(
            "grandchild".to_string(),
            test_child_record(
                "grandchild",
                "session-grandchild",
                &child_session_id,
                &spec,
                SubagentStatus::Running,
                2,
                None,
            ),
        );
    }
    let child_controller = std::sync::Arc::new(child_controller);

    // Register the child on the parent controller with the child-scoped
    // controller attached, then close it and assert the grandchild is closed.
    {
        let mut state = parent.state.write().await;
        state.children.insert(
            "child".to_string(),
            test_child_record(
                "child",
                &child_session_id,
                "parent-session",
                &spec,
                SubagentStatus::Running,
                1,
                Some(child_controller.clone()),
            ),
        );
    }

    let closed = parent.close("child").await.expect("close child");
    assert!(closed.status.is_terminal());

    let grandchild_status = child_controller.status_for("grandchild").await.expect("grandchild status");
    assert_eq!(
        grandchild_status.status,
        SubagentStatus::Closed,
        "closing the child must cascade to its grandchildren"
    );
}

#[tokio::test]
async fn close_does_not_affect_sibling_grandchildren() {
    let temp = TempDir::new().expect("tempdir");
    let parent = SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
        .await
        .expect("parent controller");

    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");

    // Two siblings each with their own child-scoped controller and a grandchild.
    let controller_a =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller a");
    {
        let mut state = controller_a.state.write().await;
        state.children.insert(
            "a-grandchild".to_string(),
            test_child_record(
                "a-grandchild",
                "session-a-grandchild",
                "session-a",
                &spec,
                SubagentStatus::Running,
                2,
                None,
            ),
        );
    }
    let controller_b =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller b");
    {
        let mut state = controller_b.state.write().await;
        state.children.insert(
            "b-grandchild".to_string(),
            test_child_record(
                "b-grandchild",
                "session-b-grandchild",
                "session-b",
                &spec,
                SubagentStatus::Running,
                2,
                None,
            ),
        );
    }
    let controller_a = std::sync::Arc::new(controller_a);
    let controller_b = std::sync::Arc::new(controller_b);

    {
        let mut state = parent.state.write().await;
        state.children.insert(
            "a".to_string(),
            test_child_record(
                "a",
                "session-a",
                "parent-session",
                &spec,
                SubagentStatus::Running,
                1,
                Some(controller_a.clone()),
            ),
        );
        state.children.insert(
            "b".to_string(),
            test_child_record(
                "b",
                "session-b",
                "parent-session",
                &spec,
                SubagentStatus::Running,
                1,
                Some(controller_b.clone()),
            ),
        );
    }

    parent.close("a").await.expect("close a");

    let b_grandchild = controller_b.status_for("b-grandchild").await.expect("b-grandchild status");
    assert_eq!(
        b_grandchild.status,
        SubagentStatus::Running,
        "closing sibling 'a' must not affect sibling 'b''s grandchildren"
    );
    let a_grandchild = controller_a.status_for("a-grandchild").await.expect("a-grandchild status");
    assert_eq!(a_grandchild.status, SubagentStatus::Closed, "closing 'a' must cascade to its own grandchildren");
}

#[tokio::test]
async fn spawn_is_rejected_after_close_begins() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.subagents.max_depth = 2;
    let mut child_config = test_controller_config(temp.path().to_path_buf(), vt_cfg);
    child_config.depth = 1;
    let child_controller = SubagentController::new(child_config).await.expect("child controller");
    child_controller
        .set_turn_delegation_hints_from_input("delegate this task")
        .await;

    // Mark the controller as closing: this is exactly what `close_tree` does
    // to a child-scoped controller before aborting its subtree, so a still-
    // running child cannot spawn a grandchild after the descendant snapshot.
    child_controller.begin_close().await;

    let err = child_controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("explorer".to_string()),
            message: Some("Inspect the codebase.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("spawn after close began must be rejected");

    assert!(err.to_string().contains("shutting down"));

    // Reopening the subtree clears the transient close flag, so a resumed
    // child can delegate again. Permanent `shutdown_requested` stays set only
    // for real shutdown, not for a subtree close.
    child_controller.end_close().await;
    child_controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("explorer".to_string()),
            message: Some("Inspect the codebase.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn after end_close must be allowed again");

    // Abort the queued child task so it cannot run provider work after the
    // temp workspace is removed.
    child_controller.signal_shutdown().await;
}

#[tokio::test]
async fn resume_rejected_after_shutdown() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller.signal_shutdown().await;

    let err = controller
        .resume("some-agent")
        .await
        .expect_err("resume after shutdown must be rejected");

    assert!(err.to_string().contains("shutting down"));
}

#[tokio::test]
async fn resume_restores_closed_grandchildren() {
    let temp = TempDir::new().expect("tempdir");
    let parent = SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
        .await
        .expect("parent controller");

    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");

    // Child-scoped controller that has a grandchild under the child session id.
    let child_controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("child controller");
    let child_session_id = "child-session".to_string();
    {
        let mut state = child_controller.state.write().await;
        state.children.insert(
            "grandchild".to_string(),
            test_child_record(
                "grandchild",
                "session-grandchild",
                &child_session_id,
                &spec,
                SubagentStatus::Running,
                2,
                None,
            ),
        );
    }
    let child_controller = std::sync::Arc::new(child_controller);
    {
        let mut state = parent.state.write().await;
        state.children.insert(
            "child".to_string(),
            test_child_record(
                "child",
                &child_session_id,
                "parent-session",
                &spec,
                SubagentStatus::Running,
                1,
                Some(child_controller.clone()),
            ),
        );
    }

    // Closing the child cascades to the grandchild...
    parent.close("child").await.expect("close child");
    let closed_grandchild = child_controller.status_for("grandchild").await.expect("grandchild status");
    assert_eq!(closed_grandchild.status, SubagentStatus::Closed, "grandchild must be closed with the child");

    // ...and resuming the child must reopen the grandchild too, not just clear
    // the close gate.
    parent.resume("child").await.expect("resume child");
    let resumed_grandchild = child_controller.status_for("grandchild").await.expect("grandchild status");
    assert!(
        matches!(resumed_grandchild.status, SubagentStatus::Queued | SubagentStatus::Running),
        "resumed grandchild must be re-queued, got {:?}",
        resumed_grandchild.status
    );

    // Abort restarted child tasks so they cannot run provider work after the
    // temp workspace is removed.
    parent.signal_shutdown().await;
}

#[tokio::test]
async fn resume_does_not_affect_sibling_grandchildren() {
    let temp = TempDir::new().expect("tempdir");
    let parent = SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
        .await
        .expect("parent controller");

    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");

    // Two siblings each with their own child-scoped controller and a grandchild.
    let controller_a =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller a");
    {
        let mut state = controller_a.state.write().await;
        state.children.insert(
            "a-grandchild".to_string(),
            test_child_record(
                "a-grandchild",
                "session-a-grandchild",
                "session-a",
                &spec,
                SubagentStatus::Running,
                2,
                None,
            ),
        );
    }
    let controller_b =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller b");
    {
        let mut state = controller_b.state.write().await;
        state.children.insert(
            "b-grandchild".to_string(),
            test_child_record(
                "b-grandchild",
                "session-b-grandchild",
                "session-b",
                &spec,
                SubagentStatus::Running,
                2,
                None,
            ),
        );
    }
    let controller_a = std::sync::Arc::new(controller_a);
    let controller_b = std::sync::Arc::new(controller_b);
    {
        let mut state = parent.state.write().await;
        state.children.insert(
            "a".to_string(),
            test_child_record(
                "a",
                "session-a",
                "parent-session",
                &spec,
                SubagentStatus::Running,
                1,
                Some(controller_a.clone()),
            ),
        );
        state.children.insert(
            "b".to_string(),
            test_child_record(
                "b",
                "session-b",
                "parent-session",
                &spec,
                SubagentStatus::Running,
                1,
                Some(controller_b.clone()),
            ),
        );
    }

    // Close both siblings' grandchildren, then resume only "a": "b"'s
    // grandchild must stay closed (resume_tree must respect subtree isolation).
    parent.close("a").await.expect("close a");
    parent.close("b").await.expect("close b");

    parent.resume("a").await.expect("resume a");

    let a_grandchild = controller_a.status_for("a-grandchild").await.expect("a-grandchild status");
    assert!(
        matches!(a_grandchild.status, SubagentStatus::Queued | SubagentStatus::Running),
        "resumed 'a''s grandchild must be re-queued, got {:?}",
        a_grandchild.status
    );
    let b_grandchild = controller_b.status_for("b-grandchild").await.expect("b-grandchild status");
    assert_eq!(
        b_grandchild.status,
        SubagentStatus::Closed,
        "resuming sibling 'a' must not reopen sibling 'b''s grandchildren"
    );

    // Abort restarted child tasks so they cannot run provider work after the
    // temp workspace is removed.
    parent.signal_shutdown().await;
}

#[tokio::test]
async fn wait_returns_first_terminal_child() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
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
                    child_controller: None,
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

#[tokio::test]
#[cfg(unix)]
async fn background_clean_exit_zero_becomes_stopped_without_restart() {
    let temp = TempDir::new().expect("tempdir");
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    cfg.subagents.background.auto_restore = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    let workspace = controller.config.workspace_root.clone();
    controller
        .config
        .exec_sessions
        .create_pipe_session_with_sandbox_and_background(
            "exec-clean-exit".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "exit 0".to_string()],
            workspace,
            Default::default(),
            false,
            true,
        )
        .await
        .expect("clean exec session");
    wait_for_exec_exit(&controller.config.exec_sessions, "exec-clean-exit").await;

    let created_at = Utc::now();
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            "background-clean".to_string(),
            BackgroundRecord {
                exit_code: None,
                termination_requested: false,
                id: "background-clean".to_string(),
                agent_name: "demo".to_string(),
                display_label: "demo".to_string(),
                description: "demo".to_string(),
                source: "test".to_string(),
                color: None,
                session_id: "session-clean".to_string(),
                exec_session_id: "exec-clean-exit".to_string(),
                desired_enabled: true,
                status: BackgroundSubprocessStatus::Running,
                created_at,
                updated_at: created_at,
                started_at: Some(created_at),
                ended_at: None,
                pid: None,
                prompt: "demo".to_string(),
                summary: None,
                error: None,
                archive_path: None,
                transcript_path: None,
                max_turns: None,
                model_override: None,
                reasoning_override: None,
                restart_attempts: 0,
            },
        );
    }

    let entries = controller.refresh_background_processes().await.expect("refresh");
    let entry = entries.iter().find(|e| e.id == "background-clean").expect("clean entry");
    // Clean `exit 0` must surface as Stopped (matching `exited (0)`), not Error,
    // and must not consume the restart budget.
    assert_eq!(entry.status, BackgroundSubprocessStatus::Stopped);
    assert_eq!(entry.exit_code, Some(0));
    assert!(!entry.termination_requested);
    assert!(entry.error.is_none());
    assert!(!entry.desired_enabled);
    let state = controller.state.read().await;
    let record = state.background_children.get("background-clean").expect("record");
    assert_eq!(record.restart_attempts, 0);
}

#[tokio::test]
#[cfg(unix)]
async fn background_nonzero_exit_becomes_error_when_restore_disabled() {
    let temp = TempDir::new().expect("tempdir");
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    cfg.subagents.background.auto_restore = false;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    let workspace = controller.config.workspace_root.clone();
    controller
        .config
        .exec_sessions
        .create_pipe_session_with_sandbox_and_background(
            "exec-failing-exit".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "exit 1".to_string()],
            workspace,
            Default::default(),
            false,
            true,
        )
        .await
        .expect("failing exec session");
    wait_for_exec_exit(&controller.config.exec_sessions, "exec-failing-exit").await;

    let created_at = Utc::now();
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            "background-failing".to_string(),
            BackgroundRecord {
                exit_code: None,
                termination_requested: false,
                id: "background-failing".to_string(),
                agent_name: "demo".to_string(),
                display_label: "demo".to_string(),
                description: "demo".to_string(),
                source: "test".to_string(),
                color: None,
                session_id: "session-failing".to_string(),
                exec_session_id: "exec-failing-exit".to_string(),
                desired_enabled: true,
                status: BackgroundSubprocessStatus::Running,
                created_at,
                updated_at: created_at,
                started_at: Some(created_at),
                ended_at: None,
                pid: None,
                prompt: "demo".to_string(),
                summary: None,
                error: None,
                archive_path: None,
                transcript_path: None,
                max_turns: None,
                model_override: None,
                reasoning_override: None,
                restart_attempts: 0,
            },
        );
    }

    let entries = controller.refresh_background_processes().await.expect("refresh");
    let entry = entries.iter().find(|e| e.id == "background-failing").expect("failing entry");
    // Asymmetric oracle vs exit 0: non-zero must stay Error with diagnostic,
    // never collapse to a clean Stopped.
    assert_eq!(entry.status, BackgroundSubprocessStatus::Error);
    assert_eq!(entry.exit_code, Some(1));
    assert!(!entry.termination_requested);
    assert!(entry.error.as_deref().is_some_and(|e| e.contains('1')));
    assert!(entry.desired_enabled);
}

#[tokio::test]
#[cfg(unix)]
async fn background_graceful_stop_stays_stopped_while_process_drains() {
    let temp = TempDir::new().expect("tempdir");
    let mut cfg = VTCodeConfig::default();
    cfg.subagents.background.enabled = true;
    let controller = SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
        .await
        .expect("controller");

    let workspace = controller.config.workspace_root.clone();
    controller
        .config
        .exec_sessions
        .create_pipe_session_with_sandbox_and_background(
            "exec-long-running".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace,
            Default::default(),
            false,
            true,
        )
        .await
        .expect("long exec session");

    let created_at = Utc::now();
    {
        let mut state = controller.state.write().await;
        state.background_children.insert(
            "background-stopping".to_string(),
            BackgroundRecord {
                exit_code: None,
                termination_requested: false,
                id: "background-stopping".to_string(),
                agent_name: "demo".to_string(),
                display_label: "demo".to_string(),
                description: "demo".to_string(),
                source: "test".to_string(),
                color: None,
                session_id: "session-stopping".to_string(),
                exec_session_id: "exec-long-running".to_string(),
                // Graceful stop sets this optimistically before SIGTERM drains.
                desired_enabled: false,
                status: BackgroundSubprocessStatus::Stopped,
                created_at,
                updated_at: created_at,
                started_at: Some(created_at),
                ended_at: Some(created_at),
                pid: None,
                prompt: "demo".to_string(),
                summary: None,
                error: None,
                archive_path: None,
                transcript_path: None,
                max_turns: None,
                model_override: None,
                reasoning_override: None,
                restart_attempts: 0,
            },
        );
    }

    let entries = controller.refresh_background_processes().await.expect("refresh");
    // Must not resurrect to Running while the process still drains; TUI stays
    // non-interfering and the `Exited` arm finalizes later.
    let entry = entries.iter().find(|e| e.id == "background-stopping").expect("stopping entry");
    assert_eq!(entry.status, BackgroundSubprocessStatus::Stopped);
    let state = controller.state.read().await;
    let record = state.background_children.get("background-stopping").expect("record");
    assert_eq!(record.status, BackgroundSubprocessStatus::Stopped);
    assert!(!record.desired_enabled);

    controller.config.exec_sessions.close_session("exec-long-running").await.ok();
}

async fn wait_for_exec_exit(exec_sessions: &ExecSessionManager, session_id: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(snapshot) = exec_sessions.snapshot_session(session_id).await
                && snapshot.exit_code.is_some()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("exec session should exit");
}

#[test]
fn parse_verifier_decision_reads_explicit_decision_line() {
    assert_eq!(parse_verifier_decision("Looks fine.\nDecision: APPROVED"), Some(true));
    assert_eq!(parse_verifier_decision("**Decision:** REJECT\n"), Some(false));
    assert_eq!(parse_verifier_decision("- Decision: `approve`"), Some(true));
    assert_eq!(
        parse_verifier_decision("Decision: APPROVED\nno unsafe code was blocked"),
        Some(true),
        "prose keywords must not override the decision line"
    );
    assert_eq!(parse_verifier_decision("approved, no issues found"), None);
    assert_eq!(parse_verifier_decision("Decision: APPROVED."), Some(true));
}

#[test]
fn parse_verifier_decision_fails_closed_on_unclear_or_negated_value() {
    for summary in [
        "Decision: NOT APPROVED",
        "Decision: not approve",
        "**Decision:** Not Approved",
        "Decision: approved? no",
        "Decision: can't approve",
        "Decision: pending",
        "Decision:",
        "Decision: disapproved",
    ] {
        assert_eq!(parse_verifier_decision(summary), Some(false), "{summary:?}");
    }
    assert_eq!(
        parse_verifier_decision("Decision: APPROVED\nDecision: NOT APPROVED"),
        Some(false),
        "the last decision line wins"
    );
}

#[test]
fn heuristic_verifier_approval_rejects_negated_approval() {
    assert!(!heuristic_verifier_approval("The change is not approved.", &[]));
    assert!(!heuristic_verifier_approval("I do not approve this change", &[]));
    assert!(!heuristic_verifier_approval("Approved.", &["ISSUE: a.rs:1 bug".to_string()]));
    assert!(!heuristic_verifier_approval("Unclear; could not inspect the files.", &[]));
    assert!(heuristic_verifier_approval("Approved, no issues found.", &[]));
}

#[test]
fn extract_issues_reads_only_structured_issue_lines() {
    let summary = "- ISSUE: src/lib.rs:3 missing bounds check\n\
                   1. issue: src/a.rs:9 wrong default\n\
                   * **ISSUE:** src/b.rs:1 stale doc\n\
                   ISSUE: src/c.rs:2 unhandled error\n\
                   Reasoning: no error: all tests pass\n\
                   error: expected `;`, found `}` (quoted compiler output)\n\
                   - the problem: none\n\
                   Verdict mentions REJECT: only in prose\n\
                   - ISSUE:\n\
                   Decision: APPROVED";
    assert_eq!(
        extract_issues_from_summary(summary),
        vec![
            "ISSUE: src/lib.rs:3 missing bounds check".to_string(),
            "ISSUE: src/a.rs:9 wrong default".to_string(),
            "ISSUE: src/b.rs:1 stale doc".to_string(),
            "ISSUE: src/c.rs:2 unhandled error".to_string(),
        ]
    );
    assert!(extract_issues_from_summary("Reasoning: no error: all tests pass\nDecision: APPROVED").is_empty());
}
