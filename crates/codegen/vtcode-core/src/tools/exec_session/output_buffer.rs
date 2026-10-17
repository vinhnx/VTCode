//! Bounded pipe-output buffering: head/tail windows, stats, retained previews.

use super::*;

/// Result of the Ctrl+B foreground-session handoff request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundShortcutResult {
    /// A live foreground session was reserved for background promotion.
    Requested,
    /// The runtime already owns the maximum number of background processes.
    AtCapacity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PipeOutputStats {
    pub total_bytes: u64,
    pub truncated: bool,
    pub spool_path: String,
    pub spool_available: bool,
    pub spool_complete: bool,
    pub spool_integrity: Option<SpoolIntegrity>,
}

#[derive(Default)]
pub(crate) struct PipeOutputBuffer {
    pub(crate) pending: Mutex<PipeOutputWindow>,
    total_bytes: AtomicU64,
    pub(crate) truncated: AtomicBool,
}

#[derive(Default)]
pub(crate) struct PipeOutputWindow {
    head: String,
    tail: String,
    total_bytes: u64,
    pub(crate) truncated: bool,
}

impl PipeOutputBuffer {
    pub(crate) async fn append(&self, chunk: &str, raw_byte_count: usize) {
        let mut pending = self.pending.lock().await;
        pending.total_bytes = pending.total_bytes.saturating_add(raw_byte_count as u64);
        self.total_bytes.fetch_add(raw_byte_count as u64, Ordering::Relaxed);
        if pending.head.len() < PIPE_OUTPUT_HEAD_BYTES {
            let remaining = PIPE_OUTPUT_HEAD_BYTES - pending.head.len();
            let end = chunk.floor_char_boundary(remaining.min(chunk.len()));
            pending.head.push_str(&chunk[..end]);
            if end < chunk.len() {
                pending.truncated = true;
                self.truncated.store(true, Ordering::Relaxed);
            }
        }

        // Operate on `pending.tail` in place. The previous code cloned the tail,
        // mutated the clone, then assigned it back — an O(tail) copy on every
        // output chunk. Since `pending` is already a &mut MutexGuard, in-place
        // mutation is equivalent and avoids the per-chunk allocation.
        pending.tail.push_str(chunk);
        if pending.tail.len() > PIPE_OUTPUT_TAIL_BYTES {
            let start = pending.tail.ceil_char_boundary(pending.tail.len() - PIPE_OUTPUT_TAIL_BYTES);
            pending.tail.drain(..start);
            pending.truncated = true;
            self.truncated.store(true, Ordering::Relaxed);
        }
    }

    pub(crate) async fn peek_pending(&self) -> Option<String> {
        let pending = self.pending.lock().await;
        if pending.total_bytes == 0 {
            None
        } else {
            Some(pending.preview())
        }
    }

    pub(crate) async fn drain_pending(&self) -> Option<String> {
        let mut pending = self.pending.lock().await;
        if pending.total_bytes == 0 {
            None
        } else {
            Some(std::mem::take(&mut *pending).preview())
        }
    }

    pub(crate) async fn stats(&self) -> (u64, bool) {
        (self.total_bytes.load(Ordering::Relaxed), self.truncated.load(Ordering::Relaxed))
    }
}

impl PipeOutputWindow {
    pub(crate) fn preview(&self) -> String {
        if !self.truncated {
            return self.head.clone();
        }
        if self.head == self.tail {
            return format!("{}\n[output preview truncated]", self.head);
        }
        format!("{}\n[output preview truncated]\n{}", self.head, self.tail)
    }
}

#[derive(Clone, Default)]
pub(crate) struct RetainedSessionPreview {
    head: String,
    tail: String,
    total_bytes: u64,
    pub(crate) truncated: bool,
}

impl RetainedSessionPreview {
    pub(crate) fn append(&mut self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }

        self.total_bytes = self.total_bytes.saturating_add(chunk.len() as u64);
        if self.head.len() < EXEC_SESSION_PREVIEW_HEAD_BYTES {
            let remaining = EXEC_SESSION_PREVIEW_HEAD_BYTES - self.head.len();
            let end = chunk.floor_char_boundary(remaining.min(chunk.len()));
            self.head.push_str(&chunk[..end]);
            if end < chunk.len() {
                self.truncated = true;
            }
        }

        self.tail.push_str(chunk);
        if self.tail.len() > EXEC_SESSION_PREVIEW_TAIL_BYTES {
            let start = self.tail.ceil_char_boundary(self.tail.len() - EXEC_SESSION_PREVIEW_TAIL_BYTES);
            self.tail.drain(..start);
            self.truncated = true;
        }
    }

    pub(crate) fn render(&self) -> String {
        if self.total_bytes == 0 {
            return String::new();
        }
        if !self.truncated {
            return self.head.clone();
        }
        if self.head == self.tail {
            return format!("{}\n[output preview truncated]", self.head);
        }
        format!("{}\n[output preview truncated]\n{}", self.head, self.tail)
    }
}
