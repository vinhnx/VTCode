//! Async line writer using a Tokio-based actor pattern.
//!
//! The actor owns a background task that performs file I/O via `spawn_blocking`.
//! The handle (`AsyncLineWriter`) sends messages through a bounded `mpsc` channel,
//! so the caller never blocks on I/O. When the handle is dropped, the channel closes
//! and the background task finishes its current write before exiting.
//!
//! This follows the "Actors with Tokio" pattern:
//! - Handle (`AsyncLineWriter`) is separate from the actor task.
//! - Handle is `Clone` (via `mpsc::Sender`).
//! - Graceful shutdown when all senders are dropped.
//! - Bounded channel for backpressure.

use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::spawn_blocking;
use tokio::time::{self, MissedTickBehavior};

use crate::config::constants::defaults;
use crate::utils::file_utils::ensure_dir_exists_sync;

enum LogMessage {
    Line { line: String, bytes: usize },
    Flush(oneshot::Sender<bool>),
}

#[derive(Default)]
struct Diagnostics {
    dropped_lines: AtomicUsize,
    dropped_bytes: AtomicUsize,
    write_failures: AtomicUsize,
}

/// Snapshot of best-effort exporter writer health.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
pub struct AsyncLineWriterDiagnostics {
    pub dropped_lines: usize,
    pub dropped_bytes: usize,
    pub write_failures: usize,
}

/// Asynchronous line writer that buffers writes on a background Tokio task.
///
/// The writer is `Clone`; all clones share the same background task. The task
/// exits when all handles are dropped.
#[derive(Clone)]
pub struct AsyncLineWriter {
    sender: mpsc::Sender<LogMessage>,
    reserved_bytes: Arc<AtomicUsize>,
    diagnostics: Arc<Diagnostics>,
    max_buffer_bytes: usize,
}

impl AsyncLineWriter {
    /// Create a new line writer that appends to `path`.
    ///
    /// Spawns a background actor task that owns the file handle and performs
    /// all I/O via `spawn_blocking`. The file is created eagerly if it does
    /// not exist.
    pub fn new(path: PathBuf) -> Result<Self> {
        Self::new_with_limits(
            path,
            defaults::DEFAULT_TRAJECTORY_LOG_CHANNEL_CAPACITY,
            defaults::DEFAULT_TRAJECTORY_LOG_BUFFER_BYTES,
            Duration::from_millis(defaults::DEFAULT_TRAJECTORY_LOG_FLUSH_INTERVAL_MS),
        )
    }

    /// Create a new line writer without performing synchronous setup on the
    /// async executor.
    pub async fn new_async(path: PathBuf) -> Result<Self> {
        Self::new_async_with_limits(
            path,
            defaults::DEFAULT_TRAJECTORY_LOG_CHANNEL_CAPACITY,
            defaults::DEFAULT_TRAJECTORY_LOG_BUFFER_BYTES,
            Duration::from_millis(defaults::DEFAULT_TRAJECTORY_LOG_FLUSH_INTERVAL_MS),
        )
        .await
    }

    fn new_with_limits(
        path: PathBuf,
        channel_capacity: usize,
        buffer_bytes: usize,
        flush_interval: Duration,
    ) -> Result<Self> {
        let runtime = tokio::runtime::Handle::try_current()
            .context("AsyncLineWriter::new requires an active Tokio runtime; use new_async or call within one")?;
        if let Some(parent) = path.parent() {
            ensure_dir_exists_sync(parent)
                .with_context(|| format!("Failed to create log directory: {}", parent.display()))?;
        }

        // Create the file eagerly so callers can observe it immediately.
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to create log file: {}", path.display()))?;

        Ok(Self::spawn_actor(&runtime, path, channel_capacity, buffer_bytes, flush_interval))
    }

