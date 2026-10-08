use crate::tui::core_tui::types::{
    define_inline_message_commands, impl_inline_control_methods, impl_inline_message_methods,
};
use std::ops::Deref;
use std::sync::Arc;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::ContentPart;
use super::overlay::{ListOverlayRequest, ModalOverlayRequest, OverlayEvent, OverlayRequest};
use super::selection::{InlineListItem, InlineListSearchConfig, InlineListSelection, SecurePromptConfig};
use super::style::{InlineHeaderContext, InlineTextStyle, InlineTheme};
use crate::tui::core_tui::session::config::AppearanceConfig;
use crate::tui::options::FullscreenInteractionSettings;

pub use vtcode_commons::ui_protocol::ActivityState;
pub use vtcode_commons::ui_protocol::InlineMessageKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedInput {
    pub text: String,
    pub attachments: Vec<ContentPart>,
    /// Whether this submission may be merged with other queued text-only
    /// submissions of the same agent into a single model turn. Ctrl+Enter sets
    /// this when it queues while busy; steered inputs bypass the queue
    /// entirely and plain-Enter slash commands stay a one-per-turn dispatch.
    pub batchable: bool,
}

impl SubmittedInput {
    pub fn new(text: impl Into<String>, attachments: Vec<ContentPart>) -> Self {
        Self { text: text.into(), attachments, batchable: false }
    }

    fn text_only(text: impl Into<String>) -> Self {
        Self::new(text, Vec::new())
    }

    /// Mark this submission as eligible for queue batching (Ctrl+Enter).
    pub fn batchable(mut self) -> Self {
        self.batchable = true;
        self
    }

    pub fn trim_text(self) -> Self {
        Self {
            text: self.text.trim().to_string(),
            attachments: self.attachments,
            batchable: self.batchable,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.attachments.is_empty()
    }

    pub fn has_attachments(&self) -> bool {
        !self.attachments.is_empty()
    }
}

impl From<String> for SubmittedInput {
    fn from(text: String) -> Self {
        Self::text_only(text)
    }
}

impl From<&str> for SubmittedInput {
    fn from(text: &str) -> Self {
        Self::text_only(text)
    }
}

impl Deref for SubmittedInput {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl PartialEq<&str> for SubmittedInput {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

impl PartialEq<&str> for &SubmittedInput {
    fn eq(&self, other: &&str) -> bool {
        self.text == *other
    }
}

impl PartialEq<String> for SubmittedInput {
    fn eq(&self, other: &String) -> bool {
        self.text == *other
    }
}

impl PartialEq<&String> for SubmittedInput {
    fn eq(&self, other: &&String) -> bool {
        self.text == **other
    }
}

define_inline_message_commands! {
    pub enum InlineCommand {
        SetPrompt {
            prefix: String,
            style: InlineTextStyle,
        },
        SetPlaceholder {
            hint: Option<String>,
            style: Option<InlineTextStyle>,
        },
        SetMessageLabels {
            agent: Option<String>,
            user: Option<String>,
        },
        SetHeaderContext {
            context: Box<InlineHeaderContext>,
        },
        SetInputStatus {
            left: Option<String>,
            right: Option<String>,
        },
        SetConfiguredInputStatus {
            left: Option<String>,
            right: Option<String>,
        },
        ProgramStatus(vtcode_commons::program_status::ProgramStatusUpdate),
        SetActivityState(ActivityState),
        UpdateProgress(vtcode_commons::ui_protocol::ProgressUpdate),
        SetTerminalTitleItems {
            items: Option<Vec<String>>,
        },
        SetTerminalTitleThreadLabel {
            label: Option<String>,
        },
        SetTerminalTitleGitBranch {
            branch: Option<String>,
        },
        SetTheme {
            theme: InlineTheme,
        },
        SetColorSchemeAuto {
            enabled: bool,
        },
        SetAppearance {
            appearance: AppearanceConfig,
        },
        SetFullscreenInteraction {
            interaction: FullscreenInteractionSettings,
        },
        SetVimModeEnabled(bool),
        SetQueuedInputs {
            entries: Vec<String>,
        },
        SetSubprocessEntries {
            entries: Vec<String>,
        },
        SetSubagentPreview {
            text: Option<String>,
        },
        SetPrimaryAgent {
            name: Option<String>,
            color: Option<String>,
        },
        SetCursorVisible(bool),
        SetInputEnabled(bool),
        SetImageInputEnabled(bool),
        SetInput(String),
        RestoreInputDraft(SubmittedInput),
        ApplySuggestedPrompt(String),
        SetInlinePromptSuggestion {
            suggestion: String,
            llm_generated: bool,
        },
        ClearInlinePromptSuggestion,
        ClearInput,
        ForceRedraw,
        ShowOverlay {
            request: Box<OverlayRequest>,
        },
        CloseOverlay,
        // App-only palette/history commands are defined in the app protocol layer.
        ClearScreen,
        SuspendEventLoop,
        ResumeEventLoop,
        ClearInputQueue,
        StopEventStream,
        StartEventStream,
        SetSkipConfirmations(bool),
        Shutdown,
        /// Update reasoning stage in header context
        SetReasoningStage(Option<String>),
    }
}

#[derive(Debug, Clone)]
pub enum InlineEvent {
    Submit(SubmittedInput),
    QueueSubmit(SubmittedInput),
    Steer(SubmittedInput),
    ProcessLatestQueued,
    /// Edit the newest queued input (pop into input buffer)
    EditQueue,
    Overlay(OverlayEvent),
    Cancel,
    Exit,
    Interrupt,
    Pause,
    Resume,
    BackgroundOperation,
    ExecSessionAction {
        id: String,
        action: super::local_agents::ExecSessionAction,
    },
    ScrollLineUp,
    ScrollLineDown,
    ScrollPageUp,
    ScrollPageDown,
    JumpToLastChange,
    OpenFileInEditor(String),
    OpenUrl(String),
    LaunchEditor {
        draft: String,
    },
    ForceCancelPtySession,
    RequestInlinePromptSuggestion(String),
    CyclePrimaryAgent,
    CyclePrimaryAgentPrevious,
    SelectPrimaryAgent {
        name: Option<String>,
    },
    HistoryPrevious,
    HistoryNext,
    ToggleToolDisplayMode,
}

pub type InlineEventCallback = Arc<dyn Fn(&InlineEvent) + Send + Sync + 'static>;
pub type FocusChangeCallback = Arc<dyn Fn(bool) + Send + Sync + 'static>;
pub type PreviewCallback = Arc<dyn Fn(Option<&InlineListSelection>) -> anyhow::Result<()> + Send + Sync + 'static>;

#[derive(Clone)]
pub struct InlineHandle {
    pub(crate) sender: UnboundedSender<InlineCommand>,
}

impl InlineHandle {
    pub fn new_for_tests(sender: UnboundedSender<InlineCommand>) -> Self {
        Self { sender }
    }

