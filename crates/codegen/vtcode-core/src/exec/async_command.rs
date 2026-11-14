use hashbrown::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use std::process::{ExitStatus, Stdio};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::time::{Sleep, sleep, timeout};
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

use crate::telemetry::perf;
use crate::utils::gatekeeper;

const DEFAULT_CAPTURE_LIMIT: usize = 256 * 1024; // 256 KiB

/// Upper bound on how long `run` will wait, after killing the direct child on
/// the `TimedOut`/`Cancelled` paths, for (a) the child to be reaped and (b) the
/// stdout/stderr pipes to drain.
///
/// The timeout/cancellation kill targets the child's whole process group, so
/// descendants that inherited the group die with it. Two rare cases still
/// justify the bound: a grandchild that called `setsid()` escapes the group and
/// can keep the stdout/stderr pipe write ends open, and a wedged reap could
/// stall indefinitely. Bounding the wait ensures `run` always returns; whatever
/// bytes were already captured before the bound elapses are returned as-is,
/// and any data written afterward is accepted as lost since the process has
/// already been killed.
const POST_KILL_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Configuration for capturing a process stream (stdout or stderr).
#[derive(Debug, Clone)]
pub struct StreamCaptureConfig {
    /// Whether to capture this stream.
    pub capture: bool,
    /// Maximum number of bytes to capture before truncating.
    pub max_bytes: usize,
}

impl Default for StreamCaptureConfig {
    fn default() -> Self {
        Self { capture: true, max_bytes: DEFAULT_CAPTURE_LIMIT }
    }
}

/// Options for spawning an asynchronous process.
#[derive(Debug, Clone, Default)]
pub struct ProcessOptions {
    /// The program to execute.
    pub program: String,
    /// Arguments to pass to the program.
    pub args: Vec<String>,
    /// Environment variables for the process.
    pub env: HashMap<OsString, OsString>,
    /// Working directory for the process.
    pub current_dir: Option<PathBuf>,
    /// Maximum time the process is allowed to run before being killed.
    pub timeout: Option<Duration>,
    /// Token to externally cancel the process.
    pub cancellation_token: Option<CancellationToken>,
    /// Configuration for stdout capture.
    pub stdout: StreamCaptureConfig,
    /// Configuration for stderr capture.
    pub stderr: StreamCaptureConfig,
}

/// Output from a completed asynchronous process.
#[derive(Debug)]
pub struct ProcessOutput {
    /// The exit status of the process.
    pub exit_status: ExitStatus,
    /// Captured stdout bytes.
    pub stdout: Vec<u8>,
    /// Captured stderr bytes.
    pub stderr: Vec<u8>,
    /// Whether the process was killed due to a timeout.
    pub timed_out: bool,
    /// Whether the process was cancelled via its cancellation token.
    pub cancelled: bool,
    /// Wall-clock duration from spawn to completion.
    pub duration: Duration,
}

/// Runs a child process asynchronously with timeout and cancellation support.
pub struct AsyncProcessRunner;

