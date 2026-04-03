//! List selection and wizard step types.

/// Rewind action choices for the rewind overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RewindAction {
    RestoreBoth,
    RestoreConversation,
    RestoreCode,
    SummarizeFromHere,
    NeverMind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenAIServiceTierChoice {
    ProjectDefault,
    Flex,
    Priority,
}

/// Selection value returned from a list or wizard overlay.
///
/// The `Reasoning` variant carries a `String` reasoning-effort level rather
/// than a typed enum so that this type stays free of config-crate dependencies.
/// Callers convert to/from their local `ReasoningEffortLevel` as needed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineListSelection {
    Model(usize),
    DynamicModel(usize),
    CustomProvider(usize),
    RefreshDynamicModels,
    Reasoning(String),
    DisableReasoning,
    OpenAIServiceTier(OpenAIServiceTierChoice),
    CustomModel,
    Theme(String),
    Session(String),
    SessionForkMode {
        session_id: String,
        summarize: bool,
    },
    ConfigAction(String),
    SlashCommand(String),
    ToolApproval(bool),
    ToolApprovalDenyOnce,
    ToolApprovalSession,
    ToolApprovalPermanent,
    FileConflictReload,
    FileConflictViewDiff,
    FileConflictAbort,
    SessionLimitIncrease(usize),
    RewindCheckpoint(usize),
    RewindAction(RewindAction),

    /// Selection shape used by legacy tabbed HITL flows.
    AskUserChoice {
        tab_id: String,
        choice_id: String,
        text: Option<String>,
    },

    /// Selection returned from the `request_user_input` HITL tool.
    RequestUserInputAnswer {
        question_id: String,
        selected: Vec<String>,
        other: Option<String>,
    },

    /// Plan confirmation dialog result (human-in-the-loop flow).
    PlanApprovalExecute,
    /// Return to planning to edit the plan.
    PlanApprovalEditPlan,
    /// Auto-accept all future plans in this session.
    PlanApprovalAutoAccept,
}

/// A selectable item inside a list overlay.
#[derive(Clone, Debug)]
pub struct InlineListItem {
    pub title: String,
    pub subtitle: Option<String>,
    pub badge: Option<String>,
    pub indent: u8,
    pub selection: Option<InlineListSelection>,
    pub search_value: Option<String>,
}

/// A single step in a wizard modal flow.
#[derive(Clone, Debug)]
pub struct WizardStep {
    /// Title displayed in the tab header.
    pub title: String,
    /// Question or instruction shown above the list.
    pub question: String,
    /// Selectable items for this step.
    pub items: Vec<InlineListItem>,
    /// Whether this step has been completed.
    pub completed: bool,
    /// The selected answer for this step (if completed).
    pub answer: Option<InlineListSelection>,

    pub allow_freeform: bool,
    pub freeform_label: Option<String>,
    pub freeform_placeholder: Option<String>,
    pub freeform_default: Option<String>,
}
