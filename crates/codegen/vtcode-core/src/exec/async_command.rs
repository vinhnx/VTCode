use hashbrown::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use async_process::{Child, Command as AsyncCommand, ExitStatus, Stdio};

use futures::io::AsyncReadExt;
use tokio::sync::Mutex;
use tokio::time::{Sleep, sleep, timeout};
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

use crate::telemetry::perf;
use crate::utils::gatekeeper;

const DEFAULT_CAPTURE_LIMIT: usize = 256 * 1024; // 256 KiB

/// Upper bound on how long `run` will wait, after killing the direct child on
/// the `TimedOut`/`Cancelled` paths, for (a) the child to be reaped and (b) the
/// stdout/stderr pipes to drain.
///
/// Killing the direct child does not guarantee its descendants exit: a
/// grandchild process (for example a background server, a Python
/// `subprocess.Popen`, or a `multiprocessing` worker) may have inherited the
/// stdout/stderr pipe file descriptors and can keep the write end open after
/// the direct child is gone. In that case the pipe never reaches EOF and an
/// unbounded drain would block `run` forever, silently defeating the tool's
/// own timeout/cancellation guarantee. Bounding the wait ensures `run` always
/// returns; whatever bytes were already captured before the bound elapses are
/// returned as-is, and any data written afterward is accepted as lost since
/// the process has already been killed.
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
    /// On timeout or cancellation, only the direct child is killed (this
    /// crate's `async-process` dependency does not expose creating the child
    /// in its own process group without resorting to unsafe `pre_exec`
    /// hooks, unlike the `libc`-based group-kill used by
    /// `vtcode-bash-runner`), so any grandchild process the child spawned may
    /// survive and keep holding the stdout/stderr pipes open. The post-kill
    /// stream drain and process reap are therefore bounded by
    /// `POST_KILL_DRAIN_TIMEOUT` so this function can never hang.
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
        let mut command = AsyncCommand::new(&options.program);
        command.args(&options.args);
        if let Some(dir) = &options.current_dir {
            command.current_dir(dir);
        }
        if !options.env.is_empty() {
            command.envs(&options.env);
        }
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let mut child = command.spawn().with_context(|| {
            format!("failed to spawn '{}' with args {:?}", options.program, options.args)
        })?;

        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();
        let shared_child = Arc::new(Mutex::new(child));

        let mut stdout_future = Box::pin(read_stream(stdout_handle, options.stdout));
        let mut stderr_future = Box::pin(read_stream(stderr_handle, options.stderr));
        let mut wait_future = Box::pin(wait_for_status(shared_child.clone()));
        let mut timeout_future = options.timeout.map(|dur| Box::pin(sleep(dur)) as Pin<Box<Sleep>>);
        let mut cancellation_future = options.cancellation_token.as_ref().map(|token| {
            Box::pin(token.clone().cancelled_owned()) as Pin<Box<WaitForCancellationFutureOwned>>
        });

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
                res = &mut wait_future, if exit_status.is_none() => {
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

        // Once the process has been killed on the TimedOut/Cancelled paths, both
        // reaping the child and draining its stdout/stderr pipes must be bounded:
        // a surviving grandchild can hold the pipes open indefinitely, and (much
        // more rarely) a wedged wait could stall too. The Finished path keeps its
        // original, unbounded behavior because EOF and exit status are guaranteed
        // once the process has exited normally.
        let bounded_drain = matches!(completion, Completion::TimedOut | Completion::Cancelled);

        let (timed_out, cancelled, status) = match completion {
            Completion::Finished => {
                let status = match exit_status {
                    Some(status) => status,
                    None => wait_future.await?,
                };
                (false, false, status)
            }
            Completion::TimedOut => {
                kill_child(shared_child.clone()).await?;
                let status = match exit_status {
                    Some(status) => status,
                    None => match timeout(POST_KILL_DRAIN_TIMEOUT, wait_future.as_mut()).await {
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
                kill_child(shared_child.clone()).await?;
                let status = match exit_status {
                    Some(status) => status,
                    None => match timeout(POST_KILL_DRAIN_TIMEOUT, wait_future.as_mut()).await {
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
        // still-pending stream is bounded: a killed process may have left a
        // grandchild holding the pipe's write end open, so we accept whatever
        // was captured so far rather than blocking forever.
        let stdout = match stdout_result {
            Some(Ok(data)) => data,
            Some(Err(e)) => return Err(e),
            None if bounded_drain => {
                match timeout(POST_KILL_DRAIN_TIMEOUT, stdout_future.as_mut()).await {
                    Ok(res) => res?,
                    Err(_) => Vec::new(),
                }
            }
            None => stdout_future.await?,
        };
        let stderr = match stderr_result {
            Some(Ok(data)) => data,
            Some(Err(e)) => return Err(e),
            None if bounded_drain => {
                match timeout(POST_KILL_DRAIN_TIMEOUT, stderr_future.as_mut()).await {
                    Ok(res) => res?,
                    Err(_) => Vec::new(),
                }
            }
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
    R: futures::io::AsyncRead + Unpin,
{
    if !config.capture {
        return Ok(Vec::new());
    }

    let mut reader = match reader {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };

    let mut output = Vec::new();
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

async fn wait_for_status(child: Arc<Mutex<Child>>) -> Result<ExitStatus> {
    // `Child::status` only needs `&mut Child` momentarily to build its returned
    // future; the future itself does not borrow the child afterward (it holds
    // its own clone of async-process's internal handle). The mutex guard is
    // therefore taken only long enough to obtain that future and is dropped
    // before awaiting it. Holding the guard across the await instead would
    // deadlock `kill_child`, which also needs this same mutex: it would never
    // be able to acquire the lock to kill the process while this future is
    // pending on exit, and the process cannot exit until it is killed.
    let status_future = {
        let mut guard = child.lock().await;
        guard.status()
    };
    let status = status_future.await?;
    Ok(status)
}

async fn kill_child(child: Arc<Mutex<Child>>) -> Result<()> {
    let mut guard = child.lock().await;
    guard.kill()?;
    Ok(())
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
    /// direct child (killed on timeout) must not block `run` forever: the
    /// bounded drain must let the call return, accepting a partial/empty
    /// stdout capture instead of hanging.
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