    fn send_command(&self, command: InlineCommand) {
        if self.sender.is_closed() {
            return;
        }
        let _ = self.sender.send(command);
    }

    impl_inline_message_methods!(InlineCommand);
    impl_inline_control_methods!(InlineCommand);

    pub fn set_placeholder_with_style(&self, hint: Option<String>, style: Option<InlineTextStyle>) {
        self.send_command(InlineCommand::SetPlaceholder { hint, style });
    }

    pub fn set_message_labels(&self, agent: Option<String>, user: Option<String>) {
        self.send_command(InlineCommand::SetMessageLabels { agent, user });
    }

    pub fn show_overlay(&self, request: OverlayRequest) {
        self.send_command(InlineCommand::ShowOverlay { request: Box::new(request) });
    }

    pub fn show_modal(&self, title: String, lines: Vec<String>, secure_prompt: Option<SecurePromptConfig>) {
        self.show_overlay(OverlayRequest::Modal(ModalOverlayRequest {
            title,
            lines,
            secure_prompt,
            is_help_modal: false,
        }));
    }

    pub fn show_list_modal(
        &self,
        title: String,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
    ) {
        self.show_list_modal_with_footer(title, lines, items, selected, search, None);
    }

    pub fn show_list_modal_with_footer(
        &self,
        title: String,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
        footer_hint: Option<String>,
    ) {
        self.show_list_modal_with_status(title, lines, items, selected, search, footer_hint, None);
    }

    /// Show a list modal with an optional status strip (last action feedback).
    pub fn show_list_modal_with_status(
        &self,
        title: String,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
        footer_hint: Option<String>,
        status: Option<crate::tui::core_tui::types::InlineStatus>,
    ) {
        self.show_overlay(OverlayRequest::List(ListOverlayRequest {
            title,
            lines,
            items,
            selected,
            search,
            footer_hint,
            hotkeys: Vec::new(),
            status,
        }));
    }

    pub fn close_overlay(&self) {
        self.send_command(InlineCommand::CloseOverlay);
    }

    pub fn close_modal(&self) {
        self.close_overlay();
    }
}

pub struct InlineSession {
    pub handle: InlineHandle,
    pub events: UnboundedReceiver<InlineEvent>,
}

impl InlineSession {
    pub async fn next_event(&mut self) -> Option<InlineEvent> {
        self.events.recv().await
    }

    pub fn set_skip_confirmations(&mut self, skip: bool) {
        self.handle.set_skip_confirmations(skip);
    }

    pub fn set_color_scheme_auto(&mut self, enabled: bool) {
        self.handle.set_color_scheme_auto(enabled);
    }

    pub fn clone_inline_handle(&self) -> InlineHandle {
        InlineHandle { sender: self.handle.sender.clone() }
    }
}
