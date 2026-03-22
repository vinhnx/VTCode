//! Shared buffered trace log writer with flush-on-exit support.
//!
//! Provides a `BufWriter`-backed file writer wrapped in `Arc<Mutex<..>>` so the
//! tracing `fmt::layer` can write efficiently (batched syscalls) while still
//! allowing an explicit `flush_trace_log()` call on process exit or signal.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use anyhow::{Context, Result};

/// Capacity of the internal `BufWriter` (64 KiB — large enough to batch many
/// log lines before issuing a single `write` syscall).
const BUF_CAPACITY: usize = 64 * 1024;

/// Global handle to the active trace log writer so `flush_trace_log` can be
/// called from signal handlers / shutdown hooks without threading the writer
/// through the entire call stack.
static GLOBAL_WRITER: OnceLock<FlushableWriter> = OnceLock::new();

/// A clonable, thread-safe buffered writer that implements `std::io::Write`
/// so it can be passed directly to `tracing_subscriber::fmt::layer().with_writer(..)`.
#[derive(Clone)]
pub struct FlushableWriter {
    inner: Arc<Mutex<BufWriter<File>>>,
}

impl FlushableWriter {
    /// Open (or create) a log file and wrap it in a buffered writer.
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("Failed to open trace log file: {}", path.display()))?;
        let writer = BufWriter::with_capacity(BUF_CAPACITY, file);
        let flushable = Self {
            inner: Arc::new(Mutex::new(writer)),
        };
        // Store globally so `flush_trace_log` works from anywhere.
        let _ = GLOBAL_WRITER.set(flushable.clone());
        // Register the flush hook in vtcode-commons so crates that don't
        // depend on vtcode-core (e.g. vtcode-tui) can still trigger a flush.
        vtcode_commons::trace_flush::register_trace_flush_hook(flush_trace_log);
        Ok(flushable)
    }

    /// Flush the internal buffer to disk.
    pub fn flush(&self) {
        let _ = self.flush_locked();
    }

    fn lock_writer(&self) -> std::io::Result<MutexGuard<'_, BufWriter<File>>> {
        self.inner
            .lock()
            .map_err(|_| std::io::Error::other("trace writer lock poisoned"))
    }

    fn flush_locked(&self) -> std::io::Result<()> {
        let mut guard = self.lock_writer()?;
        guard.flush()
    }
}

impl Write for FlushableWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut guard = self.lock_writer()?;
        guard.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flush_locked()
    }
}

/// Flush the global trace log writer to disk.
///
/// Safe to call from signal handlers, shutdown hooks, or `Drop` implementations.
/// No-op if no trace writer has been initialized.
pub fn flush_trace_log() {
    if let Some(writer) = GLOBAL_WRITER.get() {
        writer.flush();
    }
}
