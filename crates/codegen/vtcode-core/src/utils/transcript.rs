use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;

use crate::ui::{InlineHandle, InlineMessageKind, InlineSegment, InlineTextStyle};
pub use crate::utils::message_style::MessageStyle;

const MAX_LINES: usize = 4000;
const MAX_QUEUE_SIZE: usize = 100;

static TRANSCRIPT: Lazy<RwLock<Vec<String>>> = Lazy::new(|| RwLock::new(Vec::new()));
static INLINE_HANDLE: Lazy<RwLock<Option<Arc<InlineHandle>>>> = Lazy::new(|| RwLock::new(None));
/// Session-scoped replaceable tracker transcript block. Shared by every tracker
/// writer (pipeline + plan-approval handoff) so one surface owns replace/dedupe.
static REPLACEABLE_TRACKER_BLOCK: Lazy<RwLock<Option<Vec<String>>>> = Lazy::new(|| RwLock::new(None));
/// UI line count last written to `InlineHandle` by the tracker transcript
/// helper. Used instead of deriving a UI `replace_last` count from TRANSCRIPT
/// (the two stores can diverge when other writers hit only one side).
static REPLACEABLE_TRACKER_UI_LEN: Lazy<RwLock<Option<usize>>> = Lazy::new(|| RwLock::new(None));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TranscriptMode {
    #[expect(
        dead_code,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    Normal,
    Suppressed,
}

