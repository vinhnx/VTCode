use crate::tui::core_tui::types::{
    define_inline_message_commands, impl_inline_control_methods, impl_inline_message_methods,
};
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

use chrono::{DateTime, Utc};
use hashbrown::HashMap;
use tokio::sync::{
    Notify,
    mpsc::{UnboundedReceiver, UnboundedSender},
};
use unicode_width::UnicodeWidthStr;
use vtcode_commons::ui_protocol::{CompactActivityMetadata, SlashCommandItem, TaskItemStatus, ToolOutputId};

use super::overlay::{
    AgentPaletteItem, AgentPaletteTransientRequest, FilePaletteTransientRequest, ListOverlayRequest,
    LocalAgentsTransientRequest, ModalOverlayRequest, TaskPanelMetadata, TaskPanelTransientRequest, TransientEvent,
    TransientRequest,
};
use crate::tui::core_tui::session::config::AppearanceConfig;
pub use crate::tui::core_tui::types::SubmittedInput;
use crate::tui::core_tui::types::{
    ActivityState, ExecSessionAction, InlineHeaderContext, InlineListItem, InlineListSearchConfig, InlineListSelection,
    InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme, LocalAgentEntry, SecurePromptConfig,
};
use crate::tui::options::FullscreenInteractionSettings;

const MAX_DEFERRED_EVENTS: usize = 32;

/// Shared state and wake-up signal for deferred bridge input.
///
/// The UI can close a captured-input surface without emitting an inline event
/// (for example, Esc on the local-agents drawer). A boolean alone cannot wake
/// an already waiting [`InlineSession::next_event`], so the state carries a
/// notification alongside the atomic flag.
#[derive(Default)]
pub(crate) struct TransientActivitySignal {
    active: AtomicBool,
    changed: Notify,
}

impl TransientActivitySignal {
    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    pub(crate) fn set_active(&self, active: bool) {
        let previous = self.active.swap(active, Ordering::AcqRel);
        if previous != active {
            self.changed.notify_one();
        }
    }

    async fn wait_for_change(&self) {
        self.changed.notified().await;
    }
}

/// A user prompt from a previous session archive, used to populate the history picker.
#[derive(Debug, Clone)]
pub struct ArchivedPromptEntry {
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub session_label: String,
}

