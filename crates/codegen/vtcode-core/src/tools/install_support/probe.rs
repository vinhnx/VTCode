//! Bounded subprocess capture for synchronous search-tool availability checks.
//!
//! Deadline/cleanup reference: OpenAI Codex, Apache-2.0, copyright 2025 OpenAI,
//! `codex-rs/tui/src/tui/tmux.rs` at 5fff6378d6ab9730bc7cfbd5eb58f1867a5f14c0.
//! <https://github.com/openai/codex/blob/5fff6378d6ab9730bc7cfbd5eb58f1867a5f14c0/codex-rs/tui/src/tui/tmux.rs>
//! Independently implemented with bounded pipes instead of temporary output files.

use std::io;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};

const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const VERSION_PROBE_MAX_STREAM_BYTES: usize = 64 * 1024;

pub(crate) fn run_version_probe(command: Command) -> io::Result<Output> {
    run_probe_with_limits(command, VERSION_PROBE_TIMEOUT, VERSION_PROBE_MAX_STREAM_BYTES)
}

fn run_probe_with_limits(command: Command, timeout: Duration, max_stream_bytes: usize) -> io::Result<Output> {
    // Keep the synchronous API usable inside a Tokio runtime. The worker is
    // joined, and owns both the child and its bounded asynchronous pipe reads.
    std::thread::Builder::new()
        .name("vtcode-version-probe".to_owned())
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(capture_probe(command, timeout, max_stream_bytes))
        })?
        .join()
        .map_err(|panic_payload| {
            let message = panic_payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic_payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("non-string panic payload");
            io::Error::other(format!("version probe worker panicked: {message}"))
        })?
}

async fn capture_probe(command: Command, timeout: Duration, max_stream_bytes: usize) -> io::Result<Output> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut child = tokio::process::Command::from(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let capture = async {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing version probe stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("missing version probe stderr"))?;
        let (status, stdout, stderr) = tokio::try_join!(
            child.wait(),
            read_probe_stream(stdout, max_stream_bytes, "stdout"),
            read_probe_stream(stderr, max_stream_bytes, "stderr"),
        )?;
        Ok(Output { status, stdout, stderr })
    };
    let result = tokio::time::timeout_at(deadline, capture)
        .await
        .unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::TimedOut, "version probe timed out")));
    if result.is_err()
        && let Err(error) = child.kill().await
    {
        tracing::debug!(%error, "failed to clean up version probe child");
    }
    result
}

async fn read_probe_stream(
    reader: impl AsyncRead + Unpin,
    max_bytes: usize,
    stream: &'static str,
) -> io::Result<Vec<u8>> {
    let read_limit = u64::try_from(max_bytes.saturating_add(1)).unwrap_or(u64::MAX);
    let mut bytes = Vec::new();
    reader.take(read_limit).read_to_end(&mut bytes).await?;
    if bytes.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("version probe {stream} exceeded {max_bytes} bytes"),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