    async fn new_async_with_limits(
        path: PathBuf,
        channel_capacity: usize,
        buffer_bytes: usize,
        flush_interval: Duration,
    ) -> Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("Failed to create log directory: {}", parent.display()))?;
        }

        // Create the file eagerly so callers can observe it immediately.
        tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("Failed to create log file: {}", path.display()))?;

        let runtime = tokio::runtime::Handle::current();
        Ok(Self::spawn_actor(&runtime, path, channel_capacity, buffer_bytes, flush_interval))
    }

    fn spawn_actor(
        runtime: &tokio::runtime::Handle,
        path: PathBuf,
        channel_capacity: usize,
        buffer_bytes: usize,
        flush_interval: Duration,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(channel_capacity.max(1));
        let reserved_bytes = Arc::new(AtomicUsize::new(0));
        let diagnostics = Arc::new(Diagnostics::default());

        // Spawn the actor task. It owns the file and runs the message loop.
        let actor_reserved_bytes = Arc::clone(&reserved_bytes);
        let actor_diagnostics = Arc::clone(&diagnostics);
        runtime.spawn(async move {
            actor_task(
                &path,
                receiver,
                actor_reserved_bytes,
                actor_diagnostics,
                channel_capacity.max(1),
                buffer_bytes,
                flush_interval,
            )
            .await;
        });

        Self {
            sender,
            reserved_bytes,
            diagnostics,
            max_buffer_bytes: buffer_bytes.max(1),
        }
    }

    /// Queue a line for writing. Drops the line if either bound is exhausted.
    pub fn write_line(&self, line: String) {
        let bytes = line.len().saturating_add(1);
        if !reserve_bytes(&self.reserved_bytes, bytes, self.max_buffer_bytes) {
            self.record_drop(bytes);
            return;
        }

        if self.sender.try_send(LogMessage::Line { line, bytes }).is_err() {
            self.reserved_bytes.fetch_sub(bytes, Ordering::AcqRel);
            self.record_drop(bytes);
        }
    }

    /// Flush pending writes and wait for completion.
    pub async fn flush(&self) {
        let _ = self.flush_result().await;
    }

    /// Flush pending writes and report whether the batch reached the file.
    ///
    /// [`Self::flush`] remains the compatibility API for best-effort callers;
    /// this companion method lets lifecycle code and diagnostics distinguish a
    /// successful drain from a closed actor or failed file write.
    pub async fn flush_result(&self) -> std::io::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(LogMessage::Flush(tx))
            .await
            .map_err(|_error| std::io::Error::other("trajectory writer actor closed"))?;
        match rx
            .await
            .map_err(|_error| std::io::Error::other("trajectory writer actor dropped flush acknowledgement"))?
        {
            true => Ok(()),
            false => Err(std::io::Error::other("trajectory writer failed to flush a batch")),
        }
    }

    #[allow(dead_code, reason = "Intentional compatibility, platform, or test-only suppression.")]
    pub fn diagnostics(&self) -> AsyncLineWriterDiagnostics {
        AsyncLineWriterDiagnostics {
            dropped_lines: self.diagnostics.dropped_lines.load(Ordering::Relaxed),
            dropped_bytes: self.diagnostics.dropped_bytes.load(Ordering::Relaxed),
            write_failures: self.diagnostics.write_failures.load(Ordering::Relaxed),
        }
    }

    fn record_drop(&self, bytes: usize) {
        self.diagnostics.dropped_lines.fetch_add(1, Ordering::Relaxed);
        self.diagnostics.dropped_bytes.fetch_add(bytes, Ordering::Relaxed);
        tracing::debug!(bytes, "dropping trajectory line because the writer is saturated");
    }
}

fn reserve_bytes(reserved_bytes: &AtomicUsize, bytes: usize, max_bytes: usize) -> bool {
    reserved_bytes
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_add(bytes).filter(|next| *next <= max_bytes)
        })
        .is_ok()
}

/// Background actor task: receives messages and writes to the file.
///
/// Runs until the channel is closed (all senders dropped). Uses
/// `spawn_blocking` for actual file I/O.
async fn actor_task(
    path: &Path,
    mut receiver: mpsc::Receiver<LogMessage>,
    reserved_bytes: Arc<AtomicUsize>,
    diagnostics: Arc<Diagnostics>,
    buffer_lines: usize,
    buffer_bytes: usize,
    flush_interval: Duration,
) {
    // Buffer messages until we need to flush or the channel closes.
    let mut buffer: Vec<(String, usize)> = Vec::new();
    let mut buffered_bytes = 0usize;
    let mut interval = time::interval(flush_interval.max(Duration::from_millis(1)));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            msg = receiver.recv() => match msg {
                Some(LogMessage::Line { line, bytes }) => {
                    buffered_bytes = buffered_bytes.saturating_add(bytes);
                    buffer.push((line, bytes));
                    if buffer.len() >= buffer_lines
                        || buffered_bytes >= buffer_bytes
                    {
                        let _ = flush_buffer(path, &mut buffer, &mut buffered_bytes, &reserved_bytes, &diagnostics).await;
                    }
                }
                Some(LogMessage::Flush(ack)) => {
                    let flushed = flush_buffer(path, &mut buffer, &mut buffered_bytes, &reserved_bytes, &diagnostics).await;
                    let _ = ack.send(flushed);
                }
                None => break,
            },
            _ = interval.tick() => {
                let _ = flush_buffer(path, &mut buffer, &mut buffered_bytes, &reserved_bytes, &diagnostics).await;
            },
        }
    }

    // Channel closed — flush any remaining buffered lines.
    let _ = flush_buffer(path, &mut buffer, &mut buffered_bytes, &reserved_bytes, &diagnostics).await;
}

