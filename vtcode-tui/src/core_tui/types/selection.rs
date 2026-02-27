use crate::config::types::ReasoningEffortLevel;

use super::diff::TrustMode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineListSelection {
    Model(usize),
    DynamicModel(usize),
    RefreshDynamicModels,
    Reasoning(ReasoningEffortLevel),
    DisableReasoning,
    CustomModel,
    Theme(String),
    Session(String),
    SlashCommand(String),
    ToolApproval(bool),
    ToolApprovalDenyOnce,
    ToolApprovalSession,
    ToolApprovalPermanent,
    SessionLimitIncrease(usize),

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

    /// Plan confirmation dialog result (human-in-the-loop flow)
    PlanApprovalExecute,
    /// Clear conversation context and auto-accept edits
    PlanApprovalClearContextAutoAccept,
    /// Return to planning to edit the plan
    PlanApprovalEditPlan,
    /// Cancel execution and stay in plan mode
    PlanApprovalCancel,
    /// Auto-accept all future plans in this session
    PlanApprovalAutoAccept,
    /// Diff preview approval - apply edit changes
    DiffPreviewApply,
    /// Diff preview rejection - cancel edit changes
    DiffPreviewReject,
    /// Diff preview trust mode changed
    DiffPreviewTrustChanged {
        mode: TrustMode,
    },
}

#[derive(Clone, Debug)]
pub struct InlineListItem {
    pub title: String,
    pub subtitle: Option<String>,
    pub badge: Option<String>,
    pub indent: u8,
    pub selection: Option<InlineListSelection>,
    pub search_value: Option<String>,
}

#[derive(Clone)]
pub struct InlineListSearchConfig {
    pub label: String,
    pub placeholder: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SecurePromptConfig {
    pub label: String,

    /// Optional placeholder shown when input is empty.
    pub placeholder: Option<String>,

    /// Whether the input should be masked (e.g., API keys).
    pub mask_input: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WizardModalMode {
    /// Traditional multi-step wizard behavior (Enter advances/collects answers).
    MultiStep,
    /// Tabbed list behavior (tabs switch categories; Enter submits immediately).
    TabbedList,
}

/// A single step in a wizard modal flow
#[derive(Clone, Debug)]
pub struct WizardStep {
    /// Title displayed in the tab header
    pub title: String,
    /// Question or instruction shown above the list
    pub question: String,
    /// Selectable items for this step
    pub items: Vec<InlineListItem>,
    /// Whether this step has been completed
    pub completed: bool,
    /// The selected answer for this step (if completed)
    pub answer: Option<InlineListSelection>,

    pub allow_freeform: bool,
    pub freeform_label: Option<String>,
    pub freeform_placeholder: Option<String>,
}
