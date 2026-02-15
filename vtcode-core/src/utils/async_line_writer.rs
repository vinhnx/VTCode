use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread;

use crate::config::constants::defaults;
use crate::utils::file_utils::ensure_dir_exists_sync;

enum LogMessage {
    Line(String),
    Flush(SyncSender<()>),
}

/// Asynchronous line writer that buffers writes on a background thread.
pub struct AsyncLineWriter {
    sender: SyncSender<LogMessage>,
    _handle: thread::JoinHandle<()>,
}

impl AsyncLineWriter {
    pub fn new(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            ensure_dir_exists_sync(parent)
                .with_context(|| format!("Failed to create log directory: {}", parent.display()))?;
        }

        let (sender, receiver) = sync_channel(defaults::DEFAULT_TRAJECTORY_LOG_CHANNEL_CAPACITY);
        let handle = thread::spawn(move || writer_loop(&path, receiver));

        Ok(Self {
            sender,
            _handle: handle,
        })
    }

    /// Queue a line for writing. Drops the line if the queue is full.
    pub fn write_line(&self, line: String) {
        let _ = self.sender.try_send(LogMessage::Line(line));
    }

    /// Flush pending writes and wait for completion.
    pub fn flush(&self) {
        let (tx, rx) = sync_channel(1);
        let _ = self.sender.send(LogMessage::Flush(tx));
        let _ = rx.recv();
    }
}

fn writer_loop(path: &Path, receiver: Receiver<LogMessage>) {
    let file = OpenOptions::new().create(true).append(true).open(path);
    let mut writer = match file {
        Ok(file) => BufWriter::new(file),
        Err(_) => return,
    };

    while let Ok(message) = receiver.recv() {
        match message {
            LogMessage::Line(line) => {
                let _ = writeln!(writer, "{line}");
            }
            LogMessage::Flush(ack) => {
                let _ = writer.flush();
                let _ = ack.send(());
            }
        }
    }

    let _ = writer.flush();
}
