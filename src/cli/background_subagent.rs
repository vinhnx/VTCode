use std::time::Duration;

use anyhow::{Result, bail};
use vtcode_core::cli::args::BackgroundSubagentArgs;
use vtcode_core::subagents::{SpawnAgentRequest, SubagentController, SubagentControllerConfig};
use vtcode_core::tools::exec_session::ExecSessionManager;
use vtcode_core::tools::registry::PtySessionManager;

use crate::startup::StartupContext;

pub(crate) async fn handle_background_subagent_command(
    startup: &StartupContext,
    args: BackgroundSubagentArgs,
) -> Result<()> {
    if args.prompt.trim().is_empty() {
        bail!("background subagent prompt cannot be empty");
    }

    let workspace_root = startup.agent_config.workspace.clone();
    let mut vt_cfg = startup.config.clone();
    vt_cfg.subagents.background.auto_restore = false;

    let pty_sessions = PtySessionManager::new(workspace_root.clone(), vt_cfg.pty.clone());
    let exec_sessions = ExecSessionManager::new(workspace_root.clone(), pty_sessions.clone());
    let controller = SubagentController::new(SubagentControllerConfig {
        workspace_root,
        parent_session_id: args.parent_session_id,
        parent_model: startup.agent_config.model.clone(),
        parent_provider: startup.agent_config.provider.clone(),
        parent_reasoning_effort: startup.agent_config.reasoning_effort,
        api_key: startup.agent_config.api_key.clone(),
        vt_cfg,
        openai_chatgpt_auth: startup.agent_config.openai_chatgpt_auth.clone(),
        depth: 0,
        exec_sessions,
        pty_manager: pty_sessions.manager().clone(),
    })
    .await?;

    controller
        .set_turn_delegation_hints_from_input("delegate this task to a background subagent")
        .await;
    controller
        .set_parent_session_id(args.session_id.clone())
        .await;

    let spawned = controller
        .spawn(SpawnAgentRequest {
            agent_type: Some(args.agent_name.clone()),
            message: Some(args.prompt.clone()),
            background: true,
            max_turns: args.max_turns,
            model: args.model_override.clone(),
            reasoning_effort: args.reasoning_override.clone(),
            ..SpawnAgentRequest::default()
        })
        .await?;

    let status = controller
        .wait(std::slice::from_ref(&spawned.id), Some(300_000))
        .await?;
    if let Some(status) = status {
        if let Some(summary) = status.summary.as_deref() {
            println!("background-subagent-summary: {}", summary.trim());
        }
        if let Some(error) = status.error.as_deref() {
            eprintln!("background-subagent-error: {}", error.trim());
        }
        if !status.status.is_terminal()
            || matches!(
                status.status,
                vtcode_core::subagents::SubagentStatus::Completed
            )
        {
            println!(
                "background-subagent-ready: {} {}",
                status.agent_name, status.session_id
            );
        } else {
            bail!(
                "background subagent '{}' exited with status {}",
                status.agent_name,
                status.status.as_str()
            );
        }
    } else {
        println!(
            "background-subagent-ready: {} {}",
            args.agent_name, args.session_id
        );
    }

    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}
