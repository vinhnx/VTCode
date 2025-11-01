use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;

use crate::ui::tui::{InlineHandle, InlineMessageKind, InlineSegment, InlineTextStyle};
use crate::utils::ansi::MessageStyle;

const MAX_LINES: usize = 4000;
const MAX_QUEUE_SIZE: usize = 100;

static TRANSCRIPT: Lazy<RwLock<Vec<String>>> = Lazy::new(|| RwLock::new(Vec::new()));
static INLINE_HANDLE: Lazy<RwLock<Option<Arc<InlineHandle>>>> = Lazy::new(|| RwLock::new(None));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TranscriptMode {
    #[allow(dead_code)]
    Normal,
    Suppressed,
}

thread_local! {
    static MODE_STACK: RefCell<Vec<TranscriptMode>> = RefCell::new(Vec::new());
}

fn is_suppressed() -> bool {
    MODE_STACK.with(|stack| matches!(stack.borrow().last(), Some(TranscriptMode::Suppressed)))
}

struct SuspensionGuard {
    active: bool,
}

impl SuspensionGuard {
    fn new() -> Self {
        MODE_STACK.with(|stack| stack.borrow_mut().push(TranscriptMode::Suppressed));
        Self { active: true }
    }
}

impl Drop for SuspensionGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        MODE_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            match stack.pop() {
                Some(TranscriptMode::Suppressed) | None => {}
                Some(TranscriptMode::Normal) => {
                    debug_assert!(
                        false,
                        "transcript suspension stack corrupted: expected Suppressed"
                    );
                }
            };
        });
        self.active = false;
    }
}

fn suspend() -> SuspensionGuard {
    SuspensionGuard::new()
}

pub fn with_suppressed<F, R>(operation: F) -> R
where
    F: FnOnce() -> R,
{
    let guard = suspend();
    let result = operation();
    drop(guard);
    result
}

/// Structured message with metadata for queuing
#[derive(Clone, Debug)]
struct QueuedMessage {
    text: String,
    kind: InlineMessageKind,
    style: InlineTextStyle,
}

static MESSAGE_QUEUE: Lazy<RwLock<VecDeque<QueuedMessage>>> =
    Lazy::new(|| RwLock::new(VecDeque::new()));

pub fn append(line: &str) {
    if is_suppressed() {
        return;
    }
    let mut log = TRANSCRIPT.write();
    if log.len() == MAX_LINES {
        let drop_count = MAX_LINES / 5;
        log.drain(0..drop_count);
    }
    log.push(line.to_string());
}

pub fn replace_last(count: usize, lines: &[String]) {
    if is_suppressed() {
        return;
    }
    let mut log = TRANSCRIPT.write();
    let remove = count.min(log.len());
    for _ in 0..remove {
        log.pop();
    }
    for line in lines {
        if log.len() == MAX_LINES {
            let drop_count = MAX_LINES / 5;
            log.drain(0..drop_count);
        }
        log.push(line.clone());
    }
}

pub fn snapshot() -> Vec<String> {
    TRANSCRIPT.read().clone()
}

pub fn len() -> usize {
    TRANSCRIPT.read().len()
}

pub fn clear() {
    TRANSCRIPT.write().clear();
}

/// Set the inline handle for immediate message display
pub fn set_inline_handle(handle: Arc<InlineHandle>) {
    *INLINE_HANDLE.write() = Some(handle);
}

/// Remove the inline handle
pub fn clear_inline_handle() {
    *INLINE_HANDLE.write() = None;
}

/// Map MessageStyle to InlineMessageKind (mirrors logic from ansi.rs)
fn message_kind(style: MessageStyle) -> InlineMessageKind {
    match style {
        MessageStyle::Info => InlineMessageKind::Info,
        MessageStyle::Error => InlineMessageKind::Error,
        MessageStyle::Output => InlineMessageKind::Pty,
        MessageStyle::Response => InlineMessageKind::Agent,
        MessageStyle::Tool | MessageStyle::ToolDetail => InlineMessageKind::Tool,
        MessageStyle::Status | MessageStyle::McpStatus => InlineMessageKind::Info,
        MessageStyle::User => InlineMessageKind::User,
        MessageStyle::Reasoning => InlineMessageKind::Policy,
    }
}

/// Enqueue a message with a specific style and display it immediately
pub fn enqueue_message(message: &str, style: MessageStyle) {
    enqueue_message_with_kind(message, message_kind(style), InlineTextStyle::default())
}

