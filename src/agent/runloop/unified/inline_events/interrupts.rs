use anyhow::Result;

use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::InlineHandle;

use super::action::InlineLoopAction;
use super::queue::InlineQueueState;
use crate::agent::runloop::unified::state::CtrlCState;
use vtcode_core::hooks::SessionEndReason;

#[derive(Clone, Copy)]
pub(crate) struct InlineInterruptCoordinator<'a> {
    state: &'a CtrlCState,
}

impl<'a> InlineInterruptCoordinator<'a> {
    pub(crate) fn new(state: &'a CtrlCState) -> Self {
        Self { state }
    }

    pub(crate) fn reset_after_user_action(self, notice_displayed: &mut bool) {
        if !self.state.is_cancel_requested() && !self.state.is_cancel_handled() {
            self.state.reset();
            *notice_displayed = false;
        }
    }

    pub(crate) fn reset_after_submission(self, notice_displayed: &mut bool) {
        self.state.reset();
        *notice_displayed = false;
    }

    pub(crate) fn ensure_notice_displayed(
        self,
        notice_displayed: &mut bool,
        renderer: &mut AnsiRenderer,
        handle: &InlineHandle,
        _default_placeholder: &Option<String>,
        queue: &mut InlineQueueState<'_>,
    ) -> Result<bool> {
        if self.state.is_cancel_requested() {
            if !*notice_displayed {
                self.display_notice(renderer, handle, queue)?;
                *notice_displayed = true;
                return Ok(true);
            }
        } else {
            *notice_displayed = false;
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
        queue: &mut InlineQueueState<'_>,
    ) -> Result<()> {
        renderer.line_if_not_empty(MessageStyle::Output)?;
        let preserved = queue.preserve_on_interrupt();
        if preserved > 0 {
            renderer.line(
                MessageStyle::Info,
                &format!("Interrupt received. Stopping task... Preserved {preserved} queued message(s)."),
            )?;
        } else {
            renderer.line(MessageStyle::Info, "Interrupt received. Stopping task...")?;
        }
        handle.set_placeholder(Some(vtcode_config::constants::ui::CHAT_INPUT_PLACEHOLDER_INTERRUPTED.to_owned()));
        Ok(())
    }
}