impl AsyncProcessRunner {
    /// Spawn and await a child process with the given options.
    ///
    /// On Unix the child starts in its own process group, so the timeout and
    /// cancellation kills terminate the child *and* any descendant that
    /// inherited the group (see `kill_child_process_group`). The post-kill
    /// stream drain and process reap stay bounded by `POST_KILL_DRAIN_TIMEOUT`
    /// so this function can never hang.
    pub async fn run(options: ProcessOptions) -> Result<ProcessOutput> {
        if options.program.is_empty() {
            return Err(anyhow!("program cannot be empty"));
        }

        let mut tags = HashMap::new();
        tags.insert("subsystem".to_string(), "async_command".to_string());
        tags.insert("program".to_string(), options.program.clone());
        perf::record_value("vtcode.perf.spawn_count", 1.0, tags);

        gatekeeper::check_quarantine_for_program(&options.program);

        let start = Instant::now();
        let mut command = Command::new(&options.program);
        command.args(&options.args);
        if let Some(dir) = &options.current_dir {
            command.current_dir(dir);
        }
        if !options.env.is_empty() {
            command.envs(&options.env);
        }
        // Own process group (PGID == PID) on Unix: the kill path can then reach
        // descendants via killpg instead of only the direct child.
        #[cfg(unix)]
        command.process_group(0);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .with_context(|| format!("failed to spawn '{}' with args {:?}", options.program, options.args))?;

        let child_pid = child.id();
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        let mut stdout_future = Box::pin(read_stream(stdout_handle, options.stdout));
        let mut stderr_future = Box::pin(read_stream(stderr_handle, options.stderr));
        let mut timeout_future = options.timeout.map(|dur| Box::pin(sleep(dur)) as Pin<Box<Sleep>>);
        let mut cancellation_future = options
            .cancellation_token
            .as_ref()
            .map(|token| Box::pin(token.clone().cancelled_owned()) as Pin<Box<WaitForCancellationFutureOwned>>);

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Completion {
            Finished,
            TimedOut,
            Cancelled,
        }

        let mut exit_status: Option<ExitStatus> = None;
        let mut stdout_result: Option<Result<Vec<u8>>> = None;
        let mut stderr_result: Option<Result<Vec<u8>>> = None;

        let completion = loop {
            tokio::select! {
                res = child.wait(), if exit_status.is_none() => {
                    exit_status = Some(res?);
                    // Continue to drain streams
                }
                res = &mut stdout_future, if stdout_result.is_none() => {
                    stdout_result = Some(res);
                }
                res = &mut stderr_future, if stderr_result.is_none() => {
                    stderr_result = Some(res);
                }
                _ = async {
                    if let Some(fut) = timeout_future.as_mut() {
                        fut.as_mut().await;
                    } else {
                        futures::future::pending::<()>().await;
                    }
                }, if timeout_future.is_some() => {
                    break Completion::TimedOut;
                }
                _ = async {
                    if let Some(fut) = cancellation_future.as_mut() {
                        fut.as_mut().await;
                    } else {
                        futures::future::pending::<()>().await;
                    }
                }, if cancellation_future.is_some() => {
                    break Completion::Cancelled;
                }
            }

            // Check if everything is done
            if exit_status.is_some() && stdout_result.is_some() && stderr_result.is_some() {
                break Completion::Finished;
            }
        };

        // Once the process has been killed on the TimedOut/Cancelled paths,
        // reaping the child and draining its stdout/stderr pipes must be
        // bounded (see POST_KILL_DRAIN_TIMEOUT). The Finished path keeps its
        // original, unbounded behavior because EOF and exit status are
        // guaranteed once the process has exited normally.
        let bounded_drain = matches!(completion, Completion::TimedOut | Completion::Cancelled);

        let (timed_out, cancelled, status) = match completion {
            Completion::Finished => {
                let status = match exit_status {
                    Some(status) => status,
                    None => child.wait().await?,
                };
                (false, false, status)
            }
            Completion::TimedOut => {
                // Skip the kill entirely when the child already exited: the
                // PID may have been reaped (or even reused), so signaling it
                // is both unnecessary and unsafe.
                if exit_status.is_none() {
                    kill_child_process_group(child_pid, &mut child)
                        .await
                        .with_context(|| format!("failed to kill timed-out process '{}'", options.program))?;
                }
                let status = match exit_status {
                    Some(status) => status,
                    None => match timeout(POST_KILL_DRAIN_TIMEOUT, child.wait()).await {
                        Ok(res) => res?,
                        Err(_) => {
                            return Err(anyhow!(
                                "process timed out and was killed, but could not be reaped within {POST_KILL_DRAIN_TIMEOUT:?}"
                            ));
                        }
                    },
                };
                (true, false, status)
            }
            Completion::Cancelled => {
                // Same skip rationale as the timeout path above.
                if exit_status.is_none() {
                    kill_child_process_group(child_pid, &mut child)
                        .await
                        .with_context(|| format!("failed to kill cancelled process '{}'", options.program))?;
                }
                let status = match exit_status {
                    Some(status) => status,
                    None => match timeout(POST_KILL_DRAIN_TIMEOUT, child.wait()).await {
                        Ok(res) => res?,
                        Err(_) => {
                            return Err(anyhow!(
                                "process was cancelled and killed, but could not be reaped within {POST_KILL_DRAIN_TIMEOUT:?}"
                            ));
                        }
                    },
                };
                (false, true, status)
            }
        };

        // Ensure streams are fully read. On timeout/cancellation the drain of any
        // still-pending stream is bounded: a setsid-escaping descendant may still
        // hold the pipe's write end open, so we accept whatever was captured so
        // far rather than blocking forever.
        let stdout = match stdout_result {
            Some(Ok(data)) => data,
            Some(Err(e)) => return Err(e),
            None if bounded_drain => match timeout(POST_KILL_DRAIN_TIMEOUT, stdout_future.as_mut()).await {
                Ok(res) => res?,
                Err(_) => Vec::new(),
            },
            None => stdout_future.await?,
        };
        let stderr = match stderr_result {
            Some(Ok(data)) => data,
            Some(Err(e)) => return Err(e),
            None if bounded_drain => match timeout(POST_KILL_DRAIN_TIMEOUT, stderr_future.as_mut()).await {
                Ok(res) => res?,
                Err(_) => Vec::new(),
            },
            None => stderr_future.await?,
        };

        Ok(ProcessOutput {
            exit_status: status,
            stdout,
            stderr,
            timed_out,
            cancelled,
            duration: start.elapsed(),
        })
    }
}

