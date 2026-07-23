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

use tokio::sync::{mpsc, oneshot};
use tokio::task::spawn_blocking;

use crate::config::constants::defaults;
use crate::utils::file_utils::ensure_dir_exists_sync;

enum LogMessage {
    Line(String),
    Flush(oneshot::Sender<()>),
}

/// Asynchronous line writer that buffers writes on a background Tokio task.
///
/// The writer is `Clone`; all clones share the same background task. The task
/// exits when all handles are dropped.
#[derive(Clone)]
pub struct AsyncLineWriter {
    sender: mpsc::Sender<LogMessage>,
}

impl AsyncLineWriter {
    /// Create a new line writer that appends to `path`.
    ///
    /// Spawns a background actor task that owns the file handle and performs
    /// all I/O via `spawn_blocking`. The file is created eagerly if it does
    /// not exist.
    pub fn new(path: PathBuf) -> Result<Self> {
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

        let (sender, receiver) = mpsc::channel(defaults::DEFAULT_TRAJECTORY_LOG_CHANNEL_CAPACITY);

        // Spawn the actor task. It owns the file and runs the message loop.
        tokio::spawn(async move {
            actor_task(&path, receiver).await;
        });

        Ok(Self { sender })
    }

    /// Queue a line for writing. Drops the line if the channel is full.
    pub fn write_line(&self, line: String) {
        let _ = self.sender.try_send(LogMessage::Line(line));
    }

    /// Flush pending writes and wait for completion.
    pub async fn flush(&self) {
        let (tx, rx) = oneshot::channel();
        if self.sender.send(LogMessage::Flush(tx)).await.is_ok() {
            let _ = rx.await;
        }
    }
}

/// Background actor task: receives messages and writes to the file.
///
/// Runs until the channel is closed (all senders dropped). Uses
/// `spawn_blocking` for actual file I/O.
async fn actor_task(path: &Path, mut receiver: mpsc::Receiver<LogMessage>) {
    // Buffer messages until we need to flush or the channel closes.
    let mut buffer: Vec<String> = Vec::new();

    while let Some(msg) = receiver.recv().await {
        match msg {
            LogMessage::Line(line) => {
                buffer.push(line);
            }
            LogMessage::Flush(ack) => {
                if !buffer.is_empty() {
                    flush_lines(path, &buffer).await;
                    buffer.clear();
                }
                let _ = ack.send(());
            }
        }
    }

    // Channel closed — flush any remaining buffered lines.
    if !buffer.is_empty() {
        flush_lines(path, &buffer).await;
    }
}

/// Flush buffered lines to the file using `spawn_blocking`.
async fn flush_lines(path: &Path, lines: &[String]) {
    let path = path.to_path_buf();
    let lines: Vec<String> = lines.to_vec();
    let _ = spawn_blocking(move || {
        let file = match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => file,
            Err(_) => return,
        };
        let mut writer = BufWriter::new(file);
        for line in &lines {
            let _ = writeln!(writer, "{line}");
        }
        let _ = writer.flush();
    })
    .await;
}
