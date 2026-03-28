use std::sync::Arc;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::overlay::{
    AgentPaletteItem, AgentPaletteTransientRequest, FilePaletteTransientRequest,
    ListOverlayRequest, LocalAgentsTransientRequest, ModalOverlayRequest,
    TaskPanelTransientRequest, TransientEvent, TransientRequest,
};
use crate::core_tui::session::config::AppearanceConfig;
use crate::core_tui::types::{
    EditingMode, InlineHeaderContext, InlineLinkRange, InlineListItem, InlineListSearchConfig,
    InlineListSelection, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme,
    LocalAgentEntry, SecurePromptConfig,
};

pub enum InlineCommand {
    AppendLine {
        kind: InlineMessageKind,
        segments: Vec<InlineSegment>,
    },
    AppendPastedMessage {
        kind: InlineMessageKind,
        text: String,
        line_count: usize,
    },
    Inline {
        kind: InlineMessageKind,
        segment: InlineSegment,
    },
    ReplaceLast {
        count: usize,
        kind: InlineMessageKind,
        lines: Vec<Vec<InlineSegment>>,
        link_ranges: Option<Vec<Vec<InlineLinkRange>>>,
    },
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
    SetTheme {
        theme: InlineTheme,
    },
    SetAppearance {
        appearance: AppearanceConfig,
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
    SetLocalAgents {
        entries: Vec<LocalAgentEntry>,
    },
    SetCursorVisible(bool),
    SetInputEnabled(bool),
    SetInput(String),
    ApplySuggestedPrompt(String),
    SetInlinePromptSuggestion {
        suggestion: String,
        llm_generated: bool,
    },
    ClearInlinePromptSuggestion,
    ClearInput,
    ForceRedraw,
    ShowTransient {
        request: Box<TransientRequest>,
    },
    CloseTransient,
    ClearScreen,
    SuspendEventLoop,
    ResumeEventLoop,
    ClearInputQueue,
    /// Update editing mode state in header context
    SetEditingMode(EditingMode),
    /// Update autonomous mode state in header context
    SetAutonomousMode(bool),
    SetSkipConfirmations(bool),
    Shutdown,
    /// Update reasoning stage in header context
    SetReasoningStage(Option<String>),
}

#[derive(Debug, Clone)]
pub enum InlineEvent {
    Submit(String),
    QueueSubmit(String),
    Steer(String),
    ProcessLatestQueued,
    /// Edit the newest queued input (pop into input buffer)
    EditQueue,
    Transient(TransientEvent),
    Cancel,
    Exit,
    Interrupt,
    Pause,
    Resume,
    BackgroundOperation,
    ScrollLineUp,
    ScrollLineDown,
    ScrollPageUp,
    ScrollPageDown,
    FileSelected(String),
    OpenFileInEditor(String),
    OpenUrl(String),
    LaunchEditor,
    ForceCancelPtySession,
    RequestInlinePromptSuggestion(String),
    /// Toggle editing mode (Shift+Tab cycles through Edit -> Auto -> Plan -> Edit).
    ToggleMode,
    HistoryPrevious,
    HistoryNext,
}

pub type InlineEventCallback = Arc<dyn Fn(&InlineEvent) + Send + Sync + 'static>;

impl From<crate::core_tui::types::InlineEvent> for InlineEvent {
    fn from(value: crate::core_tui::types::InlineEvent) -> Self {
        match value {
            crate::core_tui::types::InlineEvent::Submit(text) => Self::Submit(text),
            crate::core_tui::types::InlineEvent::QueueSubmit(text) => Self::QueueSubmit(text),
            crate::core_tui::types::InlineEvent::Steer(text) => Self::Steer(text),
            crate::core_tui::types::InlineEvent::ProcessLatestQueued => Self::ProcessLatestQueued,
            crate::core_tui::types::InlineEvent::EditQueue => Self::EditQueue,
            crate::core_tui::types::InlineEvent::Overlay(event) => Self::Transient(event.into()),
            crate::core_tui::types::InlineEvent::Cancel => Self::Cancel,
            crate::core_tui::types::InlineEvent::Exit => Self::Exit,
            crate::core_tui::types::InlineEvent::Interrupt => Self::Interrupt,
            crate::core_tui::types::InlineEvent::Pause => Self::Pause,
            crate::core_tui::types::InlineEvent::Resume => Self::Resume,
            crate::core_tui::types::InlineEvent::BackgroundOperation => Self::BackgroundOperation,
            crate::core_tui::types::InlineEvent::ScrollLineUp => Self::ScrollLineUp,
            crate::core_tui::types::InlineEvent::ScrollLineDown => Self::ScrollLineDown,
            crate::core_tui::types::InlineEvent::ScrollPageUp => Self::ScrollPageUp,
            crate::core_tui::types::InlineEvent::ScrollPageDown => Self::ScrollPageDown,
            crate::core_tui::types::InlineEvent::OpenFileInEditor(path) => {
                Self::OpenFileInEditor(path)
            }
            crate::core_tui::types::InlineEvent::OpenUrl(url) => Self::OpenUrl(url),
            crate::core_tui::types::InlineEvent::LaunchEditor => Self::LaunchEditor,
            crate::core_tui::types::InlineEvent::ForceCancelPtySession => {
                Self::ForceCancelPtySession
            }
            crate::core_tui::types::InlineEvent::RequestInlinePromptSuggestion(draft) => {
                Self::RequestInlinePromptSuggestion(draft)
            }
            crate::core_tui::types::InlineEvent::ToggleMode => Self::ToggleMode,
            crate::core_tui::types::InlineEvent::HistoryPrevious => Self::HistoryPrevious,
            crate::core_tui::types::InlineEvent::HistoryNext => Self::HistoryNext,
        }
    }
}

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

