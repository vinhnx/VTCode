//! List selection and wizard step types.

use super::style::InlineTone;

/// Rewind action choices for the rewind overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    Ultrafast,
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
    ToolApprovalEnable,
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
    /// Execute the approved plan after clearing transient context.
    PlanApprovalFreshContext,
    /// Return to planning to edit the plan file.
    PlanApprovalEditPlan,
    /// Return to planning to discuss and revise the plan in chat.
    PlanApprovalDiscuss,
    /// Auto-accept all future plans in this session.
    PlanApprovalAutoAccept,
    /// Hand off to the build primary agent and execute the plan.
    PlanApprovalSwitchBuild,
    /// Hand off to the auto primary agent (auto-execute with per-step HITL).
    PlanApprovalSwitchAuto,
}

/// Role of a list row, used by the renderer to pick emphasis and spacing.
///
/// Defaults to [`InlineItemKind::Item`] so existing callers keep today's look
/// until they opt into a more specific kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InlineItemKind {
    /// Ordinary selectable (or non-selectable) row.
    #[default]
    Item,
    /// Non-selectable section header (bold title, blank gap above).
    Header,
    /// Config key row: `title` is the label, `value` is the live setting value.
    Setting,
    /// Imperative row (Reset, Reload, Back, Pick model, …).
    Action,
    /// Non-selectable note (dimmed).
    Hint,
}

/// A selectable item inside a list overlay.
#[derive(Clone, Debug, Default)]
pub struct InlineListItem {
    pub title: String,
    /// Secondary description / metadata. Rendered dimmed; do **not** pack the
    /// live value here — use [`Self::value`] so the renderer can accent it.
    pub subtitle: Option<String>,
    /// Short trailing/leading label (provider, "On", "Edit", …).
    pub badge: Option<String>,
    pub indent: u8,
    pub selection: Option<InlineListSelection>,
    pub search_value: Option<String>,
    /// Live value for setting rows (accent-styled by the renderer).
    pub value: Option<String>,
    /// Semantic tone for `badge` (and for `value` emphasis on setting rows).
    pub badge_tone: InlineTone,
    /// Row role controlling title emphasis and spacing.
    pub kind: InlineItemKind,
}

impl InlineListItem {
    /// Minimal selectable row: `title` + `selection`, everything else default.
    #[must_use]
    pub fn new(title: impl Into<String>, selection: Option<InlineListSelection>) -> Self {
        Self { title: title.into(), selection, ..Self::default() }
    }

    /// Shared group header: bold title, blank spacing above and below.
    /// Renders as a labeled heading (`» Title`) so sections
    /// stay distinct from plain items and full-width dividers.
    /// Use for every grouped modal list so sections read the same way.
    #[must_use]
    pub fn group_header(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            kind: InlineItemKind::Header,
            ..Self::default()
        }
    }

    /// Full-width rule between option groups (approve vs deny, lists vs actions).
    #[must_use]
    pub fn group_divider() -> Self {
        // Empty title is the canonical untitled divider (`is_divider_title`).
        Self::default()
    }

    #[must_use]
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    #[must_use]
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    #[must_use]
    pub fn with_badge(mut self, label: impl Into<String>, tone: InlineTone) -> Self {
        self.badge = Some(label.into());
        self.badge_tone = tone;
        self
    }

    #[must_use]
    pub fn with_kind(mut self, kind: InlineItemKind) -> Self {
        self.kind = kind;
        self
    }

    #[must_use]
    pub fn with_indent(mut self, indent: u8) -> Self {
        self.indent = indent;
        self
    }

    #[must_use]
    pub fn with_search_value(mut self, search_value: impl Into<String>) -> Self {
        self.search_value = Some(search_value.into());
        self
    }

    /// Legacy section-header heuristic: non-selectable rows that are not
    /// dividers act as group headers (bold title + blank gap). Explicit
    /// [`InlineItemKind::Hint`] rows are notes, not headers.
    #[must_use]
    pub fn is_header(&self) -> bool {
        self.selection.is_none() && self.kind != InlineItemKind::Hint
    }
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