define_inline_message_commands! {
    pub enum InlineCommand {
        /// Retain one complete tool-call capture for the session-local viewer.
        RecordToolOutput {
            id: ToolOutputId,
            lines: Vec<String>,
        },
        /// Append a summary line and associate it with a previously recorded
        /// capture. This is a UI-only identity edge, not a transcript event.
        AppendToolOutputLine {
            id: ToolOutputId,
            kind: InlineMessageKind,
            segments: Vec<InlineSegment>,
        },
        /// Append a compact successful-command activity row.
        ///
        /// UI-only identity edge derived from the canonical `ThreadEvent` tool
        /// outcome, not a new source of truth: grouping must stay consistent with
        /// `vtcode_commons::ui_protocol::tool_summary` boundaries.
        AppendCompactActivity(CompactActivityMetadata),
        /// Store a completed-edit review payload for explicit expand activation.
        RecordDiffReview(vtcode_commons::ui_protocol::DiffReviewAnchor),
        /// Focus an existing capture in Transcript Review without submitting input.
        FocusTranscriptReview {
            id: ToolOutputId,
        },
        /// Replace the current compact successful-command activity row with an
        /// updated contiguous group.
        ///
        /// UI-only identity edge; see [`InlineCommand::AppendCompactActivity`].
        /// Only contiguous successful command activity may group; transient PTY
        /// rows stay separate.
        ReplaceCompactActivity(CompactActivityMetadata),
        /// Replace the live PTY preview block with a compact activity row after
        /// the command has completed. Complete output is retained separately.
        ///
        /// UI-only identity edge; see [`InlineCommand::AppendCompactActivity`].
        CollapsePtyBlock(CompactActivityMetadata),
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
        /// Replace the live action bindings after a valid configuration reload.
        SetKeyBindings {
            bindings: HashMap<String, Vec<String>>,
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
        /// Inject archived prompts from previous sessions into the history picker.
        SetArchivedHistory {
            entries: Vec<ArchivedPromptEntry>,
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
        ShowTransient {
            request: Box<TransientRequest>,
        },
        /// Deliver the full recursive workspace file list (discovered in the
        /// background) so the file palette's Search mode can match against it.
        UpdateFilePaletteSearch {
            files: Vec<String>,
        },
        /// Replace the slash-command palette after background prompt-template
        /// discovery. Lets first paint spawn with built-ins only; templates merge
        /// in without blocking `spawn_session_with_options`.
        SetSlashCommands {
            commands: Vec<SlashCommandItem>,
        },
        CloseTransient,
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
    /// Submit text from the WebMCP bridge without interpreting slash commands.
    WebmcpSubmit(SubmittedInput),
    QueueSubmit(SubmittedInput),
    Steer(SubmittedInput),
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
    ExecSessionAction {
        id: String,
        action: ExecSessionAction,
    },
    ScrollLineUp,
    ScrollLineDown,
    ScrollPageUp,
    ScrollPageDown,
    JumpToLastChange,
    FileSelected(String),
    OpenFileInEditor(String),
    OpenUrl(String),
    LaunchEditor {
        draft: String,
    },
    OpenToolOutputInEditor(String),
    OpenToolOutputScrollback(String),
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

impl From<crate::tui::core_tui::types::InlineEvent> for InlineEvent {
    fn from(value: crate::tui::core_tui::types::InlineEvent) -> Self {
        match value {
            crate::tui::core_tui::types::InlineEvent::Submit(text) => Self::Submit(text),
            crate::tui::core_tui::types::InlineEvent::QueueSubmit(text) => Self::QueueSubmit(text),
            crate::tui::core_tui::types::InlineEvent::Steer(text) => Self::Steer(text),
            crate::tui::core_tui::types::InlineEvent::ProcessLatestQueued => Self::ProcessLatestQueued,
            crate::tui::core_tui::types::InlineEvent::EditQueue => Self::EditQueue,
            crate::tui::core_tui::types::InlineEvent::Overlay(event) => Self::Transient(event.into()),
            crate::tui::core_tui::types::InlineEvent::Cancel => Self::Cancel,
            crate::tui::core_tui::types::InlineEvent::Exit => Self::Exit,
            crate::tui::core_tui::types::InlineEvent::Interrupt => Self::Interrupt,
            crate::tui::core_tui::types::InlineEvent::Pause => Self::Pause,
            crate::tui::core_tui::types::InlineEvent::Resume => Self::Resume,
            crate::tui::core_tui::types::InlineEvent::BackgroundOperation => Self::BackgroundOperation,
            crate::tui::core_tui::types::InlineEvent::ExecSessionAction { id, action } => {
                Self::ExecSessionAction { id, action }
            }
            crate::tui::core_tui::types::InlineEvent::ScrollLineUp => Self::ScrollLineUp,
            crate::tui::core_tui::types::InlineEvent::ScrollLineDown => Self::ScrollLineDown,
            crate::tui::core_tui::types::InlineEvent::ScrollPageUp => Self::ScrollPageUp,
            crate::tui::core_tui::types::InlineEvent::ScrollPageDown => Self::ScrollPageDown,
            crate::tui::core_tui::types::InlineEvent::JumpToLastChange => Self::JumpToLastChange,
            crate::tui::core_tui::types::InlineEvent::OpenFileInEditor(path) => Self::OpenFileInEditor(path),
            crate::tui::core_tui::types::InlineEvent::OpenUrl(url) => Self::OpenUrl(url),
            crate::tui::core_tui::types::InlineEvent::LaunchEditor { draft } => Self::LaunchEditor { draft },
            crate::tui::core_tui::types::InlineEvent::ForceCancelPtySession => Self::ForceCancelPtySession,
            crate::tui::core_tui::types::InlineEvent::RequestInlinePromptSuggestion(draft) => {
                Self::RequestInlinePromptSuggestion(draft)
            }
            crate::tui::core_tui::types::InlineEvent::CyclePrimaryAgent => Self::CyclePrimaryAgent,
            crate::tui::core_tui::types::InlineEvent::CyclePrimaryAgentPrevious => Self::CyclePrimaryAgentPrevious,
            crate::tui::core_tui::types::InlineEvent::SelectPrimaryAgent { name } => Self::SelectPrimaryAgent { name },
            crate::tui::core_tui::types::InlineEvent::HistoryPrevious => Self::HistoryPrevious,
            crate::tui::core_tui::types::InlineEvent::HistoryNext => Self::HistoryNext,
            crate::tui::core_tui::types::InlineEvent::ToggleToolDisplayMode => Self::ToggleToolDisplayMode,
        }
    }
}

#[derive(Default)]
struct InlineLayoutState {
    agent_label_frame_width: AtomicUsize,
}

#[derive(Clone)]
struct HandleProgress {
    operation: vtcode_commons::ui_protocol::ProgressOperation,
    phase: vtcode_commons::ui_protocol::ProgressPhase,
}

#[derive(Clone)]
pub struct InlineHandle {
    pub(crate) sender: UnboundedSender<InlineCommand>,
    message_layout: Arc<InlineLayoutState>,
    deferred_events: Arc<Mutex<VecDeque<InlineEvent>>>,
    transient_activity: Arc<TransientActivitySignal>,
    /// Whether the app session owns the authoritative transient stack. A
    /// standalone handle has no stack to resync after a close, so it updates
    /// the signal eagerly; the UI-owned handle waits for AppSession to expose
    /// the next visible surface before releasing input ownership.
    ui_owned_transient_activity: bool,
    next_tool_output_id: Arc<AtomicU64>,
    progress_operation: Arc<Mutex<Option<HandleProgress>>>,
}

impl InlineHandle {
    pub fn new_for_tests(sender: UnboundedSender<InlineCommand>) -> Self {
        Self::new(sender)
    }

    pub(crate) fn new(sender: UnboundedSender<InlineCommand>) -> Self {
        Self::new_with_transient_signal_and_ownership(sender, Arc::new(TransientActivitySignal::default()), false)
    }

    pub(crate) fn new_with_transient_signal(
        sender: UnboundedSender<InlineCommand>,
        transient_activity: Arc<TransientActivitySignal>,
    ) -> Self {
        Self::new_with_transient_signal_and_ownership(sender, transient_activity, true)
    }

    fn new_with_transient_signal_and_ownership(
        sender: UnboundedSender<InlineCommand>,
        transient_activity: Arc<TransientActivitySignal>,
        ui_owned_transient_activity: bool,
    ) -> Self {
        Self {
            sender,
            message_layout: Arc::new(InlineLayoutState::default()),
            deferred_events: Arc::new(Mutex::new(VecDeque::new())),
            transient_activity,
            ui_owned_transient_activity,
            next_tool_output_id: Arc::new(AtomicU64::new(0)),
            progress_operation: Arc::new(Mutex::new(None)),
        }
    }

    /// Defer an input event until the current transient overlay closes.
    pub fn defer_event(&self, event: InlineEvent) -> anyhow::Result<()> {
        let mut deferred_events = self
            .deferred_events
            .lock()
            .map_err(|error| anyhow::anyhow!("deferred input queue poisoned: {error}"))?;
        if deferred_events.len() >= MAX_DEFERRED_EVENTS {
            return Err(anyhow::anyhow!("deferred input queue is full"));
        }
        deferred_events.push_back(event);
        Ok(())
    }

    fn take_deferred_event(&self) -> Option<InlineEvent> {
        self.deferred_events.lock().ok()?.pop_front()
    }

    pub fn has_deferred_event(&self) -> bool {
        self.deferred_events.lock().is_ok_and(|events| !events.is_empty())
    }

    fn transient_active(&self) -> bool {
        self.transient_activity.is_active()
    }

    async fn wait_for_transient_activity_change(&self) {
        self.transient_activity.wait_for_change().await;
    }

    fn send_command(&self, command: InlineCommand) {
        if self.sender.is_closed() {
            return;
        }
        let _ = self.sender.send(command);
    }

    impl_inline_message_methods!(InlineCommand);
    impl_inline_control_methods!(InlineCommand);

    /// Own a transient row through completion, errors and cancellation.
    pub fn begin_progress(&self, phase: vtcode_commons::ui_protocol::ProgressPhase) -> ProgressGuard {
        let operation = vtcode_commons::ui_protocol::ProgressOperation::start();
        if let Ok(mut current) = self.progress_operation.lock() {
            *current = Some(HandleProgress { operation, phase });
        }
        self.update_progress(vtcode_commons::ui_protocol::ProgressUpdate::Begin { operation, phase });
        ProgressGuard {
            handle: self.clone(),
            operation,
            clear_on_drop: true,
        }
    }

    /// Resume ownership after the interaction loop transfers a submitted turn.
    pub fn resume_progress(&self, phase: vtcode_commons::ui_protocol::ProgressPhase) -> ProgressGuard {
        if let Some(operation) = self.current_progress_operation() {
            self.set_progress_phase(phase);
            ProgressGuard {
                handle: self.clone(),
                operation,
                clear_on_drop: true,
            }
        } else {
            self.begin_progress(phase)
        }
    }

    pub fn current_progress_operation(&self) -> Option<vtcode_commons::ui_protocol::ProgressOperation> {
        self.progress_operation
            .lock()
            .ok()
            .and_then(|current| current.as_ref().map(|current| current.operation))
    }

    pub fn set_progress_phase(&self, phase: vtcode_commons::ui_protocol::ProgressPhase) {
        let _ = self.replace_progress_phase(phase);
    }

    /// Update the current owner in place and retain its previous phase for scoped work.
    pub fn replace_progress_phase(
        &self,
        phase: vtcode_commons::ui_protocol::ProgressPhase,
    ) -> Option<vtcode_commons::ui_protocol::ProgressUpdate> {
        let mut current = self.progress_operation.lock().ok()?;
        let current = current.as_mut()?;
        let previous =
            vtcode_commons::ui_protocol::ProgressUpdate::Phase { operation: current.operation, phase: current.phase };
        current.phase = phase;
        self.update_progress(vtcode_commons::ui_protocol::ProgressUpdate::Phase {
            operation: current.operation,
            phase,
        });
        Some(previous)
    }

    pub fn record_tool_output(&self, lines: Vec<String>) -> ToolOutputId {
        let id = self.next_tool_output_id.fetch_add(1, Ordering::Relaxed);
        self.send_command(InlineCommand::RecordToolOutput { id, lines });
        id
    }

    /// Display retained canonical evidence using the existing review geometry.
    pub fn review_evidence(&self, lines: Vec<String>) -> bool {
        let id = self.next_tool_output_id.fetch_add(1, Ordering::Relaxed);
        self.sender.send(InlineCommand::RecordToolOutput { id, lines }).is_ok()
            && self.sender.send(InlineCommand::FocusTranscriptReview { id }).is_ok()
    }

    pub fn append_tool_output_line(&self, id: ToolOutputId, kind: InlineMessageKind, segments: Vec<InlineSegment>) {
        self.send_command(InlineCommand::AppendToolOutputLine { id, kind, segments });
    }

    pub fn append_compact_activity(&self, activity: CompactActivityMetadata) {
        self.send_command(InlineCommand::AppendCompactActivity(activity));
    }
    pub fn record_diff_review(&self, anchor: vtcode_commons::ui_protocol::DiffReviewAnchor) {
        self.send_command(InlineCommand::RecordDiffReview(anchor));
    }

    pub fn replace_compact_activity(&self, activity: CompactActivityMetadata) {
        self.send_command(InlineCommand::ReplaceCompactActivity(activity));
    }

    pub fn collapse_pty_block(&self, activity: CompactActivityMetadata) {
        self.send_command(InlineCommand::CollapsePtyBlock(activity));
    }

    fn set_placeholder_with_style(&self, hint: Option<String>, style: Option<InlineTextStyle>) {
        self.send_command(InlineCommand::SetPlaceholder { hint, style });
    }

    pub fn set_message_labels(&self, agent: Option<String>, user: Option<String>) {
        let agent_label_frame_width = agent
            .as_deref()
            .filter(|label| !label.is_empty())
            .map(|label| UnicodeWidthStr::width(label) + 1)
            .unwrap_or_default();
        self.message_layout
            .agent_label_frame_width
            .store(agent_label_frame_width, Ordering::Relaxed);
        self.send_command(InlineCommand::SetMessageLabels { agent, user });
    }

    /// Return the display width of the current agent label and its separator.
    pub fn agent_label_frame_width(&self) -> usize {
        self.message_layout.agent_label_frame_width.load(Ordering::Relaxed)
    }

    pub fn set_key_bindings(&self, bindings: HashMap<String, Vec<String>>) {
        self.send_command(InlineCommand::SetKeyBindings { bindings });
    }

    pub fn set_local_agents(&self, entries: Vec<LocalAgentEntry>) {
        self.send_command(InlineCommand::SetLocalAgents { entries });
    }

    pub fn set_archived_history(&self, entries: Vec<ArchivedPromptEntry>) {
        self.send_command(InlineCommand::SetArchivedHistory { entries });
    }

    pub fn show_transient(&self, request: TransientRequest) {
        if let Some(active) = transient_input_state(&request) {
            // Eagerly acquire ownership so bridge input cannot overtake the
            // queued UI command. A UI-owned close is released by AppSession,
            // which can account for any lower captured surface on its stack.
            if self.sender.is_closed() && self.ui_owned_transient_activity {
                // A production UI that has already torn down cannot process
                // this request, so do not strand deferred input as active.
                self.transient_activity.set_active(false);
            } else if active || !self.ui_owned_transient_activity {
                self.transient_activity.set_active(active);
            }
        }
        self.send_command(InlineCommand::ShowTransient { request: Box::new(request) });
    }

    pub fn show_modal(&self, title: String, lines: Vec<String>, secure_prompt: Option<SecurePromptConfig>) {
        self.show_transient(TransientRequest::Modal(ModalOverlayRequest {
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
        self.show_transient(TransientRequest::List(ListOverlayRequest {
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

    pub fn configure_file_palette(
        &self,
        workspace: std::path::PathBuf,
        dir_lister: crate::tui::core_tui::app::session::file_palette::DirLister,
    ) {
        self.show_transient(TransientRequest::FilePalette(FilePaletteTransientRequest {
            dir_lister,
            workspace,
            visible: None,
        }));
    }

    /// Push the full recursive file list discovered in the background so Search
    /// mode has a corpus to match against. Browse mode does not require it.
    pub fn set_file_palette_search_index(&self, files: Vec<String>) {
        self.send_command(InlineCommand::UpdateFilePaletteSearch { files });
    }

    /// Replace slash palette commands after background template discovery.
    pub fn set_slash_commands(&self, commands: Vec<SlashCommandItem>) {
        self.send_command(InlineCommand::SetSlashCommands { commands });
    }

    pub fn configure_agent_palette(&self, agents: Vec<AgentPaletteItem>) {
        self.show_transient(TransientRequest::AgentPalette(AgentPaletteTransientRequest { agents, visible: None }));
    }

    pub fn show_history_picker(&self) {
        self.show_transient(TransientRequest::HistoryPicker);
    }

    pub fn show_task_panel(&self) {
        self.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
            lines: Vec::new(),
            statuses: Vec::new(),
            current: None,
            visible: Some(true),
            metadata: None,
        }));
    }

    pub fn show_local_agents(&self) {
        self.show_transient(TransientRequest::LocalAgents(LocalAgentsTransientRequest { visible: Some(true) }));
    }

    pub fn hide_local_agents(&self) {
        self.show_transient(TransientRequest::LocalAgents(LocalAgentsTransientRequest { visible: Some(false) }));
    }

    pub fn hide_task_panel(&self) {
        self.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
            lines: Vec::new(),
            statuses: Vec::new(),
            current: None,
            visible: Some(false),
            metadata: None,
        }));
    }

    pub fn update_task_panel(&self, lines: Vec<String>) {
        self.update_task_panel_with_metadata(lines, None);
    }

    pub fn update_task_panel_with_metadata(&self, lines: Vec<String>, metadata: Option<TaskPanelMetadata>) {
        self.update_task_panel_with_statuses(lines, Vec::new(), None, metadata);
    }

    /// Update the panel body with per-row statuses for text-styling.
    ///
    /// `statuses` runs parallel to `lines`; `current` is the focused-row
    /// index for accent emphasis. Length mismatches fall back to the uniform
    /// base style so legacy callers keep working.
    pub fn update_task_panel_with_statuses(
        &self,
        lines: Vec<String>,
        statuses: Vec<TaskItemStatus>,
        current: Option<usize>,
        metadata: Option<TaskPanelMetadata>,
    ) {
        self.show_transient(TransientRequest::TaskPanel(TaskPanelTransientRequest {
            lines,
            statuses,
            current,
            visible: None,
            metadata,
        }));
    }

    pub fn close_transient(&self) {
        // AppSession synchronizes the shared signal from its nested transient
        // stack. Clearing eagerly here would briefly release deferred bridge
        // input while a lower captured-input surface remains visible.
        if !self.ui_owned_transient_activity || self.sender.is_closed() {
            self.transient_activity.set_active(false);
        }
        self.send_command(InlineCommand::CloseTransient);
    }

    pub fn close_modal(&self) {
        self.close_transient();
    }
}

/// Return the deferred-input state for requests that own or release input.
/// Passive updates and palette preloads leave the current state unchanged.
fn transient_input_state(request: &TransientRequest) -> Option<bool> {
    match request {
        TransientRequest::Modal(_)
        | TransientRequest::List(_)
        | TransientRequest::Wizard(_)
        | TransientRequest::Diff(_)
        | TransientRequest::HistoryPicker
        | TransientRequest::SlashPalette => Some(true),
        TransientRequest::FilePalette(request) => request.visible,
        TransientRequest::AgentPalette(request) => request.visible,
        TransientRequest::LocalAgents(request) => request.visible,
        // The task panel is a passive surface; it keeps the input enabled and
        // therefore must not hold bridge events while it is visible.
        TransientRequest::TaskPanel(_) => None,
    }
}

/// Scoped owner; transferring a turn preserves its monotonic start time.
pub struct ProgressGuard {
    handle: InlineHandle,
    operation: vtcode_commons::ui_protocol::ProgressOperation,
    clear_on_drop: bool,
}

impl ProgressGuard {
    pub fn operation(&self) -> vtcode_commons::ui_protocol::ProgressOperation {
        self.operation
    }

    pub fn transfer(mut self) {
        self.clear_on_drop = false;
    }
}

impl Drop for ProgressGuard {
    fn drop(&mut self) {
        if !self.clear_on_drop {
            return;
        }
        self.handle
            .update_progress(vtcode_commons::ui_protocol::ProgressUpdate::Finish { operation: self.operation });
        if let Ok(mut current) = self.handle.progress_operation.lock()
            && current.as_ref().is_some_and(|current| current.operation == self.operation)
        {
            *current = None;
        }
    }
}

pub struct InlineSession {
    pub handle: InlineHandle,
    pub events: UnboundedReceiver<InlineEvent>,
    /// Background task running the terminal event loop. The host must await
    /// it after `shutdown()` before restoring terminal state itself;
    /// otherwise the task's final frames are painted onto the main screen.
    pub worker: Option<tokio::task::JoinHandle<()>>,
}

impl InlineSession {
    pub async fn next_event(&mut self) -> Option<InlineEvent> {
        loop {
            if !self.handle.transient_active()
                && let Some(event) = self.handle.take_deferred_event()
            {
                return Some(event);
            }

            tokio::select! {
                event = self.events.recv() => match event {
                    Some(event) => return Some(event),
                    None if self.handle.transient_active() && self.handle.has_deferred_event() => {
                        // A closed UI channel must not strand bridge input
                        // behind a captured-input surface. Wait for the
                        // surface to release ownership, then drain it.
                        self.handle.wait_for_transient_activity_change().await;
                    }
                    None => return self.handle.take_deferred_event(),
                },
                _ = self.handle.wait_for_transient_activity_change() => {}
            }
        }
    }

    /// Wait for the TUI task to finish its own terminal teardown.
    ///
    /// Returns `true` when the task exited (or no task was spawned); `false`
    /// when it was still running after `timeout`, in which case the caller
    /// should force-restore the terminal as a backstop. On timeout the task
    /// is aborted to guarantee it cannot keep raw mode or the alternate
    /// screen active after the host has restored the terminal.
    pub async fn wait_for_exit(&mut self, timeout: std::time::Duration) -> bool {
        let Some(mut worker) = self.worker.take() else {
            return true;
        };
        let result = tokio::time::timeout(timeout, &mut worker).await;
        match result {
            Ok(_) => true,
            Err(_) => {
                worker.abort();
                // Give abort a brief moment to run drop handlers.
                let _ = tokio::time::timeout(std::time::Duration::from_millis(50), worker).await;
                false
            }
        }
    }

    pub fn set_skip_confirmations(&mut self, skip: bool) {
        self.handle.set_skip_confirmations(skip);
    }

    pub fn set_color_scheme_auto(&mut self, enabled: bool) {
        self.handle.set_color_scheme_auto(enabled);
    }

    pub fn clone_inline_handle(&self) -> InlineHandle {
        self.handle.clone()
    }
}

impl crate::tui::core_tui::runner::TuiCommand for InlineCommand {
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

    fn is_stop_event_stream(&self) -> bool {
        matches!(self, InlineCommand::StopEventStream)
    }

    fn is_start_event_stream(&self) -> bool {
        matches!(self, InlineCommand::StartEventStream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_scope_transfers_identity_and_only_clears_its_operation() {
        use vtcode_commons::ui_protocol::ProgressPhase;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let first = handle.begin_progress(ProgressPhase::PreparingContext);
        let operation = first.operation();
        first.transfer();
        let resumed = handle.resume_progress(ProgressPhase::SavingCheckpoint);
        assert_eq!(resumed.operation(), operation);
        let replacement = handle.begin_progress(ProgressPhase::Initializing);
        drop(resumed);
        assert_eq!(handle.current_progress_operation(), Some(replacement.operation()));
        drop(replacement);
        assert_eq!(handle.current_progress_operation(), None);
        let updates: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert_eq!(updates.len(), 5);
    }

    #[tokio::test]
    async fn cancellation_drops_progress_scope_and_emits_matching_finish() {
        use vtcode_commons::ui_protocol::{ProgressPhase, ProgressUpdate};
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(tx);
        let worker_handle = handle.clone();
        let worker = tokio::spawn(async move {
            let _guard = worker_handle.begin_progress(ProgressPhase::WaitingForModel);
            std::future::pending::<()>().await;
        });
        let operation = match rx.recv().await.unwrap() {
            InlineCommand::UpdateProgress(ProgressUpdate::Begin { operation, .. }) => operation,
            _ => panic!("expected progress begin"),
        };
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        assert!(
            matches!(rx.recv().await, Some(InlineCommand::UpdateProgress(ProgressUpdate::Finish { operation: finished })) if finished == operation)
        );
        assert!(handle.current_progress_operation().is_none());
    }

    #[test]
    fn evidence_navigation_records_capture_before_focus_and_reports_closed_ui() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        assert!(handle.review_evidence(vec!["recorded event".into()]));
        let InlineCommand::RecordToolOutput { id, lines } = receiver.try_recv().unwrap() else {
            panic!("capture must precede focus")
        };
        assert_eq!(lines, ["recorded event"]);
        assert!(
            matches!(receiver.try_recv().unwrap(), InlineCommand::FocusTranscriptReview { id: focused } if focused == id)
        );
        assert!(receiver.try_recv().is_err(), "navigation must not submit input");
        drop(receiver);
        assert!(!handle.review_evidence(vec!["expired receiver".into()]));
    }

    fn test_session() -> (InlineHandle, InlineSession, UnboundedSender<InlineEvent>) {
        let (command_sender, _command_receiver) = tokio::sync::mpsc::unbounded_channel();
        let (event_sender, event_receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(command_sender);
        let session = InlineSession {
            handle: handle.clone(),
            events: event_receiver,
            worker: None,
        };
        (handle, session, event_sender)
    }

    #[tokio::test]
    async fn deferred_event_waits_until_transient_closes() {
        let (handle, mut session, event_sender) = test_session();

        handle
            .defer_event(InlineEvent::WebmcpSubmit("deferred".into()))
            .expect("defer bridge event");
        handle.show_list_modal("test".into(), Vec::new(), Vec::new(), None, None);
        event_sender.send(InlineEvent::Cancel).expect("send modal result");

        let modal_event = session.next_event().await;
        assert!(matches!(modal_event, Some(InlineEvent::Cancel)));

        handle.close_transient();
        let deferred_event = session.next_event().await;
        assert!(matches!(deferred_event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "deferred"));
    }

    #[tokio::test]
    async fn transient_close_wakes_waiting_deferred_event_consumer() {
        let (handle, session, _event_sender) = test_session();

        handle
            .defer_event(InlineEvent::WebmcpSubmit("deferred".into()))
            .expect("defer bridge event");
        handle.show_local_agents();

        let waiter = tokio::spawn(async move {
            let mut session = session;
            session.next_event().await
        });
        tokio::task::yield_now().await;

        // A captured-input surface can close without sending a user event.
        handle.transient_activity.set_active(false);

        let deferred_event = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("deferred consumer should wake after transient close")
            .expect("deferred consumer task should finish");
        assert!(matches!(deferred_event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "deferred"));
    }

    #[tokio::test]
    async fn closed_event_stream_waits_for_transient_close_before_deferred_event() {
        let (handle, session, event_sender) = test_session();

        handle
            .defer_event(InlineEvent::WebmcpSubmit("deferred".into()))
            .expect("defer bridge event");
        handle.show_local_agents();
        drop(event_sender);

        let mut waiter = tokio::spawn(async move {
            let mut session = session;
            session.next_event().await
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut waiter)
                .await
                .is_err(),
            "closed UI channel must not bypass an active captured-input surface"
        );

        handle.transient_activity.set_active(false);
        let deferred_event = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("deferred consumer should wake after transient close")
            .expect("deferred consumer task should finish");
        assert!(matches!(deferred_event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "deferred"));
    }

    #[tokio::test]
    async fn palette_preload_does_not_hold_deferred_input() {
        let (handle, mut session, _event_sender) = test_session();

        handle
            .defer_event(InlineEvent::WebmcpSubmit("deferred".into()))
            .expect("defer bridge event");
        handle.configure_agent_palette(Vec::new());

        let deferred_event = session.next_event().await;
        assert!(matches!(deferred_event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "deferred"));
    }

    #[tokio::test]
    async fn passive_task_panel_updates_do_not_hold_deferred_input() {
        let (handle, mut session, _event_sender) = test_session();

        handle
            .defer_event(InlineEvent::WebmcpSubmit("deferred".into()))
            .expect("defer bridge event");
        handle.show_task_panel();
        handle.update_task_panel(Vec::new());

        let deferred_event = session.next_event().await;
        assert!(matches!(deferred_event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "deferred"));
    }

    #[tokio::test]
    async fn captured_local_agents_drawer_releases_deferred_input_when_hidden() {
        let (handle, mut session, event_sender) = test_session();

        handle
            .defer_event(InlineEvent::WebmcpSubmit("deferred".into()))
            .expect("defer bridge event");
        handle.show_local_agents();
        event_sender.send(InlineEvent::Cancel).expect("send drawer event");

        let drawer_event = session.next_event().await;
        assert!(matches!(drawer_event, Some(InlineEvent::Cancel)));

        handle.hide_local_agents();
        let deferred_event = session.next_event().await;
        assert!(matches!(deferred_event, Some(InlineEvent::WebmcpSubmit(input)) if input.text == "deferred"));
    }

    #[tokio::test]
    async fn deferred_queue_overflow_fails_closed_without_dropping() {
        let (handle, _session, _event_sender) = test_session();

        for index in 0..MAX_DEFERRED_EVENTS {
            handle
                .defer_event(InlineEvent::WebmcpSubmit(format!("deferred-{index}").into()))
                .expect("queue should accept up to the bound");
        }
        assert!(handle.has_deferred_event());

        let overflow = handle.defer_event(InlineEvent::WebmcpSubmit("overflow".into()));
        assert!(overflow.is_err(), "32-event bound must fail closed, not silently drop");
        assert!(handle.has_deferred_event(), "overflow must not drain retained events");
    }
}
