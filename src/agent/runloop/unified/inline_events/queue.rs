use std::collections::VecDeque;

use vtcode_ui::tui::app::InlineHandle;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueuedInput {
    pub(crate) text: String,
    pub(crate) primary_agent: Option<String>,
}

impl QueuedInput {
    pub(crate) fn new(text: String, primary_agent: Option<String>) -> Self {
        Self {
            text,
            primary_agent: primary_agent.filter(|name| !name.trim().is_empty()),
        }
    }

    fn display_label(&self) -> String {
        match self.primary_agent.as_deref() {
            Some(agent) => format!("{agent}: {}", self.text),
            None => self.text.clone(),
        }
    }
}

pub(crate) struct InlineQueueState<'a> {
    handle: &'a InlineHandle,
    queued_inputs: &'a mut VecDeque<QueuedInput>,
    prefer_latest_once: &'a mut bool,
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
        }
    }

    pub(crate) fn push(&mut self, text: String, primary_agent: Option<String>) {
        self.queued_inputs
            .push_back(QueuedInput::new(text, primary_agent));
        *self.prefer_latest_once = true;
        self.sync_handle_queue();
    }

    pub(crate) fn take_next_submission(&mut self) -> Option<QueuedInput> {
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
        let result = self.queued_inputs.pop_back().map(|queued| queued.text);
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
        self.handle.set_queued_inputs(
            self.queued_inputs
                .iter()
                .map(QueuedInput::display_label)
                .collect(),
        );
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

        queue.push("first".to_string(), Some("duck".to_string()));
        queue.push("second".to_string(), Some("build".to_string()));
        queue.push("third".to_string(), Some("review".to_string()));

        assert_eq!(
            queue
                .take_next_submission()
                .map(|queued| queued.text)
                .as_deref(),
            Some("third")
        );
        assert_eq!(
            queue
                .take_next_submission()
                .map(|queued| queued.text)
                .as_deref(),
            Some("first")
        );
        assert_eq!(
            queue
                .take_next_submission()
                .map(|queued| queued.text)
                .as_deref(),
            Some("second")
        );
    }

    #[test]
    fn prefer_latest_next_promotes_existing_queue_without_reordering_it() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::from([
            QueuedInput::new("first".to_string(), Some("duck".to_string())),
            QueuedInput::new("second".to_string(), Some("build".to_string())),
            QueuedInput::new("third".to_string(), Some("review".to_string())),
        ]);
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.prefer_latest_next();

        assert_eq!(
            queue
                .take_next_submission()
                .map(|queued| queued.text)
                .as_deref(),
            Some("third")
        );
        assert_eq!(
            queue
                .take_next_submission()
                .map(|queued| queued.text)
                .as_deref(),
            Some("first")
        );
        assert_eq!(
            queue
                .take_next_submission()
                .map(|queued| queued.text)
                .as_deref(),
            Some("second")
        );
    }

    #[test]
    fn queued_input_keeps_primary_agent_captured_at_queue_time() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        queue.push("first".to_string(), Some("planner".to_string()));
        queue.push("second".to_string(), Some("builder".to_string()));

        let latest = queue.take_next_submission().expect("latest queued input");
        assert_eq!(latest.text, "second");
        assert_eq!(latest.primary_agent.as_deref(), Some("builder"));

        let first = queue.take_next_submission().expect("first queued input");
        assert_eq!(first.text, "first");
        assert_eq!(first.primary_agent.as_deref(), Some("planner"));
    }
}
