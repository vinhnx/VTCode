use super::diff::{DiffHunk, DiffPreviewMode, TrustMode};
use crate::core_tui::types::{
    InlineListItem, InlineListSearchConfig, InlineListSelection, SecurePromptConfig,
    WizardModalMode, WizardStep,
};

#[derive(Clone, Debug)]
pub struct ModalOverlayRequest {
    pub title: String,
    pub lines: Vec<String>,
    pub secure_prompt: Option<SecurePromptConfig>,
}

#[derive(Clone, Debug)]
pub struct ListOverlayRequest {
    pub title: String,
    pub lines: Vec<String>,
    pub footer_hint: Option<String>,
    pub items: Vec<InlineListItem>,
    pub selected: Option<InlineListSelection>,
    pub search: Option<InlineListSearchConfig>,
    pub hotkeys: Vec<OverlayHotkey>,
}

#[derive(Clone, Debug)]
pub struct WizardOverlayRequest {
    pub title: String,
    pub steps: Vec<WizardStep>,
    pub current_step: usize,
    pub search: Option<InlineListSearchConfig>,
    pub mode: WizardModalMode,
}

#[derive(Clone, Debug)]
pub struct DiffOverlayRequest {
    pub file_path: String,
    pub before: String,
    pub after: String,
    pub hunks: Vec<DiffHunk>,
    pub current_hunk: usize,
    pub mode: DiffPreviewMode,
}

#[derive(Clone, Debug)]
pub enum OverlayRequest {
    Modal(ModalOverlayRequest),
    List(ListOverlayRequest),
    Wizard(WizardOverlayRequest),
    Diff(DiffOverlayRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayHotkey {
    pub key: OverlayHotkeyKey,
    pub action: OverlayHotkeyAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayHotkeyKey {
    CtrlChar(char),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayHotkeyAction {
    LaunchEditor,
    FocusJobOutput,
    InterruptJob,
    PreviewJobSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayEvent {
    SelectionChanged(OverlaySelectionChange),
    Submitted(OverlaySubmission),
    Cancelled,
}

impl From<crate::core_tui::types::OverlayEvent> for OverlayEvent {
    fn from(value: crate::core_tui::types::OverlayEvent) -> Self {
        match value {
            crate::core_tui::types::OverlayEvent::SelectionChanged(change) => {
                Self::SelectionChanged(change.into())
            }
            crate::core_tui::types::OverlayEvent::Submitted(submission) => {
                Self::Submitted(submission.into())
            }
            crate::core_tui::types::OverlayEvent::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlaySelectionChange {
    List(InlineListSelection),
    DiffTrustMode { mode: TrustMode },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlaySubmission {
    Selection(InlineListSelection),
    Wizard(Vec<InlineListSelection>),
    DiffApply,
    DiffReject,
    DiffProceed,
    DiffReload,
    DiffAbort,
    Hotkey(OverlayHotkeyAction),
}

impl From<OverlayHotkeyKey> for crate::core_tui::types::OverlayHotkeyKey {
    fn from(value: OverlayHotkeyKey) -> Self {
        match value {
            OverlayHotkeyKey::CtrlChar(ch) => Self::CtrlChar(ch),
        }
    }
}

impl From<OverlayHotkeyAction> for crate::core_tui::types::OverlayHotkeyAction {
    fn from(value: OverlayHotkeyAction) -> Self {
        match value {
            OverlayHotkeyAction::LaunchEditor => Self::LaunchEditor,
            OverlayHotkeyAction::FocusJobOutput => Self::FocusJobOutput,
            OverlayHotkeyAction::InterruptJob => Self::InterruptJob,
            OverlayHotkeyAction::PreviewJobSnapshot => Self::PreviewJobSnapshot,
        }
    }
}

impl From<crate::core_tui::types::OverlayHotkeyAction> for OverlayHotkeyAction {
    fn from(value: crate::core_tui::types::OverlayHotkeyAction) -> Self {
        match value {
            crate::core_tui::types::OverlayHotkeyAction::LaunchEditor => Self::LaunchEditor,
            crate::core_tui::types::OverlayHotkeyAction::FocusJobOutput => Self::FocusJobOutput,
            crate::core_tui::types::OverlayHotkeyAction::InterruptJob => Self::InterruptJob,
            crate::core_tui::types::OverlayHotkeyAction::PreviewJobSnapshot => {
                Self::PreviewJobSnapshot
            }
        }
    }
}

impl From<crate::core_tui::types::OverlaySelectionChange> for OverlaySelectionChange {
    fn from(value: crate::core_tui::types::OverlaySelectionChange) -> Self {
        match value {
            crate::core_tui::types::OverlaySelectionChange::List(selection) => {
                Self::List(selection)
            }
        }
    }
}

impl From<crate::core_tui::types::OverlaySubmission> for OverlaySubmission {
    fn from(value: crate::core_tui::types::OverlaySubmission) -> Self {
        match value {
            crate::core_tui::types::OverlaySubmission::Selection(selection) => {
                Self::Selection(selection)
            }
            crate::core_tui::types::OverlaySubmission::Wizard(selections) => {
                Self::Wizard(selections)
            }
            crate::core_tui::types::OverlaySubmission::Hotkey(action) => {
                Self::Hotkey(action.into())
            }
        }
    }
}

impl From<OverlayHotkey> for crate::core_tui::types::OverlayHotkey {
    fn from(value: OverlayHotkey) -> Self {
        Self {
            key: value.key.into(),
            action: value.action.into(),
        }
    }
}

impl From<ModalOverlayRequest> for crate::core_tui::types::ModalOverlayRequest {
    fn from(value: ModalOverlayRequest) -> Self {
        Self {
            title: value.title,
            lines: value.lines,
            secure_prompt: value.secure_prompt,
        }
    }
}

impl From<ListOverlayRequest> for crate::core_tui::types::ListOverlayRequest {
    fn from(value: ListOverlayRequest) -> Self {
        Self {
            title: value.title,
            lines: value.lines,
            footer_hint: value.footer_hint,
            items: value.items,
            selected: value.selected,
            search: value.search,
            hotkeys: value.hotkeys.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<WizardOverlayRequest> for crate::core_tui::types::WizardOverlayRequest {
    fn from(value: WizardOverlayRequest) -> Self {
        Self {
            title: value.title,
            steps: value.steps,
            current_step: value.current_step,
            search: value.search,
            mode: value.mode,
        }
    }
}
