use crate::hooks::lifecycle::SessionEndReason;

pub(crate) enum InlineLoopAction {
    Continue,
    Submit(String),
    Exit(SessionEndReason),
    ResumeSession(String), // Session identifier to resume
    /// Plan approved (Claude Code style HITL) - transition from Plan to Edit mode
    PlanApproved {
        /// If true, auto-accept file edits without prompting
        auto_accept: bool,
    },
    /// User wants to return to plan mode to edit the plan
    PlanEditRequested,
}
