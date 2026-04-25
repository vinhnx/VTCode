use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use vtcode_core::cli::args::BackgroundSubagentArgs;
use vtcode_core::subagents::{SpawnAgentRequest, SubagentController, SubagentControllerConfig};
use vtcode_core::tools::exec_session::ExecSessionManager;
use vtcode_core::tools::registry::PtySessionManager;

use crate::startup::StartupContext;

const BACKGROUND_DEMO_AGENT: &str = "background-demo";

pub(crate) async fn handle_background_subagent_command(
    startup: &StartupContext,
    args: BackgroundSubagentArgs,
) -> Result<()> {
    if args.prompt.trim().is_empty() {
        bail!("background subagent prompt cannot be empty");
    }

    if args.agent_name == BACKGROUND_DEMO_AGENT {
        return run_background_demo_subprocess(startup, &args).await;
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
        managed_background_runtime: true,
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

async fn run_background_demo_subprocess(
    startup: &StartupContext,
    args: &BackgroundSubagentArgs,
) -> Result<()> {
    let workspace_root = startup.agent_config.workspace.clone();
    let script_path = workspace_root.join("scripts/demo-background-subagent.sh");
    if !script_path.exists() {
        bail!(
            "background demo script not found at {}",
            script_path.display()
        );
    }

    let mut child = Command::new(&script_path)
        .current_dir(&workspace_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to start {}", script_path.display()))?;

    let stdout = child
        .stdout
        .take()
        .context("background demo subprocess did not expose stdout")?;
    let stderr = child
        .stderr
        .take()
        .context("background demo subprocess did not expose stderr")?;

    let mut stdout_lines = BufReader::new(stdout).lines();
    let readiness_line = tokio::time::timeout(Duration::from_secs(10), stdout_lines.next_line())
        .await
        .context("timed out waiting for background demo readiness")?
        .context("failed to read background demo readiness line")?
        .context("background demo subprocess exited before reporting readiness")?;

    println!("background-subagent-summary: {}", readiness_line.trim());
    println!(
        "background-subagent-ready: {} {}",
        args.agent_name, args.session_id
    );

    let stdout_task = tokio::spawn(async move {
        while let Some(line) = stdout_lines
            .next_line()
            .await
            .context("failed to read background demo stdout")?
        {
            println!("{line}");
        }
        Ok::<(), anyhow::Error>(())
    });

    let mut stderr_lines = BufReader::new(stderr).lines();
    let stderr_task = tokio::spawn(async move {
        while let Some(line) = stderr_lines
            .next_line()
            .await
            .context("failed to read background demo stderr")?
        {
            eprintln!("{line}");
        }
        Ok::<(), anyhow::Error>(())
    });

    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("failed to install SIGTERM handler for background demo")?;

    #[cfg(unix)]
    let shutdown_requested = async {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = sigterm.recv() => {},
        }
    };

    #[cfg(not(unix))]
    let shutdown_requested = tokio::signal::ctrl_c();

    tokio::select! {
        status = child.wait() => {
            let status = status.context("failed waiting for background demo subprocess")?;
            stdout_task.await.context("background demo stdout task panicked")??;
            stderr_task.await.context("background demo stderr task panicked")??;
            if status.success() {
                Ok(())
            } else {
                bail!("background demo subprocess exited with status {status}");
            }
        }
        _ = shutdown_requested => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            stdout_task.await.context("background demo stdout task panicked")??;
            stderr_task.await.context("background demo stderr task panicked")??;
            Ok(())
        }
    }
}
