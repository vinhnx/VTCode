use crate::hooks::lifecycle::SessionEndReason;

pub(crate) enum InlineLoopAction {
    Continue,
    Submit(String),
    Exit(SessionEndReason),
    ResumeSession(String), // Session identifier to resume
    ToggleDelegateMode,
    SwitchTeammate(TeamSwitchDirection),
    /// Plan approved (Claude Code style HITL) - transition from Plan to Edit mode
    PlanApproved {
        /// If true, auto-accept file edits without prompting
        auto_accept: bool,
        /// If true, clear conversation context before continuing
        clear_context: bool,
    },
    /// User wants to return to plan mode to edit the plan
    PlanEditRequested,
    /// Diff preview approved - apply the edit changes
    DiffApproved,
    /// Diff preview rejected - cancel the edit changes
    DiffRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TeamSwitchDirection {
    Next,
    Previous,
}
