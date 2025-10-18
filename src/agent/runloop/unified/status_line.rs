use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command as TokioCommand;

use serde::Serialize;

use vtcode_core::config::{StatusLineConfig, StatusLineMode};
use vtcode_core::models::ModelId;
use vtcode_core::ui::tui::InlineHandle;

use crate::agent::runloop::git::{GitStatusSummary, git_status_summary};

#[derive(Default, Clone)]
pub(crate) struct InputStatusState {
    pub(crate) left: Option<String>,
    pub(crate) right: Option<String>,
    pub(crate) git_left: Option<String>,
    pub(crate) git_summary: Option<GitStatusSummary>,
    pub(crate) last_git_refresh: Option<Instant>,
    pub(crate) command_value: Option<String>,
    pub(crate) last_command_refresh: Option<Instant>,
}

const GIT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

pub(crate) async fn update_input_status_if_changed(
    handle: &InlineHandle,
    workspace: &Path,
    model: &str,
    reasoning: &str,
    status_config: Option<&StatusLineConfig>,
    state: &mut InputStatusState,
) -> Result<()> {
    let mode = status_config
        .map(|cfg| cfg.mode)
        .unwrap_or(StatusLineMode::Auto);

    if matches!(mode, StatusLineMode::Hidden) {
        state.last_git_refresh = None;
        state.git_left = None;
        state.git_summary = None;
    } else {
        let should_refresh_git = match state.last_git_refresh {
            Some(last_refresh) => last_refresh.elapsed() >= GIT_STATUS_REFRESH_INTERVAL,
            None => true,
        };

        if should_refresh_git {
            match git_status_summary(workspace) {
                Ok(Some(summary)) => {
                    let mut branch = summary.branch.clone();
                    if summary.dirty {
                        branch.push('*');
                    }
                    state.git_left = Some(branch);
                    state.git_summary = Some(summary);
                }
                Ok(None) => {
                    state.git_left = None;
                    state.git_summary = None;
                }
                Err(error) => {
                    tracing::warn!(
                        workspace = %workspace.display(),
                        error = ?error,
                        "Failed to resolve git status"
                    );
                    state.git_summary = None;
                }
            }

            state.last_git_refresh = Some(Instant::now());
        }
    }

    let trimmed_model = model.trim();
    let trimmed_reasoning = reasoning.trim();
    let model_display = ModelId::from_str(trimmed_model)
        .map(|id| id.display_name().to_string())
        .unwrap_or_else(|_| trimmed_model.to_string());

    let mut command_error: Option<anyhow::Error> = None;

    let (left, right) = match mode {
        StatusLineMode::Hidden => {
            state.command_value = None;
            state.last_command_refresh = None;
            (None, None)
        }
        StatusLineMode::Auto => {
            let right = build_model_status_right(trimmed_model, trimmed_reasoning);
            (state.git_left.clone(), right)
        }
        StatusLineMode::Command => {
            if let Some(cfg) = status_config {
                if let Some(command) = cfg
                    .command
                    .as_ref()
                    .map(|cmd| cmd.trim().to_string())
                    .filter(|cmd| !cmd.is_empty())
                {
                    let refresh_interval = Duration::from_millis(cfg.refresh_interval_ms);
                    let should_refresh_command = match state.last_command_refresh {
                        Some(last_refresh) => {
                            if refresh_interval.is_zero() {
                                true
                            } else {
                                last_refresh.elapsed() >= refresh_interval
                            }
                        }
                        None => true,
                    };

                    if should_refresh_command {
                        state.last_command_refresh = Some(Instant::now());
                        match run_status_line_command(
                            &command,
                            workspace,
                            trimmed_model,
                            &model_display,
                            trimmed_reasoning,
                            state.git_summary.as_ref(),
                            cfg,
                        )
                        .await
                        {
                            Ok(output) => {
                                state.command_value = output;
                            }
                            Err(error) => {
                                command_error = Some(error);
                            }
                        }
                    }

                    (state.command_value.clone(), None)
                } else {
                    state.command_value = None;
                    let right = build_model_status_right(trimmed_model, trimmed_reasoning);
                    (state.git_left.clone(), right)
                }
            } else {
                state.command_value = None;
                let right = build_model_status_right(trimmed_model, trimmed_reasoning);
                (state.git_left.clone(), right)
            }
        }
    };

    if state.left != left || state.right != right {
        handle.set_input_status(left.clone(), right.clone());
        state.left = left;
        state.right = right;
    }

    if let Some(error) = command_error {
        Err(error)
    } else {
        Ok(())
    }
}

