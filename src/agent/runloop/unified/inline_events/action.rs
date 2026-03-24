use vtcode_core::hooks::SessionEndReason;

pub(crate) enum InlineLoopAction {
    Continue,
    Submit(String),
    RequestInlinePromptSuggestion(String),
    Exit(SessionEndReason),
    ResumeSession(String), // Session identifier to resume
    ForkSession {
        session_id: String,
        summarize: bool,
    },
    /// Plan approved (Claude Code style HITL) - transition from Plan to Edit mode
    PlanApproved {
        /// If true, auto-accept file edits without prompting
        auto_accept: bool,
    },
    /// User wants to return to plan mode to edit the plan
    PlanEditRequested,
    /// Diff preview approved - apply the edit changes
    DiffApproved,
    /// Diff preview rejected - cancel the edit changes
    DiffRejected,
}
