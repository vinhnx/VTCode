use std::collections::VecDeque;
use std::sync::Arc;

use vtcode_ui::tui::app::{InlineHandle, InlineMessageKind, InlineSegment, InlineTextStyle, SubmittedInput};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueuedInput {
    pub(crate) input: SubmittedInput,
    pub(crate) primary_agent: Option<String>,
    /// Only Ctrl+Enter submissions are batchable; plain-Enter submissions
    /// queued while busy must dispatch as their own turn.
    pub(crate) batchable: bool,
}

impl QueuedInput {
    pub(crate) fn new(input: SubmittedInput, primary_agent: Option<String>) -> Self {
        Self {
            batchable: input.batchable,
            input,
            primary_agent: primary_agent.filter(|name| !name.trim().is_empty()),
        }
    }

    pub(crate) fn display_label(&self) -> String {
        match self.primary_agent.as_deref() {
            Some(agent) => format!("{agent}: {}", self.input.text),
            None => self.input.text.clone(),
        }
    }
}

/// Soft cap on concurrently queued user inputs. Under a paste-storm the
/// oldest entry is dropped once the cap is exceeded so the authoritative
/// VecDeque cannot grow without bound; the newest submissions are kept.
/// Drops are coalesced into one visible warning per `flush_sync` so the
/// discard is never silent.
pub(crate) const MAX_QUEUED_INPUTS: usize = 256;

pub(crate) struct InlineQueueState<'a> {
    handle: &'a InlineHandle,
    queued_inputs: &'a mut VecDeque<QueuedInput>,
    prefer_latest_once: &'a mut bool,
    /// When true, the TUI overlay is stale relative to `queued_inputs` and
    /// needs `flush_sync`. Deferred so a drain of N QueueSubmit events does
    /// not publish N partial snapshots that make optimistic UI entries
    /// flicker out of existence between acknowledgements.
    sync_dirty: bool,
    /// Oldest inputs discarded by the soft cap since the last warning.
    /// Coalesced so a paste-storm emits one notice, not one per drop.
    dropped_since_notice: usize,
}

impl<'a> InlineQueueState<'a> {
    pub(crate) fn new(
        handle: &'a InlineHandle,
        queued_inputs: &'a mut VecDeque<QueuedInput>,
        prefer_latest_once: &'a mut bool,
    ) -> Self {
        Self {
            handle,
            queued_inputs,
            prefer_latest_once,
            sync_dirty: false,
            dropped_since_notice: 0,
        }
    }

    pub(crate) fn push(&mut self, input: SubmittedInput, primary_agent: Option<String>) {
        self.queued_inputs.push_back(QueuedInput::new(input, primary_agent));
        // Enforce a soft FIFO cap: drop the oldest only after the newest has
        // been accepted so rapid influx cannot grow the queue without bound.
        while self.queued_inputs.len() > MAX_QUEUED_INPUTS {
            self.queued_inputs.pop_front();
            self.dropped_since_notice += 1;
        }
        self.mark_sync_dirty();
    }

    pub(crate) fn take_next_submission(&mut self) -> Option<QueuedInput> {
        let result = if *self.prefer_latest_once {
            *self.prefer_latest_once = false;
            self.queued_inputs.pop_back()
        } else {
            self.queued_inputs.pop_front()
        };
        self.mark_sync_dirty();
        result
    }

