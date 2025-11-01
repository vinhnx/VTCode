use crate::hooks::lifecycle::SessionEndReason;

pub(crate) enum InlineLoopAction {
    Continue,
    Submit(String),
    Exit(SessionEndReason),
}