    pub fn append_line(&self, kind: InlineMessageKind, segments: Vec<InlineSegment>) {
        self.send_command(InlineCommand::AppendLine { kind, segments });
    }

    pub fn append_pasted_message(&self, kind: InlineMessageKind, text: String, line_count: usize) {
        self.send_command(InlineCommand::AppendPastedMessage {
            kind,
            text,
            line_count,
        });
    }

    pub fn inline(&self, kind: InlineMessageKind, segment: InlineSegment) {
        self.send_command(InlineCommand::Inline { kind, segment });
    }

    pub fn replace_last(
        &self,
        count: usize,
        kind: InlineMessageKind,
        lines: Vec<Vec<InlineSegment>>,
    ) {
        self.send_command(InlineCommand::ReplaceLast {
            count,
            kind,
            lines,
            link_ranges: None,
        });
    }

    pub fn replace_last_with_links(
        &self,
        count: usize,
        kind: InlineMessageKind,
        lines: Vec<Vec<InlineSegment>>,
        link_ranges: Vec<Vec<InlineLinkRange>>,
    ) {
        self.send_command(InlineCommand::ReplaceLast {
            count,
            kind,
            lines,
            link_ranges: Some(link_ranges),
        });
    }

    pub fn suspend_event_loop(&self) {
        self.send_command(InlineCommand::SuspendEventLoop);
    }

    pub fn resume_event_loop(&self) {
        self.send_command(InlineCommand::ResumeEventLoop);
    }

    pub fn clear_input_queue(&self) {
        self.send_command(InlineCommand::ClearInputQueue);
    }

    pub fn set_prompt(&self, prefix: String, style: InlineTextStyle) {
        self.send_command(InlineCommand::SetPrompt { prefix, style });
    }

    pub fn set_placeholder(&self, hint: Option<String>) {
        self.set_placeholder_with_style(hint, None);
    }

    pub fn set_placeholder_with_style(&self, hint: Option<String>, style: Option<InlineTextStyle>) {
        self.send_command(InlineCommand::SetPlaceholder { hint, style });
    }

    pub fn set_message_labels(&self, agent: Option<String>, user: Option<String>) {
        self.send_command(InlineCommand::SetMessageLabels { agent, user });
    }

    pub fn set_header_context(&self, context: InlineHeaderContext) {
        self.send_command(InlineCommand::SetHeaderContext {
            context: Box::new(context),
        });
    }

    pub fn set_input_status(&self, left: Option<String>, right: Option<String>) {
        self.send_command(InlineCommand::SetInputStatus { left, right });
    }

    pub fn set_theme(&self, theme: InlineTheme) {
        self.send_command(InlineCommand::SetTheme { theme });
    }

    pub fn set_appearance(&self, appearance: AppearanceConfig) {
        self.send_command(InlineCommand::SetAppearance { appearance });
    }

    pub fn set_vim_mode_enabled(&self, enabled: bool) {
        self.send_command(InlineCommand::SetVimModeEnabled(enabled));
    }

    pub fn set_queued_inputs(&self, entries: Vec<String>) {
        self.send_command(InlineCommand::SetQueuedInputs { entries });
    }

    pub fn set_subprocess_entries(&self, entries: Vec<String>) {
        self.send_command(InlineCommand::SetSubprocessEntries { entries });
    }

    pub fn set_subagent_preview(&self, text: Option<String>) {
        self.send_command(InlineCommand::SetSubagentPreview { text });
    }

    pub fn set_local_agents(&self, entries: Vec<LocalAgentEntry>) {
        self.send_command(InlineCommand::SetLocalAgents { entries });
    }

    pub fn set_cursor_visible(&self, visible: bool) {
        self.send_command(InlineCommand::SetCursorVisible(visible));
    }

