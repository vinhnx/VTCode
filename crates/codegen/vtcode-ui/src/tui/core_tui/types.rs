mod content;
mod control_commands;
mod local_agents;
mod message_commands;
mod overlay;
mod program_status;
mod protocol;
mod selection;
mod style;

pub(crate) use control_commands::impl_inline_control_methods;
pub(crate) use message_commands::{define_inline_message_commands, impl_inline_message_methods};

pub use content::ContentPart;
pub use local_agents::{ExecSessionAction, LocalAgentEntry, LocalAgentKind};
pub use overlay::{
    ListOverlayRequest, ModalOverlayRequest, OverlayEvent, OverlayHotkey, OverlayHotkeyAction, OverlayHotkeyKey,
    OverlayRequest, OverlaySelectionChange, OverlaySubmission, WizardOverlayRequest,
};
pub use program_status::ProgramStatusWaitGuard;
pub use protocol::{
    ActivityState, FocusChangeCallback, InlineCommand, InlineEvent, InlineEventCallback, InlineHandle,
    InlineMessageKind, InlineSession, PreviewCallback, SubmittedInput,
};
pub use selection::{
    InlineItemKind, InlineListItem, InlineListSearchConfig, InlineListSelection, OpenAIServiceTierChoice, RewindAction,
    SecurePromptConfig, WizardModalMode, WizardStep,
};
pub use style::{
    InlineHeaderBadge, InlineHeaderContext, InlineHeaderHighlight, InlineHeaderStatusBadge, InlineHeaderStatusTone,
    InlineLinkRange, InlineLinkTarget, InlineSegment, InlineStatus, InlineTextStyle, InlineTheme, InlineTone,
};
