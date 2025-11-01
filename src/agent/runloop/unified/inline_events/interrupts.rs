use anyhow::Result;

use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::action::InlineLoopAction;
use super::queue::InlineQueueState;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::hooks::lifecycle::SessionEndReason;

#[derive(Clone, Copy)]
pub(crate) struct InlineInterruptCoordinator<'a> {
    state: &'a CtrlCState,
}

impl<'a> InlineInterruptCoordinator<'a> {
    pub(crate) fn new(state: &'a CtrlCState) -> Self {
        Self { state }
    }

    pub(crate) fn reset_after_user_action(self, notice_displayed: &mut bool) {
        self.state.disarm_exit();
        self.state.clear_cancel();
        *notice_displayed = false;
    }

    pub(crate) fn ensure_notice_displayed(
        self,
        notice_displayed: &mut bool,
        renderer: &mut AnsiRenderer,
        handle: &InlineHandle,
        default_placeholder: &Option<String>,
        queue: &mut InlineQueueState<'_>,
    ) -> Result<bool> {
        if self.state.is_cancel_requested() {
            if !*notice_displayed {
                self.display_notice(renderer, handle, default_placeholder, queue)?;
                *notice_displayed = true;
            }
            return Ok(true);
        }

        Ok(false)
    }

    pub(crate) fn action_for_interrupt(self) -> InlineLoopAction {
        if self.state.is_exit_requested() {
            InlineLoopAction::Exit(SessionEndReason::Exit)
        } else {
            InlineLoopAction::Continue
        }
    }

    fn display_notice(
        self,
        renderer: &mut AnsiRenderer,
        handle: &InlineHandle,
        default_placeholder: &Option<String>,
        queue: &mut InlineQueueState<'_>,
    ) -> Result<()> {
        renderer.line_if_not_empty(MessageStyle::Output)?;
        renderer.line(
            MessageStyle::Info,
            "Interrupt received. Agent loop stopped. Press Ctrl+C again to exit.",
        )?;
        handle.clear_input();
        handle.set_placeholder(default_placeholder.clone());
        queue.clear();
        Ok(())
    }
}
