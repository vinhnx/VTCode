use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use async_process::{Child, Command as AsyncCommand, ExitStatus, Stdio};
use futures::future::try_join;
use futures_lite::AsyncReadExt;
use tokio::sync::Mutex;
use tokio::time::{Sleep, sleep};
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

const DEFAULT_CAPTURE_LIMIT: usize = 256 * 1024; // 256 KiB

#[derive(Debug, Clone)]
pub struct StreamCaptureConfig {
    pub capture: bool,
    pub max_bytes: usize,
}

impl Default for StreamCaptureConfig {
    fn default() -> Self {
        Self {
            capture: true,
            max_bytes: DEFAULT_CAPTURE_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProcessOptions {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<OsString, OsString>,
    pub current_dir: Option<PathBuf>,
    pub timeout: Option<Duration>,
    pub cancellation_token: Option<CancellationToken>,
    pub stdout: StreamCaptureConfig,
    pub stderr: StreamCaptureConfig,
}

#[derive(Debug)]
pub struct ProcessOutput {
    pub exit_status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub duration: Duration,
}

pub struct AsyncProcessRunner;

impl AsyncProcessRunner {
    pub async fn run(options: ProcessOptions) -> Result<ProcessOutput> {
        if options.program.is_empty() {
            return Err(anyhow!("program cannot be empty"));
        }

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
            format!(
                "failed to spawn '{}' with args {:?}",
                options.program, options.args
            )
        })?;

        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();
        let shared_child = Arc::new(Mutex::new(child));

        let stdout_future = read_stream(stdout_handle, options.stdout);
        let stderr_future = read_stream(stderr_handle, options.stderr);
        let mut wait_future = Box::pin(wait_for_status(shared_child.clone()));
        let mut timeout_future = options
            .timeout
            .map(|dur| Box::pin(sleep(dur)) as Pin<Box<Sleep>>);
        let mut cancellation_future = options.cancellation_token.as_ref().map(|token| {
            Box::pin(token.clone().cancelled_owned()) as Pin<Box<WaitForCancellationFutureOwned>>
        });

        enum Completion {
            Finished,
            TimedOut,
            Cancelled,
        }

        let mut exit_status: Option<ExitStatus> = None;
        let completion = tokio::select! {
            res = &mut wait_future => {
                exit_status = Some(res?);
                Completion::Finished
            }
            _ = async {
                if let Some(fut) = timeout_future.as_mut() {
                    fut.as_mut().await;
                } else {
                    futures::future::pending::<()>().await;
                }
            }, if timeout_future.is_some() => Completion::TimedOut,
            _ = async {
                if let Some(fut) = cancellation_future.as_mut() {
                    fut.as_mut().await;
                } else {
                    futures::future::pending::<()>().await;
                }
            }, if cancellation_future.is_some() => Completion::Cancelled,
        };

        let (timed_out, cancelled, status) = match completion {
            Completion::Finished => (false, false, exit_status.expect("status captured")),
            Completion::TimedOut => {
                kill_child(shared_child.clone()).await?;
                let status = wait_future.await?;
                (true, false, status)
            }
            Completion::Cancelled => {
                kill_child(shared_child.clone()).await?;
                let status = wait_future.await?;
                (false, true, status)
            }
        };

        let (stdout, stderr) = try_join(stdout_future, stderr_future).await?;

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
    R: futures_lite::AsyncRead + Unpin,
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
    let mut guard = child.lock().await;
    let status = guard.status().await?;
    Ok(status)
}

async fn kill_child(child: Arc<Mutex<Child>>) -> Result<()> {
    let mut guard = child.lock().await;
    guard.kill()?;
    Ok(())
}
