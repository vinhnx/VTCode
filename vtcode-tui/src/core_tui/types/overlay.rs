use super::diff::{DiffHunk, DiffPreviewMode, TrustMode};
use super::selection::{
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayEvent {
    SelectionChanged(OverlaySelectionChange),
    Submitted(OverlaySubmission),
    Cancelled,
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
