use std::collections::VecDeque;

use vtcode_tui::InlineHandle;

pub(crate) struct InlineQueueState<'a> {
    handle: &'a InlineHandle,
    queued_inputs: &'a mut VecDeque<String>,
    prefer_latest_once: &'a mut bool,
}

impl<'a> InlineQueueState<'a> {
    pub(crate) fn new(
        handle: &'a InlineHandle,
        queued_inputs: &'a mut VecDeque<String>,
        prefer_latest_once: &'a mut bool,
    ) -> Self {
        Self {
            handle,
            queued_inputs,
            prefer_latest_once,
        }
    }

    pub(crate) fn push(&mut self, text: String) {
        self.queued_inputs.push_back(text);
        *self.prefer_latest_once = true;
        self.sync_handle_queue();
    }

    pub(crate) fn take_next_submission(&mut self) -> Option<String> {
        let result = if *self.prefer_latest_once {
            *self.prefer_latest_once = false;
            self.queued_inputs.pop_back()
        } else {
            self.queued_inputs.pop_front()
        };
        self.sync_handle_queue();
        result
    }

    pub(crate) fn prefer_latest_next(&mut self) {
        *self.prefer_latest_once = !self.queued_inputs.is_empty();
    }

    pub(crate) fn edit_latest(&mut self) -> Option<String> {
        let result = self.queued_inputs.pop_back();
        if result.is_some() {
            *self.prefer_latest_once = false;
        }
        self.sync_handle_queue();
        result
    }

    pub(crate) fn clear(&mut self) {
        self.queued_inputs.clear();
        *self.prefer_latest_once = false;
        self.sync_handle_queue();
    }

    fn sync_handle_queue(&self) {
        self.handle
            .set_queued_inputs(self.queued_inputs.iter().cloned().collect());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_submission_runs_once_then_queue_returns_to_fifo() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.push("first".to_string());
        queue.push("second".to_string());
        queue.push("third".to_string());

        assert_eq!(queue.take_next_submission().as_deref(), Some("third"));
        assert_eq!(queue.take_next_submission().as_deref(), Some("first"));
        assert_eq!(queue.take_next_submission().as_deref(), Some("second"));
    }

    #[test]
    fn prefer_latest_next_promotes_existing_queue_without_reordering_it() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::from([
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ]);
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.prefer_latest_next();

        assert_eq!(queue.take_next_submission().as_deref(), Some("third"));
        assert_eq!(queue.take_next_submission().as_deref(), Some("first"));
        assert_eq!(queue.take_next_submission().as_deref(), Some("second"));
    }
}
