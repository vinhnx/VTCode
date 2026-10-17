//! Unified process handle types for PTY and pipe backends.
//!
//! This module provides abstractions for interacting with spawned processes
//! regardless of whether they use a PTY or regular pipes.
//!
//! Inspired by [codex-rs] PTY process handle patterns (Apache-2.0).
//! Copyright 2025 OpenAI. See the repository `THIRD-PARTY-NOTICES` file for
//! full attribution.
//!
//! [codex-rs]: https://github.com/openai/codex

use std::fmt;
use std::io;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::{AbortHandle, JoinHandle};

const POST_EXIT_DRAIN_QUIET_MS: u64 = 50;
const POST_EXIT_DRAIN_MAX_MS: u64 = 500;

/// Trait for process termination strategies.
///
/// Different backends (PTY vs pipe) may need different termination approaches.
pub trait ChildTerminator: Send + Sync {
    /// Kill the child process.
    fn kill(&mut self) -> io::Result<()>;
}

/// Keep-alive guard for PTY master/slave handles.
///
/// This is a marker trait for opaque OS handles (e.g. `portable-pty` pair
/// halves) whose only contract is ownership: dropping the handle releases the
/// underlying resource. It exists so `PtyHandles` can name its vtable instead
/// of erasing to bare `dyn Send` (which carries an empty vtable and documents
/// no intent).
///
/// Memory layout note: `Box<dyn PtyHandle>` is a wide pointer (data pointer +
/// vtable pointer, 16 bytes on 64-bit). There is one vtable per concrete
/// handle type, emitted as external static data and paired with the object at
/// the construction site — Rust chooses dynamic dispatch at the call site, so
/// storing the concrete handle type directly (instead of boxing) would use
/// static dispatch. Boxing is justified here only because PTY backends are
/// selected at runtime and their handle types are heterogeneous.
///
/// The blanket implementation covers every `Send` handle, so existing backends
/// can wrap their concrete handle with `Box::new(handle) as Box<dyn PtyHandle>`
/// without additional work.
pub trait PtyHandle: Send {}

impl<T: Send> PtyHandle for T {}

/// Optional PTY-specific handles that must be preserved.
///
/// For PTY processes, the slave handle must be kept alive because the process
/// will receive SIGHUP if it's closed.
pub struct PtyHandles {
    /// The slave PTY handle (kept alive to prevent SIGHUP).
    pub _slave: Option<Box<dyn PtyHandle>>,
    /// The master PTY handle.
    pub _master: Box<dyn PtyHandle>,
}

impl fmt::Debug for PtyHandles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PtyHandles").finish()
    }
}

/// Handle for driving an interactive or non-interactive process.
///
/// This provides a unified interface for both PTY and pipe-based processes:
/// - Write to stdin via `writer_sender()`
/// - Read merged stdout/stderr via `output_receiver()`
/// - Check exit status via `has_exited()` and `exit_code()`
/// - Clean up via `terminate()`
pub struct ProcessHandle {
    writer_tx: mpsc::Sender<Vec<u8>>,
    output_tx: broadcast::Sender<Bytes>,
    killer: StdMutex<Option<Box<dyn ChildTerminator>>>,
    reader_handle: StdMutex<Option<JoinHandle<()>>>,
    reader_abort_handles: StdMutex<Vec<AbortHandle>>,
    writer_handle: StdMutex<Option<JoinHandle<()>>>,
    wait_handle: StdMutex<Option<JoinHandle<()>>>,
    exit_status: Arc<AtomicBool>,
    exit_code: Arc<StdMutex<Option<i32>>>,
    // PTY handles must be preserved to prevent the process from receiving Control+C
    _pty_handles: StdMutex<Option<PtyHandles>>,
}

impl fmt::Debug for ProcessHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessHandle")
            .field("has_exited", &self.has_exited())
            .field("exit_code", &self.exit_code())
            .finish()
    }
}

