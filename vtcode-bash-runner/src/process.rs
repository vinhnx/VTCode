//! Unified process handle types for PTY and pipe backends.
//!
//! This module provides abstractions for interacting with spawned processes
//! regardless of whether they use a PTY or regular pipes.
//!
//! Inspired by codex-rs/utils/pty process handle patterns.

use std::fmt;
use std::io;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::{AbortHandle, JoinHandle};

/// Trait for process termination strategies.
///
/// Different backends (PTY vs pipe) may need different termination approaches.
pub trait ChildTerminator: Send + Sync {
    /// Kill the child process.
    fn kill(&mut self) -> io::Result<()>;
}

/// Optional PTY-specific handles that must be preserved.
///
/// For PTY processes, the slave handle must be kept alive because the process
/// will receive SIGHUP if it's closed.
pub struct PtyHandles {
    /// The slave PTY handle (kept alive to prevent SIGHUP).
    pub _slave: Option<Box<dyn Send>>,
    /// The master PTY handle.
    pub _master: Box<dyn Send>,
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
    output_tx: broadcast::Sender<Vec<u8>>,
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        writer_tx: mpsc::Sender<Vec<u8>>,
        output_tx: broadcast::Sender<Vec<u8>>,
        initial_output_rx: broadcast::Receiver<Vec<u8>>,
        killer: Box<dyn ChildTerminator>,
        reader_handle: JoinHandle<()>,
        reader_abort_handles: Vec<AbortHandle>,
        writer_handle: JoinHandle<()>,
        wait_handle: JoinHandle<()>,
        exit_status: Arc<AtomicBool>,
        exit_code: Arc<StdMutex<Option<i32>>>,
        pty_handles: Option<PtyHandles>,
    ) -> (Self, broadcast::Receiver<Vec<u8>>) {
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
    pub fn writer_sender(&self) -> mpsc::Sender<Vec<u8>> {
        self.writer_tx.clone()
    }

    /// Returns a broadcast receiver that yields stdout/stderr chunks.
    ///
    /// Multiple receivers can be created; each receives all output from the
    /// point of subscription.
    pub fn output_receiver(&self) -> broadcast::Receiver<Vec<u8>> {
        self.output_tx.subscribe()
    }

    /// True if the child process has exited.
    pub fn has_exited(&self) -> bool {
        self.exit_status.load(Ordering::SeqCst)
    }

    /// Returns the exit code if the process has exited.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code.lock().ok().and_then(|guard| *guard)
    }

    /// Attempts to kill the child and abort helper tasks.
    ///
    /// This is idempotent and safe to call multiple times.
    pub fn terminate(&self) {
        self.terminate_internal();
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
    pub fn is_running(&self) -> bool {
        !self.has_exited() && !self.is_writer_closed()
    }

    /// Send bytes to the process stdin.
    ///
    /// Returns an error if the stdin channel is closed.
    pub async fn write(
        &self,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<(), mpsc::error::SendError<Vec<u8>>> {
        self.writer_tx.send(bytes.into()).await
    }

    /// Check if the writer channel is closed.
    pub fn is_writer_closed(&self) -> bool {
        self.writer_tx.is_closed()
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
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
    /// Receiver for stdout/stderr output chunks.
    pub output_rx: broadcast::Receiver<Vec<u8>>,
    /// Receiver for exit code (receives once when process exits).
    pub exit_rx: oneshot::Receiver<i32>,
}

impl SpawnedProcess {
    /// Convenience method to wait for the process to exit and collect output.
    ///
    /// Returns (collected_output, exit_code).
    pub async fn wait_with_output(self, timeout_ms: u64) -> (Vec<u8>, i32) {
        collect_output_until_exit(self.output_rx, self.exit_rx, timeout_ms).await
    }
}

/// Collect output from a process until it exits or times out.
///
/// This is useful for tests and simple use cases where you want all output.
pub async fn collect_output_until_exit(
    mut output_rx: broadcast::Receiver<Vec<u8>>,
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
                let quiet = tokio::time::Duration::from_millis(50);
                let max_deadline = tokio::time::Instant::now() + tokio::time::Duration::from_millis(500);

                while tokio::time::Instant::now() < max_deadline {
                    match tokio::time::timeout(quiet, output_rx.recv()).await {
                        Ok(Ok(chunk)) => collected.extend_from_slice(&chunk),
                        Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
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
}
