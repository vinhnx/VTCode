use vtcode_core::utils::ansi::AnsiRenderer;

use super::interrupts::InlineInterruptCoordinator;

pub(crate) struct InlineEventState<'a> {
    renderer: &'a mut AnsiRenderer,
    interrupts: InlineInterruptCoordinator<'a>,
    ctrl_c_notice_displayed: &'a mut bool,
}

impl<'a> InlineEventState<'a> {
    pub(crate) fn new(
        renderer: &'a mut AnsiRenderer,
        interrupts: InlineInterruptCoordinator<'a>,
        ctrl_c_notice_displayed: &'a mut bool,
    ) -> Self {
        Self {
            renderer,
            interrupts,
            ctrl_c_notice_displayed,
        }
    }

    pub(crate) fn renderer(&mut self) -> &mut AnsiRenderer {
        self.renderer
    }

    pub(crate) fn interrupts(&self) -> InlineInterruptCoordinator<'a> {
        self.interrupts
    }

    pub(crate) fn reset_interrupt_state(&mut self) {
        self.interrupts
            .reset_after_user_action(self.ctrl_c_notice_displayed);
    }
}