impl ProcessHandle {
    /// Create a new process handle with all required components.
    #[allow(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    pub(crate) fn new(
        writer_tx: mpsc::Sender<Vec<u8>>,
        output_tx: broadcast::Sender<Bytes>,
        initial_output_rx: broadcast::Receiver<Bytes>,
        killer: Box<dyn ChildTerminator>,
        reader_handle: JoinHandle<()>,
        reader_abort_handles: Vec<AbortHandle>,
        writer_handle: JoinHandle<()>,
        wait_handle: JoinHandle<()>,
        exit_status: Arc<AtomicBool>,
        exit_code: Arc<StdMutex<Option<i32>>>,
        pty_handles: Option<PtyHandles>,
    ) -> (Self, broadcast::Receiver<Bytes>) {
        (
            Self {
                writer_tx,
                output_tx,
                killer: StdMutex::new(Some(killer)),
                reader_handle: StdMutex::new(Some(reader_handle)),
                reader_abort_handles: StdMutex::new(reader_abort_handles),
                writer_handle: StdMutex::new(Some(writer_handle)),
                wait_handle: StdMutex::new(Some(wait_handle)),
                exit_status,
                exit_code,
                _pty_handles: StdMutex::new(pty_handles),
            },
            initial_output_rx,
        )
    }

    /// Returns a channel sender for writing raw bytes to the child stdin.
    ///
    /// # Example
    /// ```ignore
    /// let writer = handle.writer_sender();
    /// writer.send(b"input\n".to_vec()).await?;
    /// ```
    #[inline]
    pub fn writer_sender(&self) -> mpsc::Sender<Vec<u8>> {
        self.writer_tx.clone()
    }

    /// Returns a broadcast receiver that yields stdout/stderr chunks.
    ///
    /// Multiple receivers can be created; each receives all output from the
    /// point of subscription.
    #[inline]
    pub fn output_receiver(&self) -> broadcast::Receiver<Bytes> {
        self.output_tx.subscribe()
    }

    /// True if the child process has exited.
    #[inline]
    pub fn has_exited(&self) -> bool {
        self.exit_status.load(Ordering::SeqCst)
    }

    /// Returns the exit code if the process has exited.
    #[inline]
    pub fn exit_code(&self) -> Option<i32> {
        *self.exit_code.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// True once the stdout/stderr reader task has drained the child streams.
    #[inline]
    pub fn is_output_drained(&self) -> bool {
        self.reader_handle
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(JoinHandle::is_finished))
            .unwrap_or(true)
    }

    /// Attempts to kill the child and abort helper tasks.
    ///
    /// This is idempotent and safe to call multiple times.
    pub fn terminate(&self) {
        self.terminate_internal();
    }

    /// Kill the child process group without aborting the readers or wait task.
    ///
    /// Session owners use this path when they still need to drain output and
    /// reap the child after termination. Call [`Self::terminate`] when the
    /// caller is abandoning the session and does not need that final drain.
    pub fn terminate_process(&self) {
        if let Ok(mut killer_opt) = self.killer.lock()
            && let Some(mut killer) = killer_opt.take()
        {
            let _ = killer.kill();
        }
    }

    /// Internal termination that aborts all tasks.
    fn terminate_internal(&self) {
        // Kill the child process
        if let Ok(mut killer_opt) = self.killer.lock()
            && let Some(mut killer) = killer_opt.take()
        {
            let _ = killer.kill();
        }

        self.abort_tasks();
    }

    /// Abort all background tasks associated with this process.
    fn abort_tasks(&self) {
        // Abort reader handle
        if let Ok(mut h) = self.reader_handle.lock()
            && let Some(handle) = h.take()
        {
            handle.abort();
        }

        // Abort individual reader abort handles
        if let Ok(mut handles) = self.reader_abort_handles.lock() {
            for handle in handles.drain(..) {
                handle.abort();
            }
        }

        // Abort writer handle
        if let Ok(mut h) = self.writer_handle.lock()
            && let Some(handle) = h.take()
        {
            handle.abort();
        }

        // Abort wait handle
        if let Ok(mut h) = self.wait_handle.lock()
            && let Some(handle) = h.take()
        {
            handle.abort();
        }
    }

    /// Check if the process is still running.
    #[inline]
    pub fn is_running(&self) -> bool {
        !self.has_exited() && !self.is_writer_closed()
    }

    /// Send bytes to the process stdin.
    ///
    /// Returns an error if the stdin channel is closed.
    pub async fn write(&self, bytes: impl Into<Vec<u8>>) -> Result<(), mpsc::error::SendError<Vec<u8>>> {
        self.writer_tx.send(bytes.into()).await
    }

    /// Check if the writer channel is closed.
    #[inline]
    pub fn is_writer_closed(&self) -> bool {
        self.writer_tx.is_closed()
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // Synchronous kill + task aborts, reusing the same body as
        // `terminate_internal`. Both operations are non-blocking (`kill`
        // signals the process group; `abort` flags the tasks), so Drop never
        // parks the caller. A previous revision bridged this through a
        // dedicated thread + Tokio runtime (`async_drop`) for zero async
        // work — the thread/runtime only added spawn latency per Drop and
        // blocked the dropping thread, which serialized shutdown storms
        // (many handles dropped at registry teardown) and could stall a Tokio
        // worker during TUI exit.
        self.terminate_internal();
    }
}

