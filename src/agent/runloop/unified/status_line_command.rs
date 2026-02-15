use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command as TokioCommand;
use vtcode_core::config::StatusLineConfig;
use vtcode_core::utils::ansi_parser::strip_ansi;

use crate::agent::runloop::git::GitStatusSummary;

pub(super) async fn run_status_line_command(
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
        .filter(|line| !line.is_empty())
        .map(|line| strip_ansi(&line));

    Ok(first_line)
}

#[derive(Serialize)]
struct StatusLineCommandPayload {
    hook_event_name: &'static str,
    cwd: String,
    workspace: StatusLineWorkspace,
    model: StatusLineModel,
    runtime: StatusLineRuntime,
    context: Option<StatusLineContext>,
    git: Option<StatusLineGit>,
    version: &'static str,
}

#[derive(Serialize)]
struct StatusLineContext {
    utilization_percent: f64,
    total_tokens: usize,
    semantic_value_per_token: f64,
}

impl StatusLineCommandPayload {
    fn new(
        workspace: &Path,
        model_id: &str,
        model_display: &str,
        reasoning: &str,
        git: Option<&GitStatusSummary>,
    ) -> Self {
        Self::with_context(workspace, model_id, model_display, reasoning, git, None)
    }

    fn with_context(
        workspace: &Path,
        model_id: &str,
        model_display: &str,
        reasoning: &str,
        git: Option<&GitStatusSummary>,
        context: Option<StatusLineContext>,
    ) -> Self {
        let workspace_path = workspace.to_string_lossy().into_owned();
        Self {
            hook_event_name: "Status",
            cwd: workspace_path.clone(),
            workspace: StatusLineWorkspace {
                current_dir: workspace_path.clone(),
                project_dir: workspace_path,
            },
            model: StatusLineModel {
                id: model_id.to_string(),
                display_name: model_display.to_string(),
            },
            runtime: StatusLineRuntime {
                reasoning_effort: reasoning.to_string(),
            },
            context,
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

impl StatusLineContext {
    #[allow(dead_code)]
    pub(crate) fn from_efficiency(
        utilization_percent: f64,
        total_tokens: usize,
        semantic_value_per_token: f64,
    ) -> Self {
        Self {
            utilization_percent,
            total_tokens,
            semantic_value_per_token,
        }
    }
}
