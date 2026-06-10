use vtcode_core::hooks::SessionEndReason;

pub(crate) enum InlineLoopAction {
    Continue,
    Submit(String),
    SubmitQueued(super::queue::QueuedInput),
    CyclePrimaryAgent,
    CyclePrimaryAgentPrevious,
    SelectPrimaryAgent {
        name: Option<String>,
    },
    RequestInlinePromptSuggestion(String),
    OpenTranscriptReviewInEditor(String),
    OpenTranscriptReviewScrollback(String),
    Exit(SessionEndReason),
    ResumeSession(String), // Session identifier to resume
    ForkSession {
        session_id: String,
        summarize: bool,
    },
    /// Plan approved (Claude Code style HITL) - continue with implementation
    PlanApproved {
        /// If true, auto-accept file edits without prompting
        auto_accept: bool,
    },
    /// User wants to return to planning workflow to edit the plan
    PlanEditRequested,
    /// Diff preview approved - apply the edit changes
    DiffApproved,
    /// Diff preview rejected - cancel the edit changes
    DiffRejected,
    /// Launch external editor pre-populated with the given draft text
    LaunchEditorWithDraft {
        draft: String,
    },
}