/// Return value from spawn helpers (PTY or pipe).
///
/// Bundles the process handle with receivers for output and exit notification.
#[derive(Debug)]
pub struct SpawnedProcess {
    /// Handle for interacting with the process.
    pub session: ProcessHandle,
    /// Operating-system process identifier for the direct child.
    pub process_id: u32,
    /// Receiver for stdout/stderr output chunks.
    pub output_rx: broadcast::Receiver<Bytes>,
    /// Bounded, lossless receiver for consumers that must spool complete
    /// output. Unlike `output_rx`, this channel applies backpressure to the
    /// child-process readers instead of dropping lagged chunks.
    pub reliable_output_rx: mpsc::Receiver<Bytes>,
    /// Whether the producer is connected to `reliable_output_rx`.
    pub(crate) reliable_output_enabled: bool,
    /// Receiver for exit code (receives once when process exits).
    pub exit_rx: oneshot::Receiver<i32>,
}

impl SpawnedProcess {
    /// Convenience method to wait for the process to exit and collect output.
    ///
    /// Returns (collected_output, exit_code).
    pub async fn wait_with_output(self, timeout_ms: u64) -> (Vec<u8>, i32) {
        if self.reliable_output_enabled {
            collect_reliable_output_until_exit(self.reliable_output_rx, self.exit_rx, timeout_ms).await
        } else {
            collect_output_until_exit(self.output_rx, self.exit_rx, timeout_ms).await
        }
    }
}

/// Collect all output from the bounded process stream until exit or timeout.
async fn collect_reliable_output_until_exit(
    mut output_rx: mpsc::Receiver<Bytes>,
    exit_rx: oneshot::Receiver<i32>,
    timeout_ms: u64,
) -> (Vec<u8>, i32) {
    let mut collected = Vec::new();
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
    tokio::pin!(exit_rx);

    loop {
        tokio::select! {
            chunk = output_rx.recv() => {
                if let Some(chunk) = chunk {
                    collected.extend_from_slice(&chunk);
                } else {
                    return (collected, exit_rx.await.unwrap_or(-1));
                }
            }
            res = &mut exit_rx => {
                let code = res.unwrap_or(-1);
                // A descendant may inherit stdout/stderr after the direct
                // child exits. Keep the lossless path bounded just like the
                // compatibility broadcast path instead of waiting forever
                // for an inherited pipe descriptor to close.
                let quiet = tokio::time::Duration::from_millis(POST_EXIT_DRAIN_QUIET_MS);
                let max_deadline = tokio::time::Instant::now()
                    + tokio::time::Duration::from_millis(POST_EXIT_DRAIN_MAX_MS);
                while tokio::time::Instant::now() < max_deadline {
                    match tokio::time::timeout(quiet, output_rx.recv()).await {
                        Ok(Some(chunk)) => collected.extend_from_slice(&chunk),
                        Ok(None) | Err(_) => break,
                    }
                }
                return (collected, code);
            }
            _ = tokio::time::sleep_until(deadline) => {
                return (collected, -1);
            }
        }
    }
}

