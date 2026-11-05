use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::Utc;
use parking_lot::Mutex;
use portable_pty::{Child, MasterPty, PtySize};
use tracing::warn;

use crate::tools::types::VTCodePtySession;

use super::screen_backend::PtyScreenState;
use super::scrollback::PtyScrollback;

/// Maximum time to wait for reader thread to finish (ms)
const READER_THREAD_TIMEOUT_MS: u64 = 5000;

/// Maximum time to poll `try_wait` after kill before escalating (ms).
/// `Child::wait` is unbounded and can stall the close path forever; kill +
/// bounded poll must never hang the runloop.
const CHILD_REAP_TIMEOUT_MS: u64 = 2000;

/// Second-phase poll budget after SIGKILL escalation (ms).
const CHILD_REAP_ESCALATED_TIMEOUT_MS: u64 = 1000;

/// Poll `try_wait` until the child exits or `budget` expires.
/// Never calls `Child::wait`. Returns whether the child was observed exited.
fn reap_child_poll(child: &mut Box<dyn Child + Send>, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) | Err(_) => {
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

/// Re-send SIGKILL to the process group (when known) and the direct child.
fn escalate_child_kill(child: &mut Box<dyn Child + Send>, child_pid: Option<u32>) {
    if let Some(pid) = child_pid {
        let _ = vtcode_bash_runner::kill_process_group_by_pid(pid);
        let _ = vtcode_bash_runner::kill_process_group(pid);
    }
    let _ = child.kill();
}

/// Two-phase reap: poll, escalate SIGKILL if still live, poll again.
/// Never calls `Child::wait`. Returns whether the child was observed exited.
/// A false return means the child is still live after both budgets (unkillable
/// or stuck in uninterruptible sleep); callers must still release capacity.
///
/// Platform note: escalation uses `kill_process_group_by_pid` /
/// `kill_process_group` / `child.kill()`. Process-group kill is exercised on
/// Unix (`#[cfg(unix)]` tests in `force_terminate_reaps_live_pty_child`);
/// Windows uses `child.kill()` only — group semantics are not covered by those
/// tests and remain unverified on Windows CI.
fn reap_child_bounded(child: &mut Box<dyn Child + Send>, child_pid: Option<u32>) -> bool {
    if reap_child_poll(child, Duration::from_millis(CHILD_REAP_TIMEOUT_MS)) {
        return true;
    }
    warn!("PTY child did not exit within reap timeout; escalating SIGKILL");
    escalate_child_kill(child, child_pid);
    if reap_child_poll(child, Duration::from_millis(CHILD_REAP_ESCALATED_TIMEOUT_MS)) {
        return true;
    }
    warn!("PTY child did not exit within escalated reap timeout");
    false
}

/// Kill the PTY child and report whether its exit was actually observed.
/// A confirmed child exit does not itself prove that every descendant exited.
fn force_terminate_child(child: &mut Box<dyn Child + Send>, child_pid: Option<u32>) -> bool {
    let child_running = !matches!(child.try_wait(), Ok(Some(_)));
    if let Some(pid) = child_pid {
        if child_running {
            let _ = vtcode_bash_runner::kill_process_group_by_pid(pid);
        } else {
            let _ = vtcode_bash_runner::kill_process_group(pid);
        }
    }
    if child_running {
        let _ = child.kill();
        reap_child_bounded(child, child_pid)
    } else {
        true
    }
}

/// Join the PTY reader thread without blocking past `READER_THREAD_TIMEOUT_MS`.
/// Shared by `close_session` and `Drop` so neither path can hang on `join`.
pub(super) fn join_reader_thread_bounded(reader_thread: JoinHandle<()>) {
    let join_result = std::thread::spawn(move || {
        let start = Instant::now();
        let timeout = Duration::from_millis(READER_THREAD_TIMEOUT_MS);
        loop {
            if reader_thread.is_finished() {
                let _ = reader_thread.join();
                break;
            }
            if start.elapsed() > timeout {
                warn!("PTY reader thread did not finish within timeout");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })
    .join();

    if join_result.is_err() {
        warn!("PTY reader thread cleanup panicked");
    }
}

#[derive(Clone)]
pub(super) struct CommandEchoState {
    command_bytes: Vec<u8>,
    failure: Vec<usize>,
    matched: usize,
    require_newline: bool,
    pending_newline: bool,
    consumed_once: bool,
}

impl CommandEchoState {
    pub(super) fn new(command: &str, expect_newline: bool) -> Option<Self> {
        let trimmed = command.trim_matches(|ch| ch == '\n' || ch == '\r');
        if trimmed.is_empty() {
            return None;
        }

        let command_bytes = trimmed.as_bytes().to_vec();
        if command_bytes.is_empty() {
            return None;
        }

        let failure = build_failure(&command_bytes);

        Some(Self {
            command_bytes,
            failure,
            matched: 0,
            require_newline: expect_newline,
            pending_newline: expect_newline,
            consumed_once: false,
        })
    }

    fn reset(&mut self) {
        self.matched = 0;
        self.pending_newline = self.require_newline;
    }

    fn consume_chunk(&mut self, text: &str) -> (usize, bool) {
        let mut index = 0usize;
        let bytes = text.as_bytes();
        const ZERO_WIDTH_SPACE: &[u8] = "\u{200B}".as_bytes();

        while index < bytes.len() {
            if text.is_char_boundary(index) {
                let slice = &text[index..];

                if let Some(len) = parse_ansi_sequence(slice) {
                    index += len;
                    continue;
                }

                if slice.as_bytes().starts_with(ZERO_WIDTH_SPACE) {
                    index += ZERO_WIDTH_SPACE.len();
                    continue;
                }
            }

            let byte = bytes[index];

            if byte == b'\r' {
                index += 1;
                self.reset();
                continue;
            }

            if self.pending_newline {
                if byte == b'\n' {
                    index += 1;
                    self.pending_newline = false;
                    continue;
                }
                self.pending_newline = false;
            }

            let mut matched_byte = false;
            loop {
                if let Some(&expected) = self.command_bytes.get(self.matched)
                    && byte == expected
                {
                    self.matched += 1;
                    index += 1;
                    if self.matched == self.command_bytes.len() {
                        self.consumed_once = true;
                        self.pending_newline = self.require_newline;
                        self.matched = if self.command_bytes.len() > 1 {
                            self.failure[self.matched - 1]
                        } else {
                            0
                        };
                    }
                    matched_byte = true;
                    break;
                }

                if self.matched == 0 {
                    break;
                }

                self.matched = self.failure[self.matched - 1];
            }

            if matched_byte {
                continue;
            }

            break;
        }

        let done = self.consumed_once && !self.pending_newline && self.matched == 0;
        (index, done)
    }
}

fn build_failure(pattern: &[u8]) -> Vec<usize> {
    let mut failure = vec![0usize; pattern.len()];
    let mut length = 0usize;
    let mut index = 1usize;

    while index < pattern.len() {
        if pattern[index] == pattern[length] {
            length += 1;
            failure[index] = length;
            index += 1;
        } else if length != 0 {
            length = failure[length - 1];
        } else {
            failure[index] = 0;
            index += 1;
        }
    }

    failure
}

fn parse_ansi_sequence(text: &str) -> Option<usize> {
    crate::utils::ansi_parser::parse_ansi_sequence(text)
}

pub(super) struct PtySessionHandle {
    pub(super) master: Mutex<Box<dyn MasterPty + Send>>,
    pub(super) child: Mutex<Box<dyn Child + Send>>,
    pub(super) child_pid: Option<u32>,
    pub(super) writer: Mutex<Option<Box<dyn Write + Send>>>,
    pub(super) screen_state: Arc<Mutex<PtyScreenState>>,
    pub(super) scrollback: Arc<Mutex<PtyScrollback>>,
    pub(super) reader_thread: Mutex<Option<JoinHandle<()>>>,
    pub(super) reader_completed: Arc<AtomicBool>,
    pub(super) metadata: VTCodePtySession,
    pub(super) last_input: Mutex<Option<CommandEchoState>>,
    pub(super) _zsh_exec_bridge: Option<crate::zsh_exec_bridge::ZshExecBridgeSession>,
    pub(super) output_total_bytes: Arc<std::sync::atomic::AtomicU64>,
    pub(super) output_spool_failed: Arc<AtomicBool>,
    pub(super) output_spool_ready: Arc<AtomicBool>,
    pub(super) output_spool_finished: Arc<AtomicBool>,
    pub(super) output_spool_path: String,
    pub(super) output_spool_integrity: Arc<Mutex<Option<crate::tools::output_spooler::SpoolIntegrity>>>,
}

impl PtySessionHandle {
    /// Gracefully terminate the child process.
    ///
    /// This method attempts a graceful shutdown by:
    /// 1. Sending SIGTERM (via process group kill on Unix)
    /// 2. Waiting for a short grace period
    /// 3. Sending SIGKILL if the process hasn't exited
    ///
    /// This ensures that child processes have a chance to clean up
    /// before being forcibly terminated.
    pub(super) fn graceful_terminate(&self) {
        let mut child = self.child.lock();

        let child_running = match child.try_wait() {
            Ok(Some(_)) => false,
            Ok(None) | Err(_) => true,
        };

        // Kill the process group and the direct child process handle.
        // vtcode_bash_runner::graceful_kill_process_group_default now handles
        // the robust 'more kills' pattern which ensures descendants do not survive.
        if let Some(pid) = self.child_pid {
            if child_running {
                vtcode_bash_runner::graceful_kill_process_group_default(pid);
            }
            // The direct child can exit during the graceful window while a
            // descendant still owns the PTY. Finish with the cached group id,
            // and retain the direct-child fallback for platforms without
            // Unix process-group support.
            let _ = vtcode_bash_runner::kill_process_group(pid);
            if child_running {
                let _ = child.kill();
            }
        } else if child_running {
            let _ = child.kill();
        }

        if child_running {
            reap_child_bounded(&mut child, self.child_pid);
        }
    }

    /// Forcefully terminate the child process without waiting for a graceful
    /// shutdown window.
    pub(super) fn force_terminate(&self) -> bool {
        let mut child = self.child.lock();
        force_terminate_child(&mut child, self.child_pid)
    }
}

impl Drop for PtySessionHandle {
    fn drop(&mut self) {
        // Ensure cleanup even if close_session() wasn't called
        // Follow lock order: writer -> child -> reader_thread -> (no other locks in drop)

        // Close writer
        {
            let mut writer = self.writer.lock();
            if let Some(mut w) = writer.take() {
                let _ = w.write_all(b"exit\n");
                let _ = w.flush();
            }
        }

        // Kill child process and its process group using graceful termination.
        // Match the robust termination behavior from codex-rs/utils/pty PR 12688
        // which ensures descendants from interactive shells/REPLs do not survive.
        // Reap after kill so Drop-only paths (close timeout, map remove) cannot
        // leave a zombie; previously Drop never called try_wait.
        {
            let mut child = self.child.lock();
            let child_running = match child.try_wait() {
                Ok(Some(_)) => {
                    // The direct child may have exited while a descendant
                    // still owns the PTY descriptors.
                    if let Some(pid) = self.child_pid {
                        let _ = vtcode_bash_runner::kill_process_group(pid);
                    }
                    false
                }
                Ok(None) | Err(_) => {
                    if let Some(pid) = self.child_pid {
                        vtcode_bash_runner::graceful_kill_process_group_default(pid);
                        let _ = vtcode_bash_runner::kill_process_group(pid);
                        let _ = child.kill();
                    } else {
                        let _ = child.kill();
                    }
                    true
                }
            };
            if child_running {
                reap_child_bounded(&mut child, self.child_pid);
            }
        }

        // Join reader thread with timeout to prevent hangs
        {
            let mut thread_guard = self.reader_thread.lock();
            if let Some(reader_thread) = thread_guard.take() {
                join_reader_thread_bounded(reader_thread);
            }
        }
    }
}

impl PtySessionHandle {
    pub(super) fn snapshot_metadata(&self) -> VTCodePtySession {
        let mut metadata = self.metadata.clone();

        let master_size = {
            let master = self.master.lock();
            master.get_size().ok()
        };

        let size = master_size.unwrap_or(PtySize {
            rows: metadata.rows,
            cols: metadata.cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        metadata.rows = size.rows;
        metadata.cols = size.cols;
        metadata.child_pid = self.child_pid;
        if metadata.started_at.is_none() {
            metadata.started_at = Some(Utc::now());
        }
        let exit_code = {
            let mut child = self.child.lock();
            child
                .try_wait()
                .ok()
                .flatten()
                .map(crate::tools::pty::manager_utils::exit_status_code)
        };
        metadata.exit_code = exit_code;
        metadata.lifecycle_state = Some(if exit_code.is_some() {
            crate::tools::types::VTCodeSessionLifecycleState::Exited
        } else {
            crate::tools::types::VTCodeSessionLifecycleState::Running
        });

        let snapshot = {
            let screen_state = self.screen_state.lock();
            screen_state.prepare_snapshot()
        };

        metadata.screen_contents = Some(snapshot.screen_contents);
        metadata.scrollback = None;

        metadata
    }

    pub(super) fn read_output(&self, drain: bool) -> Option<String> {
        let mut scrollback = self.scrollback.lock();
        let text = if drain {
            scrollback.take_pending()
        } else {
            scrollback.pending()
        };
        if text.is_empty() {
            return None;
        }

        let filtered = if drain {
            self.strip_command_echo(text)
        } else {
            self.preview_command_echo(text)
        };
        if filtered.is_empty() { None } else { Some(filtered) }
    }

    /// Check if all output from this PTY session has been consumed.
    /// Returns `true` when there is no pending output in the scrollback buffer
    /// AND the reader thread has finished pushing data.
    pub(super) fn is_output_drained(&self) -> bool {
        let scrollback = self.scrollback.lock();
        !scrollback.has_pending()
            && self.reader_completed.load(Ordering::Acquire)
            && self.output_spool_finished.load(Ordering::Acquire)
    }

    fn strip_command_echo(&self, text: String) -> String {
        let mut guard = self.last_input.lock();
        let Some(state) = guard.as_mut() else {
            return text;
        };

        let (filtered, done) = filter_command_echo(text, state);
        if done {
            *guard = None;
        }
        filtered
    }

    fn preview_command_echo(&self, text: String) -> String {
        let mut preview_state = self.last_input.lock().clone();
        let Some(state) = preview_state.as_mut() else {
            return text;
        };

        filter_command_echo(text, state).0
    }
}

fn filter_command_echo(text: String, state: &mut CommandEchoState) -> (String, bool) {
    let (consumed, done) = state.consume_chunk(&text);
    let consumed = (0..=consumed).rev().find(|&idx| text.is_char_boundary(idx)).unwrap_or(0);
    (text[consumed..].to_owned(), done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct UnreapableChild;

    impl portable_pty::ChildKiller for UnreapableChild {
        fn kill(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("simulated kill failure"))
        }

        fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
            Box::new(self.clone())
        }
    }

    impl Child for UnreapableChild {
        fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
            Ok(None)
        }

        fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
            Err(std::io::Error::other("blocking wait must not be used"))
        }

        fn process_id(&self) -> Option<u32> {
            None
        }

        #[cfg(windows)]
        fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
            None
        }
    }

    #[test]
    fn force_terminate_child_reports_unconfirmed_exit() {
        let mut child: Box<dyn Child + Send> = Box::new(UnreapableChild);
        assert!(!force_terminate_child(&mut child, None));
    }

    #[test]
    fn command_echo_filter_handles_multibyte_command() {
        let mut state = CommandEchoState::new("привет\n", true).expect("command should be tracked");

        let (filtered, done) = filter_command_echo("привет\noutput".to_owned(), &mut state);

        assert_eq!(filtered, "output");
        assert!(done);
    }
}
