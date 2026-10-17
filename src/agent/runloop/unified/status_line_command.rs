use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command as TokioCommand;
use vtcode_core::config::StatusLineConfig;
use vtcode_core::tools::dominant_workspace_language;
use vtcode_core::utils::ansi_parser::strip_ansi;

use crate::agent::runloop::git::GitStatusSummary;

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
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
        .with_context(|| format!("failed to spawn status line command `{command}`"))?;

    let mut stdout_pipe = child.stdout.take().context("status line command missing stdout pipe")?;

    if let Some(mut stdin) = child.stdin.take() {
        let payload = StatusLineCommandPayload::new(workspace, model_id, model_display, reasoning, git);
        let mut payload_bytes = serde_json::to_vec(&payload).context("failed to serialize status line payload")?;
        payload_bytes.push(b'\n');

        stdin
            .write_all(&payload_bytes)
            .await
            .or_else(handle_status_stdin_error)
            .context("failed to write status line payload")?;
        stdin
            .shutdown()
            .await
            .or_else(handle_status_stdin_error)
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
        Ok(status_res) => status_res.with_context(|| format!("failed to wait for status line command `{command}`"))?,
        Err(_) => {
            child
                .start_kill()
                .with_context(|| format!("failed to kill timed out status line command `{command}`"))?;
            child
                .wait()
                .await
                .with_context(|| format!("failed to wait for killed status line command `{command}` after timeout"))?;
            return Err(anyhow!("status line command `{command}` timed out after {timeout_ms}ms"));
        }
    };

    let mut stdout_bytes = Vec::new();
    stdout_pipe
        .read_to_end(&mut stdout_bytes)
        .await
        .context("failed to read status line command stdout")?;

    if !status.success() {
        return Err(anyhow!("status line command exited with status {status}"));
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

fn handle_status_stdin_error(error: std::io::Error) -> std::io::Result<()> {
    // Commands may ignore the optional JSON input. Their exit status and
    // timeout still determine success after the pipe has closed.
    if error.kind() == std::io::ErrorKind::BrokenPipe {
        Ok(())
    } else {
        Err(error)
    }
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
                dominant_language: dominant_workspace_language(workspace),
                active_language: dominant_workspace_language(workspace),
            },
            model: StatusLineModel {
                id: model_id.to_string(),
                display_name: model_display.to_string(),
            },
            runtime: StatusLineRuntime { reasoning_effort: reasoning.to_string() },
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
    dominant_language: Option<String>,
    active_language: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::{StatusLineCommandPayload, handle_status_stdin_error, run_status_line_command};
    use serde_json::Value;
    use serial_test::serial;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn custom_command_may_ignore_json_input_without_hiding_process_failures() {
        let workspace = TempDir::new().unwrap();
        let config = vtcode_core::config::StatusLineConfig { command_timeout_ms: 2_000, ..Default::default() };
        // Exceed the pipe capacity so closing stdin cannot race a completed write.
        let model_id = "m".repeat(2 * 1024 * 1024);
        let output = run_status_line_command(
            "exec 0<&-; printf 'custom status\\n'",
            workspace.path(),
            &model_id,
            "Model",
            "low",
            None,
            &config,
        )
        .await
        .unwrap();
        assert_eq!(output.as_deref(), Some("custom status"));
        let output =
            run_status_line_command("exec 0<&-; exit 0", workspace.path(), &model_id, "Model", "low", None, &config)
                .await
                .unwrap();
        assert!(output.is_none());
        let error =
            run_status_line_command("exec 0<&-; exit 7", workspace.path(), &model_id, "Model", "low", None, &config)
                .await
                .unwrap_err();
        assert!(error.to_string().contains("exited with status"), "{error:#}");
        let config = vtcode_core::config::StatusLineConfig { command_timeout_ms: 20, ..Default::default() };
        let error = run_status_line_command(
            "exec 0<&-; exec sleep 1",
            workspace.path(),
            &model_id,
            "Model",
            "low",
            None,
            &config,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("timed out after 20ms"), "{error:#}");
    }

    #[test]
    fn status_stdin_tolerates_only_broken_pipe_errors() {
        assert!(handle_status_stdin_error(std::io::Error::from(std::io::ErrorKind::BrokenPipe)).is_ok());
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::ConnectionReset,
        ] {
            let error = handle_status_stdin_error(std::io::Error::from(kind)).unwrap_err();
            assert_eq!(error.kind(), kind);
        }
    }

    #[tokio::test]
    async fn custom_commands_can_still_read_the_json_payload() {
        let workspace = TempDir::new().unwrap();
        let config = vtcode_core::config::StatusLineConfig { command_timeout_ms: 2_000, ..Default::default() };
        let output = run_status_line_command(
            "cat > status-payload.json; printf 'payload read\\n'",
            workspace.path(),
            "fixture-model",
            "Fixture Model",
            "high",
            None,
            &config,
        )
        .await
        .unwrap();
        assert_eq!(output.as_deref(), Some("payload read"));
        let value: Value =
            serde_json::from_str(&fs::read_to_string(workspace.path().join("status-payload.json")).unwrap()).unwrap();
        assert_eq!(value["hook_event_name"], "Status");
        assert_eq!(value["model"]["id"], "fixture-model");
        assert_eq!(value["runtime"]["reasoning_effort"], "high");
    }

    #[test]
    #[serial]
    fn payload_includes_dominant_workspace_language() {
        let workspace = TempDir::new().expect("workspace tempdir");
        fs::create_dir_all(workspace.path().join("src")).expect("create src");
        fs::write(workspace.path().join("src/lib.rs"), "fn alpha() {}\n").expect("write rust");

        let payload = StatusLineCommandPayload::new(workspace.path(), "model", "Model", "low", None);
        let value = serde_json::to_value(payload).expect("serialize payload");

        assert_eq!(value["workspace"]["dominant_language"], Value::String("Rust".to_string()));
        assert_eq!(value["workspace"]["active_language"], Value::String("Rust".to_string()));
    }
}
