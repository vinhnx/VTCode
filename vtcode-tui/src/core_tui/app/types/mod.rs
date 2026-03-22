mod diff;
mod overlay;
mod plan;
mod protocol;
mod slash;

pub use diff::{DiffHunk, DiffPreviewMode, DiffPreviewState, TrustMode};
pub use overlay::{
    DiffOverlayRequest, FilePaletteTransientRequest, ListOverlayRequest, ModalOverlayRequest,
    TaskPanelTransientRequest, TransientEvent, TransientHotkey, TransientHotkeyAction,
    TransientHotkeyKey, TransientRequest, TransientSelectionChange, TransientSubmission,
    WizardOverlayRequest,
};
pub use plan::{PlanContent, PlanPhase, PlanStep};
pub use protocol::{InlineCommand, InlineEvent, InlineEventCallback, InlineHandle, InlineSession};
pub use slash::SlashCommandItem;

pub use crate::core_tui::types::{
    ContentPart, EditingMode, FocusChangeCallback, InlineHeaderContext, InlineHeaderHighlight,
    InlineHeaderStatusBadge, InlineHeaderStatusTone, InlineLinkRange, InlineLinkTarget,
    InlineListItem, InlineListSearchConfig, InlineListSelection, InlineMessageKind, InlineSegment,
    InlineTextStyle, InlineTheme, RewindAction, SecurePromptConfig, WizardModalMode, WizardStep,
};
