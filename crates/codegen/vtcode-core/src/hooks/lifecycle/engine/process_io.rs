use std::io::ErrorKind;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::time;

use super::HookCommandResult;
#[cfg(unix)]
use super::process_guard::HookProcessGroupGuard;
use crate::hooks::lifecycle::interpret::HookCaptureFailure;

// Descendants can keep a pipe open after the direct hook exits.
const HOOK_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) async fn capture_hook_output(
    mut child: Child,
    payload: &Value,
    timeout_seconds: u64,
) -> Result<HookCommandResult> {
    #[cfg(unix)]
    let mut group_guard = HookProcessGroupGuard::new(child.id());
    let stdin = child.stdin.take();
    let mut stdout_pipe = child.stdout.take().context("lifecycle hook missing stdout pipe")?;
    let mut stderr_pipe = child.stderr.take().context("lifecycle hook missing stderr pipe")?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let timed_out;
    let exit_code;
    let mut stdout_result = None;
    let mut stderr_result = None;
    {
        // Keep readers owned by this call so cancellation drops the pipes and
        // bytes read before the deadline remain available to the interpreter.
        let drain = async {
            tokio::join!(async { stdout_result = Some(stdout_pipe.read_to_end(&mut stdout).await) }, async {
                stderr_result = Some(stderr_pipe.read_to_end(&mut stderr).await)
            },);
        };
        tokio::pin!(drain);
        let execution = async {
            // Reap only once stdin is closed. If the write times out, the
            // unreaped child still owns the process-group leader's PID.
            send_payload(stdin, payload).await?;
            child.wait().await.context("failed to wait for lifecycle hook")
        };
        let (wait_result, drained) = {
            let wait = time::timeout(Duration::from_secs(timeout_seconds), execution);
            tokio::pin!(wait);
            tokio::select! {
                result = &mut wait => (result, false),
                () = &mut drain => (wait.await, true),
            }
        };

        (exit_code, timed_out) = match wait_result {
            Ok(status) => {
                let status = status?;
                #[cfg(unix)]
                group_guard.disarm();
                (status.code(), false)
            }
            Err(_) => {
                #[cfg(unix)]
                group_guard.kill();
                #[cfg(not(unix))]
                let _ = child.start_kill();
                (None, true)
            }
        };

        let reap = async {
            if timed_out {
                let _ = child.wait().await;
            }
        };
        let finish = async {
            if !drained {
                drain.await;
            }
        };
        let _ = time::timeout(HOOK_DRAIN_TIMEOUT, async { tokio::join!(reap, finish) }).await;
    }

    let capture_failure = if timed_out {
        Some(HookCaptureFailure::ExecutionTimeout(timeout_seconds))
    } else {
        match stdout_result {
            Some(Ok(_)) => None,
            Some(Err(error)) => Some(HookCaptureFailure::StdoutRead(error)),
            None => Some(HookCaptureFailure::StdoutTimeout),
        }
    };
    if let Some(Err(error)) = stderr_result {
        tracing::debug!(%error, "could not read complete lifecycle hook stderr");
    }
    Ok(HookCommandResult {
        exit_code,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        capture_failure,
    })
}

async fn send_payload(stdin: Option<ChildStdin>, payload: &Value) -> Result<()> {
    let Some(mut stdin) = stdin else {
        return Ok(());
    };
    let mut bytes = serde_json::to_vec(payload).context("failed to serialize lifecycle hook payload")?;
    bytes.push(b'\n');
    // Hooks can legitimately exit without consuming their payload.
    match stdin.write_all(&bytes).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error).context("failed to write lifecycle hook payload"),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::Stdio;
    use tokio::process::Command;

    fn spawn_hook(command: &str) -> Child {
        Command::new("sh")
            .args(["-c", command])
            .process_group(0)
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn fixture")
    }

    #[tokio::test]
    async fn capture_keeps_output_when_a_background_job_keeps_a_pipe_open() {
        let child = spawn_hook("(sleep 4; printf late) & printf early; printf diagnostic >&2");
        let result = capture_hook_output(child, &Value::Null, 5).await.expect("hook result");
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout, "early");
        assert_eq!(result.stderr, "diagnostic");
        assert!(matches!(result.capture_failure, Some(HookCaptureFailure::StdoutTimeout)));
        let mut outcome = crate::hooks::lifecycle::types::PreToolHookOutcome::default();
        crate::hooks::lifecycle::interpret::interpret_pre_tool(
            &crate::config::HookCommandConfig {
                kind: Default::default(),
                command: "fixture".into(),
                timeout_seconds: None,
            },
            &result,
            &mut outcome,
            false,
        );
        assert!(matches!(outcome.decision, crate::hooks::lifecycle::types::PreToolHookDecision::Deny));
    }

    #[tokio::test]
    async fn capture_accepts_complete_stdout_when_only_stderr_stays_open() {
        let child = spawn_hook("(sleep 4) >&2 & printf complete");
        let result = capture_hook_output(child, &Value::Null, 5).await.expect("hook result");
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout, "complete");
        assert!(result.capture_failure.is_none());
    }

    #[tokio::test]
    async fn capture_times_out_a_hook_that_does_not_read_its_payload() {
        let child = spawn_hook("printf started; sleep 4");
        let payload = Value::String("x".repeat(1024 * 1024));
        let result = capture_hook_output(child, &payload, 1).await.expect("hook result");
        assert!(matches!(result.capture_failure, Some(HookCaptureFailure::ExecutionTimeout(1))));
        assert_eq!(result.exit_code, None);
        assert_eq!(result.stdout, "started");
    }
}
