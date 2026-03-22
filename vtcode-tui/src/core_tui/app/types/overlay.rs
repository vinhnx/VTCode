use std::path::PathBuf;

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
    pub hotkeys: Vec<TransientHotkey>,
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
pub struct FilePaletteTransientRequest {
    pub files: Vec<String>,
    pub workspace: PathBuf,
    pub visible: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct TaskPanelTransientRequest {
    pub lines: Vec<String>,
    pub visible: Option<bool>,
}

#[derive(Clone, Debug)]
pub enum TransientRequest {
    Modal(ModalOverlayRequest),
    List(ListOverlayRequest),
    Wizard(WizardOverlayRequest),
    Diff(DiffOverlayRequest),
    FilePalette(FilePaletteTransientRequest),
    HistoryPicker,
    SlashPalette,
    TaskPanel(TaskPanelTransientRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransientHotkey {
    pub key: TransientHotkeyKey,
    pub action: TransientHotkeyAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransientHotkeyKey {
    CtrlChar(char),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransientHotkeyAction {
    LaunchEditor,
    FocusJobOutput,
    InterruptJob,
    PreviewJobSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransientEvent {
    SelectionChanged(TransientSelectionChange),
    Submitted(TransientSubmission),
    Cancelled,
}

impl From<crate::core_tui::types::OverlayEvent> for TransientEvent {
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
pub enum TransientSelectionChange {
    List(InlineListSelection),
    DiffTrustMode { mode: TrustMode },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransientSubmission {
    Selection(InlineListSelection),
    Wizard(Vec<InlineListSelection>),
    DiffApply,
    DiffReject,
    DiffProceed,
    DiffReload,
    DiffAbort,
    Hotkey(TransientHotkeyAction),
}

impl From<TransientHotkeyKey> for crate::core_tui::types::OverlayHotkeyKey {
    fn from(value: TransientHotkeyKey) -> Self {
        match value {
            TransientHotkeyKey::CtrlChar(ch) => Self::CtrlChar(ch),
        }
    }
}

impl From<TransientHotkeyAction> for crate::core_tui::types::OverlayHotkeyAction {
    fn from(value: TransientHotkeyAction) -> Self {
        match value {
            TransientHotkeyAction::LaunchEditor => Self::LaunchEditor,
            TransientHotkeyAction::FocusJobOutput => Self::FocusJobOutput,
            TransientHotkeyAction::InterruptJob => Self::InterruptJob,
            TransientHotkeyAction::PreviewJobSnapshot => Self::PreviewJobSnapshot,
        }
    }
}

impl From<crate::core_tui::types::OverlayHotkeyAction> for TransientHotkeyAction {
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

impl From<crate::core_tui::types::OverlaySelectionChange> for TransientSelectionChange {
    fn from(value: crate::core_tui::types::OverlaySelectionChange) -> Self {
        match value {
            crate::core_tui::types::OverlaySelectionChange::List(selection) => {
                Self::List(selection)
            }
        }
    }
}

impl From<crate::core_tui::types::OverlaySubmission> for TransientSubmission {
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

impl From<TransientHotkey> for crate::core_tui::types::OverlayHotkey {
    fn from(value: TransientHotkey) -> Self {
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