fn build_model_status_right(model: &str, reasoning: &str) -> Option<String> {
    if model.is_empty() {
        None
    } else if reasoning.is_empty() {
        Some(model.to_string())
    } else {
        Some(format!("{} ({})", model, reasoning))
    }
}

async fn run_status_line_command(
    command: &str,
    workspace: &Path,
    model_id: &str,
    model_display: &str,
    reasoning: &str,
    git: Option<&GitStatusSummary>,
    config: &StatusLineConfig,
) -> Result<Option<String>> {
    let mut process = TokioCommand::new("sh");
    process.arg("-c").arg(command);
    process.current_dir(workspace);
    process.stdin(std::process::Stdio::piped());
    process.stdout(std::process::Stdio::piped());
    process.stderr(std::process::Stdio::null());

    let mut child = process
        .spawn()
        .with_context(|| format!("failed to spawn status line command `{}`", command))?;

    let mut stdout_pipe = child
        .stdout
        .take()
        .context("status line command missing stdout pipe")?;

    if let Some(mut stdin) = child.stdin.take() {
        let payload =
            StatusLineCommandPayload::new(workspace, model_id, model_display, reasoning, git);
        let mut payload_bytes =
            serde_json::to_vec(&payload).context("failed to serialize status line payload")?;
        payload_bytes.push(b'\n');

        stdin
            .write_all(&payload_bytes)
            .await
            .context("failed to write status line payload")?;
        stdin
            .shutdown()
            .await
            .context("failed to close status line command stdin")?;
    }

    let timeout_ms = std::cmp::max(config.command_timeout_ms, 1);
    let timeout_duration = Duration::from_millis(timeout_ms);
    let wait_result = {
        let wait = child.wait();
        tokio::pin!(wait);
        tokio::time::timeout(timeout_duration, &mut wait).await
    };

    let status = match wait_result {
        Ok(status_res) => status_res
            .with_context(|| format!("failed to wait for status line command `{}`", command))?,
        Err(_) => {
            child.start_kill().with_context(|| {
                format!("failed to kill timed out status line command `{}`", command)
            })?;
            child.wait().await.with_context(|| {
                format!(
                    "failed to wait for killed status line command `{}` after timeout",
                    command
                )
            })?;
            return Err(anyhow!(
                "status line command `{}` timed out after {}ms",
                command,
                timeout_ms
            ));
        }
    };

    let mut stdout_bytes = Vec::new();
    stdout_pipe
        .read_to_end(&mut stdout_bytes)
        .await
        .context("failed to read status line command stdout")?;

    if !status.success() {
        return Err(anyhow!("status line command exited with status {}", status));
    }

    let stdout = String::from_utf8_lossy(&stdout_bytes);
    let first_line = stdout
        .lines()
        .next()
        .map(|line| line.trim_end().to_string())
        .filter(|line| !line.is_empty());

    Ok(first_line)
}

#[derive(Serialize)]
struct StatusLineCommandPayload {
    hook_event_name: &'static str,
    cwd: String,
    workspace: StatusLineWorkspace,
    model: StatusLineModel,
    runtime: StatusLineRuntime,
    git: Option<StatusLineGit>,
    version: &'static str,
}

impl StatusLineCommandPayload {
    fn new(
        workspace: &Path,
        model_id: &str,
        model_display: &str,
        reasoning: &str,
        git: Option<&GitStatusSummary>,
    ) -> Self {
        let workspace_path = workspace.to_string_lossy().to_string();
        Self {
            hook_event_name: "Status",
            cwd: workspace_path.clone(),
            workspace: StatusLineWorkspace {
                current_dir: workspace_path.clone(),
                project_dir: workspace_path.clone(),
            },
            model: StatusLineModel {
                id: model_id.to_string(),
                display_name: model_display.to_string(),
            },
            runtime: StatusLineRuntime {
                reasoning_effort: reasoning.to_string(),
            },
            git: git.map(StatusLineGit::from_summary),
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[derive(Serialize)]
struct StatusLineWorkspace {
    current_dir: String,
    project_dir: String,
}

#[derive(Serialize)]
struct StatusLineModel {
    id: String,
    display_name: String,
}

#[derive(Serialize)]
struct StatusLineRuntime {
    reasoning_effort: String,
}

#[derive(Serialize)]
struct StatusLineGit {
    branch: String,
    dirty: bool,
}

impl StatusLineGit {
    fn from_summary(summary: &GitStatusSummary) -> Self {
        Self {
            branch: summary.branch.clone(),
            dirty: summary.dirty,
        }
    }
}