thread_local! {
    static MODE_STACK: RefCell<Vec<TranscriptMode>> = const { RefCell::new(Vec::new()) };
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
                    debug_assert!(false, "transcript suspension stack corrupted: expected Suppressed");
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

static MESSAGE_QUEUE: Lazy<RwLock<VecDeque<QueuedMessage>>> = Lazy::new(|| RwLock::new(VecDeque::new()));
/// Messages enqueued while no inline handle was attached. Drained FIFO on
/// `set_inline_handle` so none are lost. Bounded like `MESSAGE_QUEUE`.
static PENDING_QUEUE: Lazy<RwLock<VecDeque<QueuedMessage>>> = Lazy::new(|| RwLock::new(VecDeque::new()));

pub fn append(line: &str) {
    if is_suppressed() || line.trim().is_empty() {
        return;
    }
    let mut log = TRANSCRIPT.write();
    if log.len() == MAX_LINES {
        let drop_count = MAX_LINES / 5;
        log.drain(0..drop_count);
    }
    if log.last().is_some_and(|last| last == line) {
        return;
    }
    log.push(line.to_string());
}

pub fn replace_last(count: usize, lines: &[String]) {
    if is_suppressed() {
        return;
    }
    let mut log = TRANSCRIPT.write();
    let new_len = log.len().saturating_sub(count);
    log.truncate(new_len);
    for line in lines {
        if log.len() == MAX_LINES {
            let drop_count = MAX_LINES / 5;
            log.drain(0..drop_count);
        }
        log.push(line.clone());
    }
}

pub fn tail_matches(lines: &[String]) -> bool {
    if lines.is_empty() {
        return false;
    }

    let log = TRANSCRIPT.read();
    if lines.len() > log.len() {
        return false;
    }

    log[log.len() - lines.len()..]
        .iter()
        .zip(lines.iter())
        .all(|(left, right)| left == right)
}

/// Remember the current user-facing tracker transcript block.
///
/// `ui_line_count` is how many UI lines the tracker helper last wrote to
/// `InlineHandle` for this block (used for tail-safe UI replace).
pub fn remember_tracker_block_with_ui_len(lines: Vec<String>, ui_line_count: usize) {
    let has_lines = !lines.is_empty();
    *REPLACEABLE_TRACKER_BLOCK.write() = has_lines.then_some(lines);
    *REPLACEABLE_TRACKER_UI_LEN.write() = has_lines.then_some(ui_line_count);
}

/// Remember the current user-facing tracker transcript block (UI length = line count).
pub fn remember_tracker_block(lines: Vec<String>) {
    let ui_len = lines.len();
    remember_tracker_block_with_ui_len(lines, ui_len);
}

/// Line count of the remembered tracker transcript block, if any.
pub fn tracker_block_len() -> Option<usize> {
    REPLACEABLE_TRACKER_BLOCK.read().as_ref().map(|lines| lines.len())
}

/// UI write length last recorded for the tracker block, if any.
pub fn tracker_ui_write_len() -> Option<usize> {
    *REPLACEABLE_TRACKER_UI_LEN.read()
}

/// Whether `lines` match the remembered tracker transcript block exactly.
pub fn tracker_block_matches(lines: &[String]) -> bool {
    REPLACEABLE_TRACKER_BLOCK.read().as_deref() == Some(lines)
}

/// Length of the remembered tracker block **only if** it is still the transcript tail.
pub fn tracker_block_len_if_at_tail() -> Option<usize> {
    let remembered = REPLACEABLE_TRACKER_BLOCK.read().clone()?;
    if tail_matches(&remembered) {
        Some(remembered.len())
    } else {
        None
    }
}

/// Clear remembered tracker replace state (tests / session teardown).
pub fn clear_tracker_block() {
    *REPLACEABLE_TRACKER_BLOCK.write() = None;
    *REPLACEABLE_TRACKER_UI_LEN.write() = None;
}

/// Last non-empty transcript line, if any.
pub fn last_line() -> Option<String> {
    TRANSCRIPT.read().iter().rev().find(|line| !line.trim().is_empty()).cloned()
}

pub fn snapshot() -> Vec<String> {
    TRANSCRIPT.read().clone()
}

pub fn len() -> usize {
    TRANSCRIPT.read().len()
}

pub fn clear() {
    TRANSCRIPT.write().clear();
    clear_tracker_block();
}

/// Set the inline handle for immediate message display.
///
/// Any messages enqueued while no handle was attached are replayed FIFO
/// exactly once, in order, so none are lost. The handle is captured before
/// the drain so a concurrent `clear_inline_handle` cannot drop already-taken
/// pending messages.
pub fn set_inline_handle(handle: Arc<InlineHandle>) {
    *INLINE_HANDLE.write() = Some(handle.clone());
    let pending: Vec<QueuedMessage> = PENDING_QUEUE.write().drain(..).collect();
    for msg in pending {
        display_message_to(&handle, &msg.text, msg.kind, &msg.style);
    }
}

/// Remove the inline handle
pub fn clear_inline_handle() {
    *INLINE_HANDLE.write() = None;
}

/// Map MessageStyle to InlineMessageKind
fn message_kind(style: MessageStyle) -> InlineMessageKind {
    style.message_kind()
}

/// Enqueue a message with a specific style and display it immediately
pub fn enqueue_message(message: &str, style: MessageStyle) {
    enqueue_message_with_kind(message, message_kind(style), InlineTextStyle::default())
}

/// Enqueue a message with a specific kind and display it immediately
pub fn enqueue_message_with_kind(message: &str, kind: InlineMessageKind, text_style: InlineTextStyle) {
    if message.trim().is_empty() {
        return;
    }

    let queued = QueuedMessage { text: message.to_string(), kind, style: text_style };

    // Record history (bounded FIFO, drops oldest on overflow).
    {
        let mut queue = MESSAGE_QUEUE.write();
        if queue.len() >= MAX_QUEUE_SIZE {
            queue.pop_front();
        }
        queue.push_back(queued.clone());
    }

    // Retain FIFO pending first, then drain for display when a handle is
    // attached. Never re-read `.back()`: under concurrency that shows the
    // last writer's message twice while dropping earlier ones. Pushing
    // before the handle check closes the lost-wakeup race where `set_inline_handle`
    // drains between the check and the push; the atomic drain below guarantees
    // each pending message displays exactly once across both paths.
    {
        let mut pending = PENDING_QUEUE.write();
        if pending.len() >= MAX_QUEUE_SIZE {
            pending.pop_front();
        }
        pending.push_back(queued);
    }
    if let Some(handle) = INLINE_HANDLE.read().clone() {
        let pending: Vec<QueuedMessage> = PENDING_QUEUE.write().drain(..).collect();
        for msg in pending {
            display_message_to(&handle, &msg.text, msg.kind, &msg.style);
        }
    }

    // Also add to transcript for persistence (plain text)
    append(message);
}

/// Display an error message in the transcript instead of the input field
#[cold]
pub fn display_error(message: &str) {
    enqueue_message(message, MessageStyle::Error);
}

/// Display an info message in the transcript instead of the input field
pub fn display_info(message: &str) {
    enqueue_message(message, MessageStyle::Info);
}

/// Display a message immediately without queuing (low-level function)
fn display_message_now(text: &str, kind: InlineMessageKind, style: &InlineTextStyle) {
    if let Some(handle) = INLINE_HANDLE.read().as_ref() {
        display_message_to(handle, text, kind, style);
    }
}

/// Append one message to a specific handle. Used by drain paths that already
/// captured the handle so a concurrent detach cannot swallow the message.
fn display_message_to(handle: &InlineHandle, text: &str, kind: InlineMessageKind, style: &InlineTextStyle) {
    handle.append_line(
        kind,
        vec![InlineSegment {
            text: text.to_string(),
            style: Arc::new(style.clone()),
        }],
    );
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
    MESSAGE_QUEUE.read().iter().map(|m| m.text.clone()).collect()
}

/// Get all queued messages with their metadata
pub fn get_queued_messages_with_metadata() -> Vec<(String, InlineMessageKind)> {
    MESSAGE_QUEUE.read().iter().map(|m| (m.text.clone(), m.kind)).collect()
}

/// Clear the message queue (history and undisplayed pending).
pub fn clear_queue() {
    MESSAGE_QUEUE.write().clear();
    PENDING_QUEUE.write().clear();
}

/// Get queue length (history length).
pub fn queue_len() -> usize {
    MESSAGE_QUEUE.read().len()
}

/// Get pending (undisplayed) queue length.
pub fn pending_queue_len() -> usize {
    PENDING_QUEUE.read().len()
}

/// Atomically take every queued message FIFO and clear the history queue.
/// Prefer this over `get_queued_messages` + `clear_queue`: the snapshot
/// pattern races and callers that only use `.last()` drop all but the
/// newest message.
pub fn drain_queued_messages() -> Vec<String> {
    PENDING_QUEUE.write().clear();
    MESSAGE_QUEUE.write().drain(..).map(|m| m.text).collect()
}

/// Atomically take every queued message with metadata FIFO and clear.
pub fn drain_queued_messages_with_metadata() -> Vec<(String, InlineMessageKind)> {
    PENDING_QUEUE.write().clear();
    MESSAGE_QUEUE.write().drain(..).map(|m| (m.text, m.kind)).collect()
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
    #[serial_test::serial(transcript_state)]
    fn append_and_snapshot_store_lines() {
        clear();
        append("first");
        append("second");
        assert_eq!(len(), 2);
        let snap = snapshot();
        assert_eq!(snap, vec!["first".to_owned(), "second".to_owned()]);
        clear();
    }

    #[test]
    #[serial_test::serial(transcript_state)]
    fn append_skips_adjacent_duplicate_lines() {
        clear();
        append("duplicate");
        append("duplicate");
        append("different");
        append("duplicate");
        let snap = snapshot();
        assert_eq!(snap, vec!["duplicate".to_owned(), "different".to_owned(), "duplicate".to_owned()]);
        clear();
    }

    #[test]
    #[serial_test::serial(transcript_state)]
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
        assert_eq!(snap.first().unwrap(), &format!("line {}", MAX_LINES / 5).to_owned());
        clear();
    }

    #[test]
    #[serial_test::serial(transcript_state)]
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
    #[serial_test::serial(transcript_state)]
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
    #[serial_test::serial(transcript_state)]
    fn message_queue_size_limit() {
        clear_queue();

        // Fill queue to max capacity
        for i in 0..MAX_QUEUE_SIZE {
            enqueue(&format!("message {i}"));
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
    #[serial_test::serial(transcript_state)]
    fn suppressed_scope_skips_transcript_entries() {
        clear();
        with_suppressed(|| {
            append("hidden");
        });
        append("visible");
        let snap = snapshot();
        assert_eq!(snap, vec!["visible".to_owned()]);
        clear();
    }

    #[test]
    #[serial_test::serial(transcript_state)]
    fn tracker_block_store_replaces_and_clears() {
        clear();
        assert_eq!(tracker_block_len(), None);
        remember_tracker_block(vec!["• Plan 0/1".to_string()]);
        assert_eq!(tracker_block_len(), Some(1));
        assert!(tracker_block_matches(&["• Plan 0/1".to_string()]));
        remember_tracker_block(vec!["• Plan 1/1".to_string()]);
        assert!(!tracker_block_matches(&["• Plan 0/1".to_string()]));
        clear();
        assert_eq!(tracker_block_len(), None);
    }

    #[test]
    #[serial_test::serial(transcript_state)]
    fn drain_queued_messages_returns_all_fifo_and_clears() {
        clear();
        clear_queue();
        clear_inline_handle();
        assert_eq!(pending_queue_len(), 0);

        enqueue("alpha");
        enqueue("beta");
        enqueue("gamma");

        assert_eq!(queue_len(), 3);
        assert_eq!(pending_queue_len(), 3);
        assert_eq!(get_queued_messages(), vec!["alpha".to_owned(), "beta".to_owned(), "gamma".to_owned()]);

        let drained = drain_queued_messages();
        assert_eq!(drained, vec!["alpha".to_owned(), "beta".to_owned(), "gamma".to_owned()]);
        assert_eq!(queue_len(), 0);
        assert_eq!(pending_queue_len(), 0);
        assert!(get_queued_messages().is_empty());

        clear();
        clear_queue();
        clear_inline_handle();
    }

    #[test]
    #[serial_test::serial(transcript_state)]
    fn drain_queued_messages_with_metadata_preserves_order() {
        clear();
        clear_queue();
        clear_inline_handle();

        enqueue_message("first", MessageStyle::Info);
        enqueue_message("second", MessageStyle::Error);

        let drained = drain_queued_messages_with_metadata();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].0, "first");
        assert_eq!(drained[0].1, InlineMessageKind::Info);
        assert_eq!(drained[1].0, "second");
        assert_eq!(drained[1].1, InlineMessageKind::Error);
        assert_eq!(queue_len(), 0);

        clear();
        clear_queue();
        clear_inline_handle();
    }

    #[test]
    #[serial_test::serial(transcript_state)]
    fn pending_queue_replays_fifo_once_on_handle_set() {
        use crate::ui::InlineCommand;

        clear();
        clear_queue();
        clear_inline_handle();

        enqueue("first pending");
        enqueue("second pending");
        assert_eq!(pending_queue_len(), 2);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        set_inline_handle(Arc::new(handle));
        assert_eq!(pending_queue_len(), 0);
        // History is retained for inspection; pending is what was replayed.
        assert_eq!(queue_len(), 2);

        let mut texts = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            if let InlineCommand::AppendLine { segments, .. } = cmd {
                texts.extend(segments.into_iter().map(|s| s.text));
            }
        }
        assert_eq!(texts, vec!["first pending".to_owned(), "second pending".to_owned()]);

        // Second handle attach must not duplicate replay.
        let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();
        let handle2 = InlineHandle::new_for_tests(tx2);
        set_inline_handle(Arc::new(handle2));
        assert!(rx2.try_recv().is_err());

        clear();
        clear_queue();
        clear_inline_handle();
    }

    #[test]
    #[serial_test::serial(transcript_state)]
    fn immediate_display_uses_each_message_not_last_only() {
        use crate::ui::InlineCommand;

        clear();
        clear_queue();
        clear_inline_handle();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        set_inline_handle(Arc::new(handle));

        enqueue("one");
        enqueue("two");
        assert_eq!(pending_queue_len(), 0);

        let mut texts = Vec::new();
        while let Ok(cmd) = rx.try_recv() {
            if let InlineCommand::AppendLine { segments, .. } = cmd {
                texts.extend(segments.into_iter().map(|s| s.text));
            }
        }
        assert_eq!(texts, vec!["one".to_owned(), "two".to_owned()]);

        clear();
        clear_queue();
        clear_inline_handle();
    }
}
