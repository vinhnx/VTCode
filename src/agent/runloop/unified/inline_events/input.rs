use super::action::InlineLoopAction;
use super::queue::InlineQueueState;
use super::state::InlineEventState;
use vtcode_ui::tui::app::SubmittedInput;

pub(crate) struct InlineInputProcessor<'a, 'state> {
    state: &'a mut InlineEventState<'state>,
}

impl<'a, 'state> InlineInputProcessor<'a, 'state> {
    pub(crate) fn new(state: &'a mut InlineEventState<'state>) -> Self {
        Self { state }
    }

    pub(crate) fn submit(self, input: SubmittedInput) -> InlineLoopAction {
        self.state.reset_interrupt_state();
        InlineLoopAction::Submit(input.trim_text())
    }

    pub(crate) fn queue_submit(
        self,
        input: SubmittedInput,
        queue: &mut InlineQueueState<'_>,
        primary_agent: Option<String>,
    ) -> InlineLoopAction {
        self.state.reset_interrupt_state();
        let input = input.trim_text();
        if input.is_empty() {
            return InlineLoopAction::Continue;
        }

        queue.push(input, primary_agent);
        InlineLoopAction::Continue
    }

    pub(crate) fn passive(self) -> InlineLoopAction {
        self.state.reset_interrupt_state();
        InlineLoopAction::Continue
    }
}