async fn read_stream<R>(reader: Option<R>, config: StreamCaptureConfig) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    if !config.capture {
        return Ok(Vec::new());
    }

    let mut reader = match reader {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let mut output = Vec::new();
    // Reserve a bounded amount of capacity based on the configured capture
    // limit. We cap the reservation to DEFAULT_CAPTURE_LIMIT to avoid
    // reserving an unbounded or attacker-controlled amount while still
    // improving performance for typical capture sizes.
    let reserve_cap = config.max_bytes.min(DEFAULT_CAPTURE_LIMIT);
    if reserve_cap > 0 {
        output.reserve(reserve_cap);
    }

    let mut buffer = [0u8; 4096];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = config.max_bytes.saturating_sub(output.len());
        if remaining > 0 {
            let to_copy = remaining.min(read);
            output.extend_from_slice(&buffer[..to_copy]);
        }
    }

    Ok(output)
}

/// Kill the spawned child on the timeout/cancellation path.
///
/// On Unix the child was created in its own process group, so the
/// `vtcode-bash-runner` group kill terminates descendants too; the helper
/// SIGKILLs the direct child as a fallback and treats an already-exited child
/// as success. Other platforms kill the direct child only, matching the
/// previous behavior.
async fn kill_child_process_group(child_pid: Option<u32>, child: &mut Child) -> Result<()> {
    #[cfg(unix)]
    if let Some(pid) = child_pid {
        return vtcode_bash_runner::process_group::kill_process_group_by_pid(pid)
            .context("failed to kill process group");
    }
    let _ = child_pid;
    child.kill().await.context("failed to kill child process")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A timed-out run must return promptly with `timed_out == true` instead
    /// of hanging on the post-kill stream drain. The whole call is wrapped in
    /// a generous outer `tokio::time::timeout` so a regression to the old,
    /// unbounded-drain behavior fails the test instead of hanging the suite.
    #[tokio::test]
    async fn run_returns_promptly_after_timeout_kill() {
        let options = ProcessOptions {
            program: "sleep".to_string(),
            args: vec!["30".to_string()],
            timeout: Some(Duration::from_millis(100)),
            ..Default::default()
        };

        let outcome = timeout(Duration::from_secs(10), AsyncProcessRunner::run(options))
            .await
            .expect("run() must return well within the outer bound instead of hanging")
            .expect("timed-out run should still yield a ProcessOutput, not an error");

        assert!(outcome.timed_out, "expected timed_out to be true");
        assert!(!outcome.cancelled, "cancelled must remain false on timeout");
    }

    /// Same guarantee as above, but for the cancellation path: a caller
    /// cancelling a long-running process must get a prompt return with
    /// `cancelled == true` rather than an indefinite hang.
    #[tokio::test]
    async fn run_returns_promptly_after_cancellation_kill() {
        let token = CancellationToken::new();
        let cancel_token = token.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(100)).await;
            cancel_token.cancel();
        });

        let options = ProcessOptions {
            program: "sleep".to_string(),
            args: vec!["30".to_string()],
            cancellation_token: Some(token),
            ..Default::default()
        };

        let outcome = timeout(Duration::from_secs(10), AsyncProcessRunner::run(options))
            .await
            .expect("run() must return well within the outer bound instead of hanging")
            .expect("cancelled run should still yield a ProcessOutput, not an error");

        assert!(outcome.cancelled, "expected cancelled to be true");
        assert!(!outcome.timed_out, "timed_out must remain false on cancellation");
    }

    /// A grandchild process that inherits the stdout pipe and outlives the
    /// direct child (killed on timeout) must not block `run` forever. On Unix
    /// the group kill now terminates the grandchild itself, so the drain
    /// completes normally; on platforms without process groups the bounded
    /// drain lets the call return with a partial capture. Either way the call
    /// must return promptly with `timed_out == true`.
    #[tokio::test]
    async fn run_bounds_drain_when_grandchild_holds_pipe_open() {
        let options = ProcessOptions {
            program: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                // The backgrounded `sleep` inherits stdout and keeps the pipe's
                // write end open well past the parent shell's own timeout-driven
                // death, simulating a surviving grandchild.
                "sleep 30 & exit 0".to_string(),
            ],
            timeout: Some(Duration::from_millis(100)),
            ..Default::default()
        };

        let outcome = timeout(Duration::from_secs(10), AsyncProcessRunner::run(options))
            .await
            .expect("run() must return well within the outer bound instead of hanging")
            .expect("run should still yield a ProcessOutput even with a surviving grandchild");

        assert!(outcome.timed_out, "expected timed_out to be true");
    }
}
