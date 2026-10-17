mod diff;
mod overlay;
mod plan;
mod protocol;
mod slash;

pub use diff::{DiffHunk, DiffPreviewMode, DiffPreviewState, TrustMode};
pub use overlay::{
    AgentPaletteItem, AgentPaletteTransientRequest, DiffOverlayRequest, FilePaletteTransientRequest,
    ListOverlayRequest, LocalAgentsTransientRequest, ModalOverlayRequest, TaskPanelMetadata, TaskPanelTransientRequest,
    TransientEvent, TransientHotkey, TransientHotkeyAction, TransientHotkeyKey, TransientRequest,
    TransientSelectionChange, TransientSubmission, WizardOverlayRequest,
};
pub use plan::{PlanContent, PlanPhase, PlanStep};
pub(crate) use protocol::TransientActivitySignal;
pub use protocol::{
    ArchivedPromptEntry, InlineCommand, InlineEvent, InlineEventCallback, InlineHandle, InlineSession, ProgressGuard,
    SubmittedInput,
};
pub use slash::SlashCommandItem;

pub use crate::tui::core_tui::types::{
    ContentPart, ExecSessionAction, FocusChangeCallback, InlineHeaderBadge, InlineHeaderContext, InlineHeaderHighlight,
    InlineHeaderStatusBadge, InlineHeaderStatusTone, InlineItemKind, InlineLinkRange, InlineLinkTarget, InlineListItem,
    InlineListSearchConfig, InlineListSelection, InlineMessageKind, InlineSegment, InlineStatus, InlineTextStyle,
    InlineTheme, InlineTone, LocalAgentEntry, LocalAgentKind, OpenAIServiceTierChoice, OverlayEvent,
    OverlaySelectionChange, PreviewCallback, RewindAction, SecurePromptConfig, WizardModalMode, WizardStep,
};
pub use vtcode_commons::ui_protocol::{CompactActivityMetadata, ToolOutputId};
