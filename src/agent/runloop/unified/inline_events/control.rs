use anyhow::Result;

use vtcode_core::utils::ansi::MessageStyle;

use super::action::InlineLoopAction;
use super::state::InlineEventState;
use crate::hooks::lifecycle::SessionEndReason;

pub(crate) struct InlineControlProcessor<'a, 'state> {
    state: &'a mut InlineEventState<'state>,
}

impl<'a, 'state> InlineControlProcessor<'a, 'state> {
    pub(crate) fn new(state: &'a mut InlineEventState<'state>) -> Self {
        Self { state }
    }

    pub(crate) fn cancel(self) -> Result<InlineLoopAction> {
        self.state.reset_interrupt_state();
        self.state.renderer().line(
            MessageStyle::Info,
            "Cancellation request noted. No active run to stop.",
        )?;
        Ok(InlineLoopAction::Continue)
    }

    pub(crate) fn exit(self) -> Result<InlineLoopAction> {
        self.state.renderer().line(MessageStyle::Info, "Goodbye!")?;
        Ok(InlineLoopAction::Exit(SessionEndReason::Exit))
    }
}