async fn flush_buffer(
    path: &Path,
    buffer: &mut Vec<(String, usize)>,
    buffered_bytes: &mut usize,
    reserved_bytes: &AtomicUsize,
    diagnostics: &Diagnostics,
) -> bool {
    if buffer.is_empty() {
        return true;
    }
    let lines = std::mem::take(buffer);
    let line_count = lines.len();
    let bytes = *buffered_bytes;
    *buffered_bytes = 0;
    let written = flush_lines(path, lines).await;
    reserved_bytes.fetch_sub(bytes, Ordering::AcqRel);
    if !written {
        diagnostics.dropped_lines.fetch_add(line_count, Ordering::Relaxed);
        diagnostics.dropped_bytes.fetch_add(bytes, Ordering::Relaxed);
        diagnostics.write_failures.fetch_add(1, Ordering::Relaxed);
        tracing::warn!(line_count, bytes, "trajectory writer failed to flush a batch");
    }
    written
}

/// Flush buffered lines to the file using `spawn_blocking`.
async fn flush_lines(path: &Path, lines: Vec<(String, usize)>) -> bool {
    let path = path.to_path_buf();
    spawn_blocking(move || {
        let file = match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => file,
            Err(_) => return false,
        };
        let mut writer = BufWriter::new(file);
        for (line, _) in &lines {
            if writeln!(writer, "{line}").is_err() {
                return false;
            }
        }
        writer.flush().is_ok()
    })
    .await
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn drops_lines_when_byte_bound_is_saturated() {
        let temp_dir = TempDir::new().expect("temp directory");
        let writer =
            AsyncLineWriter::new_with_limits(temp_dir.path().join("trajectory.jsonl"), 4, 4, Duration::from_secs(60))
                .expect("writer");

        writer.write_line("1234".to_owned());
        writer.write_line("more data".to_owned());

        let diagnostics = writer.diagnostics();
        assert_eq!(diagnostics.dropped_lines, 2);
        assert_eq!(diagnostics.dropped_bytes, 15);
    }

    #[test]
    fn sync_constructor_without_runtime_errors_without_creating_file() {
        let temp_dir = TempDir::new().expect("temp directory");
        let path = temp_dir.path().join("nested").join("trajectory.jsonl");

        let result = AsyncLineWriter::new_with_limits(path.clone(), 4, 1024, Duration::from_secs(60));

        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn async_constructor_creates_parent_and_file() {
        let temp_dir = TempDir::new().expect("temp directory");
        let path = temp_dir.path().join("nested").join("trajectory.jsonl");
        let writer = AsyncLineWriter::new_async(path.clone()).await.expect("writer");

        assert!(path.exists());
        writer.write_line("async setup".to_owned());
        writer.flush().await;
        assert_eq!(fs::read_to_string(path).expect("read log"), "async setup\n");
    }

    #[tokio::test]
    async fn periodic_flush_writes_without_explicit_flush() {
        let temp_dir = TempDir::new().expect("temp directory");
        let path = temp_dir.path().join("trajectory.jsonl");
        let writer =
            AsyncLineWriter::new_with_limits(path.clone(), 4, 1024, Duration::from_millis(10)).expect("writer");

        writer.write_line("periodic".to_owned());
        time::sleep(Duration::from_millis(40)).await;

        assert_eq!(fs::read_to_string(path).expect("read log"), "periodic\n");
    }

    #[tokio::test]
    async fn channel_shutdown_flushes_pending_lines() {
        let temp_dir = TempDir::new().expect("temp directory");
        let path = temp_dir.path().join("trajectory.jsonl");
        {
            let writer =
                AsyncLineWriter::new_with_limits(path.clone(), 4, 1024, Duration::from_secs(60)).expect("writer");
            writer.write_line("final".to_owned());
        }

        for _ in 0..20 {
            if fs::read_to_string(&path).expect("read log") == "final\n" {
                return;
            }
            time::sleep(Duration::from_millis(5)).await;
        }
        panic!("actor did not flush before shutdown");
    }

    #[tokio::test]
    async fn flush_result_reports_file_write_failure() {
        let temp_dir = TempDir::new().expect("temp directory");
        let path = temp_dir.path().join("trajectory.jsonl");
        let writer = AsyncLineWriter::new_with_limits(path, 4, 1024, Duration::from_secs(60)).expect("writer");

        fs::remove_dir_all(temp_dir.path()).expect("remove temporary directory");
        writer.write_line("unwritable".to_owned());

        assert!(writer.flush_result().await.is_err());
        let diagnostics = writer.diagnostics();
        assert_eq!(diagnostics.write_failures, 1);
        assert_eq!(diagnostics.dropped_lines, 1);
        assert_eq!(diagnostics.dropped_bytes, "unwritable\n".len());
    }
}