/// Enqueue a message with a specific kind and display it immediately
pub fn enqueue_message_with_kind(
    message: &str,
    kind: InlineMessageKind,
    text_style: InlineTextStyle,
) {
    let queued = QueuedMessage {
        text: message.to_string(),
        kind,
        style: text_style,
    };

    // Enqueue the message
    {
        let mut queue = MESSAGE_QUEUE.write();
        if queue.len() >= MAX_QUEUE_SIZE {
            queue.pop_front();
        }
        queue.push_back(queued.clone());
    }

    // Display immediately if we have an inline handle
    display_message_now(&queued.text, queued.kind, &queued.style);

    // Also add to transcript for persistence (plain text)
    append(message);
}

/// Display a message immediately without queuing (low-level function)
fn display_message_now(text: &str, kind: InlineMessageKind, style: &InlineTextStyle) {
    if let Some(handle) = INLINE_HANDLE.read().as_ref() {
        handle.append_line(
            kind,
            vec![InlineSegment {
                text: text.to_string(),
                style: style.clone(),
            }],
        );
    }
}

/// Enqueue a message and display it immediately (defaults to Output/Pty style)
pub fn enqueue(message: &str) {
    enqueue_message(message, MessageStyle::Output);
}

/// Display a message immediately without enqueueing or adding to transcript
pub fn display_immediate(message: &str, style: MessageStyle) {
    display_message_now(message, message_kind(style), &InlineTextStyle::default());
}

/// Get all queued messages as plain text
pub fn get_queued_messages() -> Vec<String> {
    MESSAGE_QUEUE
        .read()
        .iter()
        .map(|m| m.text.clone())
        .collect()
}

/// Get all queued messages with their metadata
pub fn get_queued_messages_with_metadata() -> Vec<(String, InlineMessageKind)> {
    MESSAGE_QUEUE
        .read()
        .iter()
        .map(|m| (m.text.clone(), m.kind))
        .collect()
}

/// Clear the message queue
pub fn clear_queue() {
    MESSAGE_QUEUE.write().clear();
}

/// Get queue length
pub fn queue_len() -> usize {
    MESSAGE_QUEUE.read().len()
}

/// Replay all queued messages to the current inline handle (useful for recovery)
pub fn replay_queued_messages() {
    let messages: Vec<QueuedMessage> = MESSAGE_QUEUE.read().iter().cloned().collect();
    for msg in messages {
        display_message_now(&msg.text, msg.kind, &msg.style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_snapshot_store_lines() {
        clear();
        append("first");
        append("second");
        assert_eq!(len(), 2);
        let snap = snapshot();
        assert_eq!(snap, vec!["first".to_string(), "second".to_string()]);
        clear();
    }

    #[test]
    fn transcript_drops_oldest_chunk_when_full() {
        clear();
        for idx in 0..MAX_LINES {
            append(&format!("line {idx}"));
        }
        assert_eq!(len(), MAX_LINES);
        for extra in 0..10 {
            append(&format!("extra {extra}"));
        }
        assert_eq!(len(), MAX_LINES - (MAX_LINES / 5) + 10);
        let snap = snapshot();
        assert_eq!(snap.first().unwrap(), &format!("line {}", MAX_LINES / 5));
        clear();
    }

    #[test]
    fn message_queue_enqueue_and_retrieve() {
        clear_queue();
        assert_eq!(queue_len(), 0);

        enqueue("first message");
        enqueue_message("second message", MessageStyle::Info);

        assert_eq!(queue_len(), 2);
        let messages = get_queued_messages();
        assert_eq!(messages, vec!["first message", "second message"]);

        clear_queue();
        assert_eq!(queue_len(), 0);
    }

    #[test]
    fn message_queue_preserves_metadata() {
        clear_queue();

        enqueue_message("info message", MessageStyle::Info);
        enqueue_message("error message", MessageStyle::Error);
        enqueue_message("user message", MessageStyle::User);

        let messages = get_queued_messages_with_metadata();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].1, InlineMessageKind::Info);
        assert_eq!(messages[1].1, InlineMessageKind::Error);
        assert_eq!(messages[2].1, InlineMessageKind::User);

        clear_queue();
    }

    #[test]
    fn message_queue_size_limit() {
        clear_queue();

        // Fill queue to max capacity
        for i in 0..MAX_QUEUE_SIZE {
            enqueue(&format!("message {}", i));
        }
        assert_eq!(queue_len(), MAX_QUEUE_SIZE);

        // Add one more message - should drop oldest
        enqueue("overflow message");
        assert_eq!(queue_len(), MAX_QUEUE_SIZE);

        let messages = get_queued_messages();
        assert_eq!(messages.first().unwrap(), "message 1"); // First message should be dropped
        assert_eq!(messages.last().unwrap(), "overflow message");

        clear_queue();
    }

    #[test]
    fn suppressed_scope_skips_transcript_entries() {
        clear();
        with_suppressed(|| {
            append("hidden");
        });
        append("visible");
        let snap = snapshot();
        assert_eq!(snap, vec!["visible".to_string()]);
        clear();
    }
}
