use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::agent::runloop::unified::team_state::TeamState;

pub(crate) fn spawn_tmux_teammates(
    team_name: &str,
    workspace: &Path,
    team: &TeamState,
) -> Result<()> {
    ensure_tmux_available()?;

    for teammate in &team.config.teammates {
        spawn_tmux_teammate_inner(
            team_name,
            workspace,
            &teammate.name,
            teammate.model.as_deref(),
        )?;
    }

    tidy_tmux_layout()?;
    Ok(())
}

pub(crate) fn spawn_tmux_teammate(
    team_name: &str,
    workspace: &Path,
    teammate_name: &str,
    model: Option<&str>,
) -> Result<()> {
    ensure_tmux_available()?;
    spawn_tmux_teammate_inner(team_name, workspace, teammate_name, model)?;
    tidy_tmux_layout()?;
    Ok(())
}

fn ensure_tmux_available() -> Result<()> {
    if std::env::var("TMUX").is_err() {
        bail!("TMUX environment not detected. Run inside tmux or set teammate_mode to in_process.");
    }
    Ok(())
}

fn spawn_tmux_teammate_inner(
    team_name: &str,
    workspace: &Path,
    teammate_name: &str,
    model: Option<&str>,
) -> Result<()> {
    let exe = std::env::current_exe().context("Failed to resolve vtcode executable path")?;
    let mut parts = vec![
        exe.display().to_string(),
        "--team".to_string(),
        team_name.to_string(),
        "--teammate".to_string(),
        teammate_name.to_string(),
        "--team-role".to_string(),
        "teammate".to_string(),
        "--teammate-mode".to_string(),
        "tmux".to_string(),
        "--workspace".to_string(),
        workspace.display().to_string(),
    ];

    if let Some(model) = model {
        let trimmed = model.trim();
        if !trimmed.is_empty() {
            parts.push("--model".to_string());
            parts.push(trimmed.to_string());
        }
    }

    let command = shell_words::join(parts.iter().map(|part| part.as_str()));

    let status = Command::new("tmux")
        .arg("split-window")
        .arg("-d")
        .arg("-c")
        .arg(workspace)
        .arg(command)
        .status()
        .context("Failed to launch tmux split-window")?;

    if !status.success() {
        bail!("tmux split-window exited with status {}", status);
    }

    Ok(())
}

fn tidy_tmux_layout() -> Result<()> {
    let status = Command::new("tmux")
        .arg("select-layout")
        .arg("tiled")
        .status()
        .context("Failed to update tmux layout")?;

    if !status.success() {
        bail!("tmux select-layout exited with status {}", status);
    }

    Ok(())
}
