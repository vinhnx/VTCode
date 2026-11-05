pub(super) use super::controller_spawn_run::finalize_background_launch;
pub(super) use super::*;
pub(super) use crate::config::constants::models;
pub(super) use crate::config::constants::tools;
pub(super) use crate::config::models::{ModelId, Provider};
pub(super) use crate::llm::provider::ToolDefinition;
pub(super) use crate::tools::exec_session::ExecSessionManager;
pub(super) use crate::tools::registry::PtySessionManager;
pub(super) use anyhow::{Result, anyhow};
pub(super) use std::collections::BTreeMap;
pub(super) use std::collections::VecDeque;
pub(super) use std::path::PathBuf;
pub(super) use std::sync::Arc;
pub(super) use std::time::Duration;
pub(super) use tempfile::TempDir;
pub(super) use tokio::sync::Notify;
pub(super) use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
pub(super) use vtcode_config::{
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
        closing: false,
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

mod background;
mod background_exit;
mod capacity_nesting;
mod child_config;
mod child_tools;
mod model_resolution;
mod shutdown_resume;
mod spawn;
mod spawn_custom_close;
mod verifier;