/// Collect output from a process until it exits or times out.
///
/// This is useful for tests and simple use cases where you want all output.
pub async fn collect_output_until_exit(
    mut output_rx: broadcast::Receiver<Bytes>,
    exit_rx: oneshot::Receiver<i32>,
    timeout_ms: u64,
) -> (Vec<u8>, i32) {
    let mut collected = Vec::new();
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(timeout_ms);
    tokio::pin!(exit_rx);

    loop {
        tokio::select! {
            res = output_rx.recv() => {
                if let Ok(chunk) = res {
                    collected.extend_from_slice(&chunk);
                }
            }
            res = &mut exit_rx => {
                let code = res.unwrap_or(-1);
                // Drain remaining output briefly after exit
                let quiet = tokio::time::Duration::from_millis(POST_EXIT_DRAIN_QUIET_MS);
                let max_deadline = tokio::time::Instant::now()
                    + tokio::time::Duration::from_millis(POST_EXIT_DRAIN_MAX_MS);

                while tokio::time::Instant::now() < max_deadline {
                    match tokio::time::timeout(quiet, output_rx.recv()).await {
                        Ok(Ok(chunk)) => collected.extend_from_slice(&chunk),
                        Ok(Err(broadcast::error::RecvError::Lagged(count))) => {
                            eprintln!("[vtcode] output stream lagged ({count} dropped)");
                            continue;
                        }
                        Ok(Err(broadcast::error::RecvError::Closed)) => break,
                        Err(_) => break, // Timeout - quiet period reached
                    }
                }
                return (collected, code);
            }
            _ = tokio::time::sleep_until(deadline) => {
                return (collected, -1);
            }
        }
    }
}

/// Backwards-compatible alias for ProcessHandle.
pub type ExecCommandSession = ProcessHandle;

/// Backwards-compatible alias for SpawnedProcess.
pub type SpawnedPty = SpawnedProcess;

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopTerminator;
    impl ChildTerminator for NoopTerminator {
        fn kill(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_process_handle_debug() {
        // Just verify Debug impl doesn't panic
        let exit_status = Arc::new(AtomicBool::new(false));
        let exit_code = Arc::new(StdMutex::new(None));

        let (writer_tx, _) = mpsc::channel(1);
        let (output_tx, initial_rx) = broadcast::channel(1);

        let (handle, _) = ProcessHandle::new(
            writer_tx,
            output_tx,
            initial_rx,
            Box::new(NoopTerminator),
            tokio::spawn(async {}),
            vec![],
            tokio::spawn(async {}),
            tokio::spawn(async {}),
            exit_status,
            exit_code,
            None,
        );

        let debug_str = format!("{handle:?}");
        assert!(debug_str.contains("ProcessHandle"));
    }

    #[tokio::test]
    async fn test_has_exited() {
        let exit_status = Arc::new(AtomicBool::new(false));
        let exit_code = Arc::new(StdMutex::new(None));

        let (writer_tx, _) = mpsc::channel(1);
        let (output_tx, initial_rx) = broadcast::channel(1);

        let (handle, _) = ProcessHandle::new(
            writer_tx,
            output_tx,
            initial_rx,
            Box::new(NoopTerminator),
            tokio::spawn(async {}),
            vec![],
            tokio::spawn(async {}),
            tokio::spawn(async {}),
            Arc::clone(&exit_status),
            exit_code,
            None,
        );

        assert!(!handle.has_exited());
        exit_status.store(true, Ordering::SeqCst);
        assert!(handle.has_exited());
    }

    struct RecordingTerminator(Arc<AtomicBool>);
    impl ChildTerminator for RecordingTerminator {
        fn kill(&mut self) -> io::Result<()> {
            self.0.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Drop must kill the child and abort helper tasks synchronously, without
    /// spawning a bridging thread/runtime (regression: a previous revision
    /// bridged Drop through `async_drop`, adding spawn latency and blocking
    /// the dropping thread during shutdown storms).
    #[tokio::test]
    async fn drop_kills_child_synchronously_without_bridging_runtime() {
        let killed = Arc::new(AtomicBool::new(false));
        let exit_status = Arc::new(AtomicBool::new(false));
        let exit_code = Arc::new(StdMutex::new(None));
        let (writer_tx, _) = mpsc::channel(1);
        let (output_tx, initial_rx) = broadcast::channel(1);

        // Tasks that never complete; Drop must abort them rather than wait.
        let (handle, _) = ProcessHandle::new(
            writer_tx,
            output_tx,
            initial_rx,
            Box::new(RecordingTerminator(Arc::clone(&killed))),
            tokio::spawn(std::future::pending()),
            vec![],
            tokio::spawn(std::future::pending()),
            tokio::spawn(std::future::pending()),
            exit_status,
            exit_code,
            None,
        );

        let started = std::time::Instant::now();
        drop(handle);
        assert!(killed.load(Ordering::SeqCst), "Drop must kill the child");
        assert!(started.elapsed() < std::time::Duration::from_millis(250), "Drop must not block on async cleanup");
    }
}
