use std::io::Write;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;
use portable_pty::{Child, MasterPty};
use tracing::warn;
use vt100::Parser;

use crate::tools::types::VTCodePtySession;

use super::scrollback::PtyScrollback;

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
            let slice = &text[index..];

            if let Some(len) = parse_ansi_sequence(slice) {
                index += len;
                continue;
            }

            if slice.as_bytes().starts_with(ZERO_WIDTH_SPACE) {
                index += ZERO_WIDTH_SPACE.len();
                continue;
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
    pub(super) writer: Mutex<Option<Box<dyn Write + Send>>>,
    pub(super) terminal: Arc<Mutex<Parser>>,
    pub(super) scrollback: Arc<Mutex<PtyScrollback>>,
    pub(super) reader_thread: Mutex<Option<JoinHandle<()>>>,
    pub(super) metadata: VTCodePtySession,
    pub(super) last_input: Mutex<Option<CommandEchoState>>,
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

        // Kill child if still running
        {
            let mut child = self.child.lock();
            if let Ok(None) = child.try_wait() {
                // Child still running, terminate it
                let _ = child.kill();
            }
        }

        // Join reader thread with timeout to prevent hangs
        {
            let mut thread_guard = self.reader_thread.lock();
            if let Some(reader_thread) = thread_guard.take() {
                // Use timeout to prevent infinite hang in Drop
                let join_result = std::thread::spawn(move || {
                    // Give reader thread up to 5 seconds to finish
                    let start = std::time::Instant::now();
                    loop {
                        if reader_thread.is_finished() {
                            let _ = reader_thread.join();
                            break;
                        }
                        if start.elapsed() > Duration::from_secs(5) {
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
        }
    }
}

impl PtySessionHandle {
    pub(super) fn snapshot_metadata(&self) -> VTCodePtySession {
        let mut metadata = self.metadata.clone();

        // Lock order: master -> terminal -> scrollback (respect documented order)
        // Note: master is acquired first (single-threaded access)
        let master_size = {
            let master = self.master.lock();
            master.get_size().ok()
        };

        if let Some(size) = master_size {
            metadata.rows = size.rows;
            metadata.cols = size.cols;
        }

        // terminal and scrollback are Arc-wrapped, can be acquired independently
        {
            let parser = self.terminal.lock();
            let contents = parser.screen().contents();
            metadata.screen_contents = Some(contents);
        }
        {
            let scrollback = self.scrollback.lock();
            let contents = scrollback.snapshot();
            if !contents.is_empty() {
                metadata.scrollback = Some(contents);
            }
        }

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

        let filtered = self.strip_command_echo(text);
        if filtered.is_empty() {
            None
        } else {
            Some(filtered)
        }
    }

    fn strip_command_echo(&self, text: String) -> String {
        let mut guard = self.last_input.lock();
        let Some(state) = guard.as_mut() else {
            return text;
        };

        let (consumed, done) = state.consume_chunk(&text);
        if done {
            *guard = None;
        }

        text.get(consumed..)
            .map(|tail| tail.to_owned())
            .unwrap_or_default()
    }
}
