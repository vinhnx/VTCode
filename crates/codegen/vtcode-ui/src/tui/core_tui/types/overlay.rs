use super::selection::{
    InlineListItem, InlineListSearchConfig, InlineListSelection, SecurePromptConfig, WizardModalMode, WizardStep,
};
use super::style::InlineStatus;

#[derive(Clone, Debug)]
pub struct ModalOverlayRequest {
    pub(crate) title: String,
    pub(crate) lines: Vec<String>,
    pub(crate) secure_prompt: Option<SecurePromptConfig>,
    pub(crate) is_help_modal: bool,
}

#[derive(Clone, Debug)]
pub struct ListOverlayRequest {
    pub(crate) title: String,
    pub(crate) lines: Vec<String>,
    pub(crate) footer_hint: Option<String>,
    pub(crate) items: Vec<InlineListItem>,
    pub(crate) selected: Option<InlineListSelection>,
    pub(crate) search: Option<InlineListSearchConfig>,
    pub(crate) hotkeys: Vec<OverlayHotkey>,
    /// Latest apply/save feedback rendered as a toned status strip.
    pub(crate) status: Option<InlineStatus>,
}

#[derive(Clone, Debug)]
pub struct WizardOverlayRequest {
    pub(crate) title: String,
    pub(crate) steps: Vec<WizardStep>,
    pub(crate) current_step: usize,
    pub(crate) search: Option<InlineListSearchConfig>,
    pub(crate) mode: WizardModalMode,
}

#[derive(Clone, Debug)]
pub enum OverlayRequest {
    Modal(ModalOverlayRequest),
    List(ListOverlayRequest),
    Wizard(WizardOverlayRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayHotkey {
    pub(crate) key: OverlayHotkeyKey,
    pub(crate) action: OverlayHotkeyAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayHotkeyKey {
    CtrlChar(char),
    Char(char),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayHotkeyAction {
    LaunchEditor,
    OpenSourceThread,
    ReloadSubagentInspector,
    GracefulStopSubagent,
    ForceCancelSubagent,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlaySelectionChange {
    List(InlineListSelection),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlaySubmission {
    Selection(InlineListSelection),
    Wizard(Vec<InlineListSelection>),
    Hotkey(OverlayHotkeyAction),
}