    /// Pop the next submission plus every following batchable text-only
    /// submission for the same agent, joined into ONE combined prompt so
    /// several queued Ctrl+Enter messages reach the model in a single turn
    /// instead of one turn each. Non-batchable items (plain Enter, attachments,
    /// agent changes) stop the batch and dispatch alone on their own turns.
    pub(crate) fn take_batched_submission(&mut self) -> Option<QueuedInput> {
        // A Ctrl+Enter "run the latest now" promotion must stay a single-turn
        // dispatch: merging the newest item with older FIFO items would batch
        // turns the user asked to run individually.
        let promoted_latest = *self.prefer_latest_once;
        let mut batch = self.take_next_submission()?;
        // Only batchable (Ctrl+Enter) submissions coalesce, and never a
        // submission carrying attachments — the association between an image
        // and its message must stay intact. A plain Enter that reached the
        // front must also dispatch alone and never absorb following items.
        if !batch.batchable || promoted_latest || batch.input.has_attachments() {
            tracing::debug!(
                target: "vtcode_ui::queue",
                batched_count = 1,
                text_bytes = batch.input.text.len(),
                "queue submission drained (single, non-batchable)"
            );
            self.mark_sync_dirty();
            return Some(batch);
        }
        let primary_agent = batch.primary_agent.clone();
        let mut batched_count = 1usize;
        const MAX_BATCH_ITEMS: usize = 32;
        // Cap the combined prompt so a user hammering Ctrl+Enter hundreds of
        // times cannot build one unbounded message.
        const MAX_BATCH_BYTES: usize = 64 * 1024;
        while batched_count < MAX_BATCH_ITEMS {
            let Some(next) = self.queued_inputs.front() else {
                break;
            };
            if !next.batchable || next.input.has_attachments() || next.primary_agent != primary_agent {
                break;
            }
            // Cap the COMBINED prompt: merging `next` must not exceed the
            // budget, otherwise one large queued message could overshoot it.
            let needs_separator = !batch.input.text.trim().is_empty() && !next.input.text.trim().is_empty();
            let merged_len = batch.input.text.len() + usize::from(needs_separator) * 2 + next.input.text.len();
            if merged_len > MAX_BATCH_BYTES {
                break;
            }
            let Some(next) = self.queued_inputs.pop_front() else {
                break;
            };
            batched_count += 1;
            if needs_separator {
                batch.input.text.push_str("\n\n");
            }
            batch.input.text.push_str(&next.input.text);
        }
        tracing::debug!(
            target: "vtcode_ui::queue",
            batched_count,
            text_bytes = batch.input.text.len(),
            "queue submission drained"
        );
        self.mark_sync_dirty();
        Some(batch)
    }

    pub(crate) fn prefer_latest_next(&mut self) {
        *self.prefer_latest_once = !self.queued_inputs.is_empty();
    }

    pub(crate) fn edit_latest(&mut self) -> Option<String> {
        let result = self.queued_inputs.pop_back().map(|queued| queued.input.text);
        if result.is_some() {
            *self.prefer_latest_once = false;
        }
        self.mark_sync_dirty();
        result
    }