    pub fn set_input_enabled(&self, enabled: bool) {
        self.send_command(InlineCommand::SetInputEnabled(enabled));
    }

    pub fn set_input(&self, content: String) {
        self.send_command(InlineCommand::SetInput(content));
    }

    pub fn apply_suggested_prompt(&self, content: String) {
        self.send_command(InlineCommand::ApplySuggestedPrompt(content));
    }

    pub fn set_inline_prompt_suggestion(&self, suggestion: String, llm_generated: bool) {
        self.send_command(InlineCommand::SetInlinePromptSuggestion {
            suggestion,
            llm_generated,
        });
    }

    pub fn clear_inline_prompt_suggestion(&self) {
        self.send_command(InlineCommand::ClearInlinePromptSuggestion);
    }

    pub fn clear_input(&self) {
        self.send_command(InlineCommand::ClearInput);
    }

    pub fn force_redraw(&self) {
        self.send_command(InlineCommand::ForceRedraw);
    }

    pub fn shutdown(&self) {
        self.send_command(InlineCommand::Shutdown);
    }

    /// Update editing mode state in the header display
    pub fn set_editing_mode(&self, mode: EditingMode) {
        self.send_command(InlineCommand::SetEditingMode(mode));
    }

    /// Update autonomous mode state in the header display
    pub fn set_autonomous_mode(&self, enabled: bool) {
        self.send_command(InlineCommand::SetAutonomousMode(enabled));
    }

    pub fn show_transient(&self, request: TransientRequest) {
        self.send_command(InlineCommand::ShowTransient {
            request: Box::new(request),
        });
    }

    pub fn show_modal(
        &self,
        title: String,
        lines: Vec<String>,
        secure_prompt: Option<SecurePromptConfig>,
    ) {
        self.show_transient(TransientRequest::Modal(ModalOverlayRequest {
            title,
            lines,
            secure_prompt,
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
        self.show_transient(TransientRequest::List(ListOverlayRequest {
            title,
            lines,
            items,
            selected,
            search,
            footer_hint: None,
            hotkeys: Vec::new(),
        }));
    }

    pub fn configure_file_palette(&self, files: Vec<String>, workspace: std::path::PathBuf) {
        self.show_transient(TransientRequest::FilePalette(FilePaletteTransientRequest {
            files,
            workspace,
            visible: None,
        }));
    }

    pub fn configure_agent_palette(&self, agents: Vec<AgentPaletteItem>) {
        self.show_transient(TransientRequest::AgentPalette(
            AgentPaletteTransientRequest {
                agents,
                visible: None,
            },
        ));
    }

    pub fn show_history_picker(&self) {
        self.show_transient(TransientRequest::HistoryPicker);
    }

    pub fn show_task_panel(&self) {
        self.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
            lines: Vec::new(),
            visible: Some(true),
        }));
    }

    pub fn show_local_agents(&self) {
        self.show_transient(TransientRequest::LocalAgents(LocalAgentsTransientRequest {
            visible: Some(true),
        }));
    }

    pub fn hide_local_agents(&self) {
        self.show_transient(TransientRequest::LocalAgents(LocalAgentsTransientRequest {
            visible: Some(false),
        }));
    }

    pub fn hide_task_panel(&self) {
        self.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
            lines: Vec::new(),
            visible: Some(false),
        }));
    }

    pub fn update_task_panel(&self, lines: Vec<String>) {
        self.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
            lines,
            visible: None,
        }));
    }

    pub fn close_transient(&self) {
        self.send_command(InlineCommand::CloseTransient);
    }

    pub fn close_modal(&self) {
        self.close_transient();
    }

    pub fn clear_screen(&self) {
        self.send_command(InlineCommand::ClearScreen);
    }

    pub fn set_skip_confirmations(&self, skip: bool) {
        self.send_command(InlineCommand::SetSkipConfirmations(skip));
    }

    pub fn set_reasoning_stage(&self, stage: Option<String>) {
        self.send_command(InlineCommand::SetReasoningStage(stage));
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

    pub fn clone_inline_handle(&self) -> InlineHandle {
        InlineHandle {
            sender: self.handle.sender.clone(),
        }
    }
}

impl crate::core_tui::runner::TuiCommand for InlineCommand {
    fn is_suspend_event_loop(&self) -> bool {
        matches!(self, InlineCommand::SuspendEventLoop)
    }

    fn is_resume_event_loop(&self) -> bool {
        matches!(self, InlineCommand::ResumeEventLoop)
    }

    fn is_clear_input_queue(&self) -> bool {
        matches!(self, InlineCommand::ClearInputQueue)
    }

    fn is_force_redraw(&self) -> bool {
        matches!(self, InlineCommand::ForceRedraw)
    }
}
