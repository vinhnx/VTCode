use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use async_process::{Command, Stdio};
use futures::io::{AsyncReadExt, AsyncWriteExt};

use crate::config::constants::shell;

/// Description of a process to execute asynchronously.
pub struct ProcessRequest<'a> {
    /// Binary or script to execute.
    pub program: &'a str,
    /// Arguments passed to the program.
    pub args: &'a [String],
    /// Human-readable representation of the command, used for diagnostics.
    pub display: &'a str,
    /// Optional working directory for the process.
    pub current_dir: Option<&'a Path>,
    /// Timeout applied to the command execution.
    pub timeout: Duration,
    /// Optional stdin payload to feed into the command.
    pub stdin: Option<&'a [u8]>,
    /// Maximum number of stdout bytes to retain.
    pub max_stdout_bytes: usize,
    /// Maximum number of stderr bytes to retain.
    pub max_stderr_bytes: usize,
}

/// Output collected from a finished process.
pub struct ProcessOutput {
    /// Whether the process exited successfully.
    pub success: bool,
    /// Exit code returned by the process. `-1` indicates termination without a code.
    pub exit_code: i32,
    /// Captured stdout data.
    pub stdout: String,
    /// Captured stderr data.
    pub stderr: String,
    /// Number of stdout bytes captured before UTF-8 conversion.
    pub stdout_bytes: usize,
    /// Number of stderr bytes captured before UTF-8 conversion.
    pub stderr_bytes: usize,
    /// Whether stdout was truncated due to capture limits.
    pub stdout_truncated: bool,
    /// Whether stderr was truncated due to capture limits.
    pub stderr_truncated: bool,
    /// Duration between process spawn and completion/termination.
    pub duration: Duration,
    /// True when the process exceeded the configured timeout and was terminated.
    pub timed_out: bool,
}

/// Execute a command asynchronously using the runtime-agnostic `async-process` crate.
pub async fn run_process(request: ProcessRequest<'_>) -> Result<ProcessOutput> {
    let mut command = Command::new(request.program);
    if !request.args.is_empty() {
        command.args(request.args);
    }
    if let Some(dir) = request.current_dir {
        command.current_dir(dir);
    }
    if request.stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let display = if request.display.is_empty() {
        format_command_label(request.program, request.args)
    } else {
        request.display.to_string()
    };

    let start = Instant::now();
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn command '{}'", display))?;

    if let Some(payload) = request.stdin {
        if !payload.is_empty() {
            if let Some(mut stdin) = child.stdin.take() {
                AsyncWriteExt::write_all(&mut stdin, payload)
                    .await
                    .with_context(|| format!("failed to write stdin for command '{}'", display))?;
                AsyncWriteExt::flush(&mut stdin)
                    .await
                    .with_context(|| format!("failed to flush stdin for command '{}'", display))?;
            }
        }
    }

    let stdout = child
        .stdout
        .take()
        .context("command stdout pipe not configured")?;
    let stderr = child
        .stderr
        .take()
        .context("command stderr pipe not configured")?;

    let stdout_future = capture_stream(stdout, request.max_stdout_bytes, "stdout", &display);
    let stderr_future = capture_stream(stderr, request.max_stderr_bytes, "stderr", &display);
    let wait_future = wait_for_child(&mut child, request.timeout, &display);

    let (stdout_result, stderr_result, wait_result) =
        tokio::join!(stdout_future, stderr_future, wait_future);

    let stdout_capture = stdout_result
        .with_context(|| format!("failed to capture stdout for command '{}'", display))?;
    let stderr_capture = stderr_result
        .with_context(|| format!("failed to capture stderr for command '{}'", display))?;
    let (status, timed_out) = wait_result?;

    Ok(ProcessOutput {
        success: status.success(),
        exit_code: status.code().unwrap_or(-1),
        stdout: stdout_capture.text,
        stderr: stderr_capture.text,
        stdout_bytes: stdout_capture.bytes,
        stderr_bytes: stderr_capture.bytes,
        stdout_truncated: stdout_capture.truncated,
        stderr_truncated: stderr_capture.truncated,
        duration: start.elapsed(),
        timed_out,
    })
}

fn format_command_label(program: &str, args: &[String]) -> String {
    if args.is_empty() {
        return program.to_string();
    }

    let mut segments = Vec::with_capacity(args.len() + 1);
    segments.push(program.to_string());
    segments.extend(args.iter().cloned());
    segments.join(" ")
}

struct StreamCapture {
    text: String,
    bytes: usize,
    truncated: bool,
}

async fn capture_stream<R>(
    mut reader: R,
    max_bytes: usize,
    stream_name: &'static str,
    command_label: &str,
) -> Result<StreamCapture>
where
    R: futures::io::AsyncRead + Unpin,
{
    let mut buffer = Vec::new();
    let mut truncated = false;
    let mut chunk = vec![0u8; shell::STREAM_CAPTURE_CHUNK_SIZE];

    loop {
        let read = reader.read(&mut chunk).await.with_context(|| {
            format!(
                "failed to read {} for command '{}'",
                stream_name, command_label
            )
        })?;
        if read == 0 {
            break;
        }

        if max_bytes == 0 {
            truncated = true;
            continue;
        }

        let remaining = max_bytes.saturating_sub(buffer.len());
        if remaining == 0 {
            truncated = true;
            continue;
        }

        let to_take = remaining.min(read);
        buffer.extend_from_slice(&chunk[..to_take]);
        if to_take < read {
            truncated = true;
        }
    }

    Ok(StreamCapture {
        bytes: buffer.len(),
        truncated,
        text: String::from_utf8_lossy(&buffer).to_string(),
    })
}

async fn wait_for_child(
    child: &mut async_process::Child,
    timeout: Duration,
    display: &str,
) -> Result<(std::process::ExitStatus, bool)> {
    let mut status_future = Box::pin(child.status());
    match tokio::time::timeout(timeout, &mut status_future).await {
        Ok(status_result) => {
            let status = status_result
                .with_context(|| format!("failed to wait for command '{}'", display))?;
            Ok((status, false))
        }
        Err(_) => {
            drop(status_future);
            child.kill().with_context(|| {
                format!("failed to terminate command '{}' after timeout", display)
            })?;
            let status = child.status().await.with_context(|| {
                format!("failed to retrieve exit status for command '{}'", display)
            })?;
            Ok((status, true))
        }
    }
}
