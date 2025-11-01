use super::action::InlineLoopAction;
use super::queue::InlineQueueState;
use super::state::InlineEventState;

pub(crate) struct InlineInputProcessor<'a, 'state> {
    state: &'a mut InlineEventState<'state>,
}

impl<'a, 'state> InlineInputProcessor<'a, 'state> {
    pub(crate) fn new(state: &'a mut InlineEventState<'state>) -> Self {
        Self { state }
    }

    pub(crate) fn submit(mut self, text: String) -> InlineLoopAction {
        self.state.reset_interrupt_state();
        InlineLoopAction::Submit(text.trim().to_string())
    }

    pub(crate) fn queue_submit(
        mut self,
        text: String,
        queue: &mut InlineQueueState<'_>,
    ) -> InlineLoopAction {
        self.state.reset_interrupt_state();
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            return InlineLoopAction::Continue;
        }

        queue.push(trimmed);
        InlineLoopAction::Continue
    }

    pub(crate) fn passive(mut self) -> InlineLoopAction {
        self.state.reset_interrupt_state();
        InlineLoopAction::Continue
    }
}