    #[allow(
        dead_code,
        reason = "Explicit clear-all path for future queue UI; interrupts preserve by design."
    )]
    pub(crate) fn clear(&mut self) {
        self.queued_inputs.clear();
        *self.prefer_latest_once = false;
        self.mark_sync_dirty();
    }

    /// Preserve queued inputs across an interrupt: reset any one-shot
    /// latest-promotion without dropping user-queued work. Returns the
    /// preserved count so the interrupt notice can report it instead of
    /// silently clearing the queue.
    pub(crate) fn preserve_on_interrupt(&mut self) -> usize {
        *self.prefer_latest_once = false;
        self.mark_sync_dirty();
        self.queued_inputs.len()
    }

    /// Publish the authoritative FIFO to the TUI overlay if it is stale.
    ///
    /// Call this once after a drain batch (and before dispatching a queued
    /// submission) so the overlay sees one consistent snapshot instead of
    /// N partial ones that race against optimistic UI entries. Also emits one
    /// coalesced warning when the soft cap discarded older inputs.
    pub(crate) fn flush_sync(&mut self) {
        self.notice_dropped_inputs();
        if !self.sync_dirty {
            return;
        }
        self.sync_dirty = false;
        self.sync_handle_queue();
    }

    fn notice_dropped_inputs(&mut self) {
        if self.dropped_since_notice == 0 {
            return;
        }
        let dropped = self.dropped_since_notice;
        self.dropped_since_notice = 0;
        let message = format!(
            "Queue full (cap {MAX_QUEUED_INPUTS}): dropped {dropped} older queued input(s); kept the newest {kept}.",
            kept = self.queued_inputs.len()
        );
        self.handle.append_line(
            InlineMessageKind::Warning,
            vec![InlineSegment {
                text: message,
                style: Arc::new(InlineTextStyle::default()),
            }],
        );
    }

    fn mark_sync_dirty(&mut self) {
        self.sync_dirty = true;
    }

    fn sync_handle_queue(&self) {
        self.handle
            .set_queued_inputs(self.queued_inputs.iter().map(QueuedInput::display_label).collect());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pushes_drain_in_fifo_order() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.push("first".into(), Some("duck".to_string()));
        queue.push("second".into(), Some("build".to_string()));
        queue.push("third".into(), Some("review".to_string()));

        // The queue is strict FIFO: first queued runs first.
        assert_eq!(queue.take_next_submission().map(|queued| queued.input.text).as_deref(), Some("first"));
        assert_eq!(queue.take_next_submission().map(|queued| queued.input.text).as_deref(), Some("second"));
        assert_eq!(queue.take_next_submission().map(|queued| queued.input.text).as_deref(), Some("third"));
    }

    #[test]
    fn prefer_latest_next_promotes_existing_queue_without_reordering_it() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::from([
            QueuedInput::new("first".into(), Some("duck".to_string())),
            QueuedInput::new("second".into(), Some("build".to_string())),
            QueuedInput::new("third".into(), Some("review".to_string())),
        ]);
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.prefer_latest_next();

        assert_eq!(queue.take_next_submission().map(|queued| queued.input.text).as_deref(), Some("third"));
        assert_eq!(queue.take_next_submission().map(|queued| queued.input.text).as_deref(), Some("first"));
        assert_eq!(queue.take_next_submission().map(|queued| queued.input.text).as_deref(), Some("second"));
    }

    #[test]
    fn take_batched_submission_joins_consecutive_batchable_items() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        for text in ["first", "second", "third"] {
            queue.push(SubmittedInput::new(text, Vec::new()).batchable(), None);
        }

        // The queue is strict FIFO: queued order is preserved inside the batch.
        let batched = queue.take_batched_submission().expect("batched submission");
        assert_eq!(batched.input.text, "first\n\nsecond\n\nthird");
        assert!(queue.take_batched_submission().is_none());
    }

    #[test]
    fn take_batched_submission_stops_at_plain_enter_attachments_or_agent_change() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.push("text one".into(), Some("planner".to_string()));
        queue.push(
            SubmittedInput::new("with image", vec![vtcode_ui::tui::app::ContentPart::image("img", "image/png")]),
            None,
        );
        queue.push(SubmittedInput::new("text two", Vec::new()).batchable(), None);

        // Strict FIFO: the planner-tagged plain-Enter item runs first alone,
        // then the attachment item alone (it stops batching), then the
        // batchable item alone because the queue behind it is already empty.
        let planner_item = queue.take_batched_submission().expect("planner submission");
        assert_eq!(planner_item.input.text, "text one");

        let with_image = queue.take_batched_submission().expect("attachment submission");
        assert_eq!(with_image.input.text, "with image");

        let batched = queue.take_batched_submission().expect("last submission");
        assert_eq!(batched.input.text, "text two");

        assert!(queue.take_batched_submission().is_none());
    }

    #[test]
    fn take_batched_submission_runs_plain_enter_items_as_their_own_turns() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.push(SubmittedInput::new("batch me", Vec::new()).batchable(), None);
        queue.push("plain enter one".into(), None);
        queue.push("plain enter two".into(), None);

        // Strict FIFO: the batchable item runs alone because the plain-Enter
        // item behind it stops the batch, then each plain-Enter item runs as
        // its own turn.
        let first = queue.take_batched_submission().expect("first submission");
        assert_eq!(first.input.text, "batch me");

        let second = queue.take_batched_submission().expect("second submission");
        assert_eq!(second.input.text, "plain enter one");

        let third = queue.take_batched_submission().expect("third submission");
        assert_eq!(third.input.text, "plain enter two");

        assert!(queue.take_batched_submission().is_none());
    }

    #[test]
    fn take_batched_submission_keeps_attachment_head_single() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.push(
            SubmittedInput::new("see image", vec![vtcode_ui::tui::app::ContentPart::image("img", "image/png")]),
            None,
        );
        queue.push(SubmittedInput::new("follow-up", Vec::new()).batchable(), None);

        // The attachment-carrying head must dispatch alone so the image stays
        // associated with its message; the following batchable item may then
        // run alone (nothing else behind it).
        let first = queue.take_batched_submission().expect("attachment head");
        assert_eq!(first.input.text, "see image");
        assert!(first.input.has_attachments());

        let second = queue.take_batched_submission().expect("follow-up");
        assert_eq!(second.input.text, "follow-up");
        assert!(queue.take_batched_submission().is_none());
    }

    #[test]
    fn take_batched_submission_does_not_merge_after_prefer_latest_promotion() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.push(SubmittedInput::new("first", Vec::new()).batchable(), None);
        queue.push(SubmittedInput::new("second", Vec::new()).batchable(), None);
        queue.push(SubmittedInput::new("third", Vec::new()).batchable(), None);

        // Ctrl+Enter with empty input while idle promotes the newest item to
        // run alone NOW; it must not absorb the older items into one batch.
        queue.prefer_latest_next();
        let promoted = queue.take_batched_submission().expect("promoted submission");
        assert_eq!(promoted.input.text, "third");

        // Remaining queue drains in strict FIFO inside one batch.
        let batch = queue.take_batched_submission().expect("remaining batch");
        assert_eq!(batch.input.text, "first\n\nsecond");
        assert!(queue.take_batched_submission().is_none());
    }

    #[test]
    fn take_batched_submission_skips_separator_for_whitespace_items() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.push(SubmittedInput::new("first", Vec::new()).batchable(), None);
        queue.push(SubmittedInput::new("   ", Vec::new()).batchable(), None);
        queue.push(SubmittedInput::new("third", Vec::new()).batchable(), None);

        let batch = queue.take_batched_submission().expect("batch");
        // The whitespace-only item is still its own message: no separator is
        // injected for it, but it must remain distinct from "third".
        assert_eq!(batch.input.text, "first   \n\nthird");
    }

    #[test]
    fn take_batched_submission_caps_batch_size() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        // 100 batchable items must not become one unbounded prompt.
        for i in 0..100 {
            queue.push(SubmittedInput::new(format!("msg {i}"), Vec::new()).batchable(), None);
        }

        let batch = queue.take_batched_submission().expect("batch");
        let item_count = batch.input.text.split("\n\n").count();
        assert!(item_count <= 32, "batch must be capped, got {item_count} items");
        assert!(queue.take_batched_submission().is_some(), "leftover items must still drain");
    }

    #[test]
    fn queued_input_keeps_primary_agent_captured_at_queue_time() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.push("first".into(), Some("planner".to_string()));
        queue.push("second".into(), Some("builder".to_string()));

        let first = queue.take_next_submission().expect("first queued input");
        assert_eq!(first.input.text, "first");
        assert_eq!(first.primary_agent.as_deref(), Some("planner"));

        let latest = queue.take_next_submission().expect("second queued input");
        assert_eq!(latest.input.text, "second");
        assert_eq!(latest.primary_agent.as_deref(), Some("builder"));
    }

    #[test]
    fn queued_input_preserves_attachments_in_order() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        let first = vtcode_ui::tui::app::ContentPart::image("first", "image/png");
        let second = vtcode_ui::tui::app::ContentPart::image("second", "image/jpeg");
        queue.push(SubmittedInput::new("see images", vec![first.clone(), second.clone()]), None);

        let queued = queue.take_next_submission().expect("queued input");
        assert_eq!(queued.input.text, "see images");
        assert_eq!(queued.input.attachments, vec![first, second]);
    }

    #[test]
    fn preserve_on_interrupt_keeps_fifo_and_resets_promotion() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.push("first".into(), None);
        queue.push("second".into(), None);
        queue.prefer_latest_next();

        let preserved = queue.preserve_on_interrupt();
        assert_eq!(preserved, 2);

        // Promotion reset: oldest dispatches first, not the newest.
        // FIFO order intact after interrupt preservation.
        assert_eq!(queue.take_next_submission().map(|q| q.input.text).as_deref(), Some("first"));
        assert_eq!(queue.take_next_submission().map(|q| q.input.text).as_deref(), Some("second"));
        assert!(queue.take_next_submission().is_none());
    }

    #[test]
    fn flush_sync_publishes_one_snapshot_after_drain() {
        use vtcode_ui::tui::app::InlineCommand;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        // Rapid influx: three pushes, no intermediate overlay publishes.
        queue.push("first".into(), None);
        queue.push("second".into(), None);
        queue.push("third".into(), None);
        assert!(rx.try_recv().is_err(), "push must not publish partial overlay snapshots");

        queue.flush_sync();
        match rx.try_recv() {
            Ok(InlineCommand::SetQueuedInputs { entries }) => {
                assert_eq!(entries, vec!["first".to_string(), "second".to_string(), "third".to_string()]);
            }
            Ok(_) => panic!("expected SetQueuedInputs snapshot"),
            Err(err) => panic!("expected one SetQueuedInputs snapshot, got {err:?}"),
        }
        assert!(rx.try_recv().is_err(), "flush must publish exactly one snapshot");
    }

    #[test]
    fn push_caps_fifo_and_keeps_newest() {
        use vtcode_ui::tui::app::InlineCommand;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;

        {
            let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
            for index in 0..(MAX_QUEUED_INPUTS + 10) {
                queue.push(format!("msg {index}").into(), None);
            }
            queue.flush_sync();
        }
        assert_eq!(queued_inputs.len(), MAX_QUEUED_INPUTS);
        assert_eq!(queued_inputs.front().map(|q| q.input.text.as_str()), Some("msg 10"));
        assert_eq!(
            queued_inputs.back().map(|q| q.input.text.as_str()),
            Some(format!("msg {}", MAX_QUEUED_INPUTS + 9).as_str())
        );

        // Drops must be visible: one coalesced warning plus one overlay snapshot.
        let mut warnings = Vec::new();
        let mut snapshots = 0usize;
        while let Ok(command) = rx.try_recv() {
            match command {
                InlineCommand::Diagnostic(diagnostic)
                    if diagnostic.severity == vtcode_commons::diagnostics::DiagnosticSeverity::Warning =>
                {
                    warnings.push(diagnostic.summary);
                }
                InlineCommand::SetQueuedInputs { .. } => snapshots += 1,
                _ => {}
            }
        }
        assert_eq!(snapshots, 1, "flush must publish one overlay snapshot");
        assert_eq!(warnings.len(), 1, "dropped inputs must emit exactly one coalesced warning");
        assert!(
            warnings[0].contains("dropped 10 older queued input(s)"),
            "warning must name the drop count: {}",
            warnings[0]
        );
        assert!(
            warnings[0].contains(&format!("cap {MAX_QUEUED_INPUTS}")),
            "warning must name the cap: {}",
            warnings[0]
        );
    }

    #[test]
    fn flush_without_drops_emits_no_warning() {
        use vtcode_ui::tui::app::InlineCommand;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        {
            let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
            queue.push("only".into(), None);
            queue.flush_sync();
        }

        while let Ok(command) = rx.try_recv() {
            match command {
                InlineCommand::AppendLine { kind, .. } => {
                    panic!("no warning expected without drops, got {kind:?}");
                }
                InlineCommand::SetQueuedInputs { entries } => {
                    assert_eq!(entries, vec!["only".to_string()]);
                }
                _ => {}
            }
        }
    }
}
