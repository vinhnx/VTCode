use std::collections::VecDeque;

use vtcode_core::ui::tui::InlineHandle;

pub(crate) struct InlineQueueState<'a> {
    handle: &'a InlineHandle,
    queued_inputs: &'a mut VecDeque<String>,
}

impl<'a> InlineQueueState<'a> {
    pub(crate) fn new(handle: &'a InlineHandle, queued_inputs: &'a mut VecDeque<String>) -> Self {
        Self {
            handle,
            queued_inputs,
        }
    }

    pub(crate) fn push(&mut self, text: String) {
        self.queued_inputs.push_back(text);
        self.sync_handle_queue();
    }

    pub(crate) fn pop_front(&mut self) -> Option<String> {
        let result = self.queued_inputs.pop_front();
        self.sync_handle_queue();
        result
    }

    pub(crate) fn clear(&mut self) {
        self.queued_inputs.clear();
        self.sync_handle_queue();
    }

    fn sync_handle_queue(&self) {
        self.handle
            .set_queued_inputs(self.queued_inputs.iter().cloned().collect());
    }
}
