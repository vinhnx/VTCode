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
        self.state.reset_after_submission();
        InlineLoopAction::Submit(input.trim_text())
    }

    pub(crate) fn submit_prompt(self, input: SubmittedInput) -> InlineLoopAction {
        self.state.reset_after_submission();
        InlineLoopAction::SubmitPrompt(input.trim_text())
    }

    pub(crate) fn queue_submit(
        self,
        input: SubmittedInput,
        queue: &mut InlineQueueState<'_>,
        primary_agent: Option<String>,
    ) -> InlineLoopAction {
        self.state.reset_after_submission();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::inline_events::interrupts::InlineInterruptCoordinator;
    use crate::agent::runloop::unified::inline_events::state::InlineEventState;
    use crate::agent::runloop::unified::state::CtrlCState;
    use vtcode_core::core::interfaces::ui::UiSession;
    use vtcode_core::utils::ansi::AnsiRenderer;
    use vtcode_ui::tui::app::{InlineEvent, InlineHandle, InlineSession};

    #[test]
    fn webmcp_prompt_keeps_slash_commands_as_prompt_text() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
        let ctrl_c_state = CtrlCState::new();
        let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
        let mut ctrl_c_notice_displayed = false;
        let mut state = InlineEventState::new(&mut renderer, interrupts, &mut ctrl_c_notice_displayed);

        let action = InlineInputProcessor::new(&mut state).submit_prompt("/exit".into());

        assert!(matches!(action, InlineLoopAction::SubmitPrompt(input) if input.text == "/exit"));
    }

    #[test]
    fn passive_actions_preserve_cancellation_until_fresh_submission() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
        let ctrl_c_state = CtrlCState::new();
        ctrl_c_state.request_local_cancel();
        ctrl_c_state.mark_cancel_handled();
        let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
        let mut displayed = true;
        let mut state = InlineEventState::new(&mut renderer, interrupts, &mut displayed);
        InlineInputProcessor::new(&mut state).passive();
        assert!(ctrl_c_state.is_cancel_handled());
        InlineInputProcessor::new(&mut state).submit("continue".into());
        assert!(!ctrl_c_state.is_cancel_handled());
        ctrl_c_state.request_exit();
        InlineInputProcessor::new(&mut state).submit("continue".into());
        assert!(ctrl_c_state.is_exit_requested());
    }

    #[tokio::test]
    async fn deferred_webmcp_prompt_is_available_through_ui_session() {
        let (command_sender, _command_receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_sender);
        let (_event_sender, event_receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut session = InlineSession {
            handle: handle.clone(),
            events: event_receiver,
            worker: None,
        };

        handle
            .defer_event(InlineEvent::WebmcpSubmit("/exit".into()))
            .expect("defer bridge prompt");

        let event = UiSession::next_event(&mut session).await;
        assert!(matches!(event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "/exit"));
    }
}
