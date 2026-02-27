use std::sync::Arc;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::diff::{DiffHunk, TrustMode};
use super::plan::{PlanConfirmationResult, PlanContent};
use super::selection::{
    InlineListItem, InlineListSearchConfig, InlineListSelection, SecurePromptConfig,
    WizardModalMode, WizardStep,
};
use super::style::{EditingMode, InlineHeaderContext, InlineSegment, InlineTextStyle, InlineTheme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineMessageKind {
    Agent,
    Error,
    Info,
    Policy,
    Pty,
    Tool,
    User,
    Warning,
}

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
        context: InlineHeaderContext,
    },
    SetInputStatus {
        left: Option<String>,
        right: Option<String>,
    },
    SetTheme {
        theme: InlineTheme,
    },
    SetQueuedInputs {
        entries: Vec<String>,
    },
    SetCursorVisible(bool),
    SetInputEnabled(bool),
    SetInput(String),
    ClearInput,
    ForceRedraw,
    ShowModal {
        title: String,
        lines: Vec<String>,
        secure_prompt: Option<SecurePromptConfig>,
    },
    ShowListModal {
        title: String,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
    },
    ShowWizardModal {
        title: String,
        steps: Vec<WizardStep>,
        current_step: usize,
        search: Option<InlineListSearchConfig>,
        mode: WizardModalMode,
    },
    CloseModal,
    LoadFilePalette {
        files: Vec<String>,
        workspace: std::path::PathBuf,
    },
    OpenConfigPalette,
    ClearScreen,
    SuspendEventLoop,
    ResumeEventLoop,
    ClearInputQueue,
    /// Update editing mode state in header context
    SetEditingMode(EditingMode),
    /// Update autonomous mode state in header context
    SetAutonomousMode(bool),
    /// Show plan confirmation dialog (Claude Code style HITL)
    /// Displays Implementation Blueprint and asks user to approve before execution
    ShowPlanConfirmation {
        plan: Box<PlanContent>,
    },
    ShowDiffPreview {
        file_path: String,
        before: String,
        after: String,
        hunks: Vec<DiffHunk>,
        current_hunk: usize,
    },
    SetSkipConfirmations(bool),
    Shutdown,
    /// Update reasoning stage in header context
    SetReasoningStage(Option<String>),
}

#[derive(Debug, Clone)]
pub enum InlineEvent {
    Submit(String),
    QueueSubmit(String),
    /// Edit the newest queued input (pop into input buffer)
    EditQueue,
    ListModalSubmit(InlineListSelection),
    ListModalCancel,
    WizardModalSubmit(Vec<InlineListSelection>),
    WizardModalStepComplete {
        step: usize,
        answer: InlineListSelection,
    },
    WizardModalBack {
        from_step: usize,
    },
    WizardModalCancel,
    Cancel,
    Exit,
    Interrupt,
    BackgroundOperation,
    ScrollLineUp,
    ScrollLineDown,
    ScrollPageUp,
    ScrollPageDown,
    FileSelected(String),
    LaunchEditor,
    ForceCancelPtySession,
    /// Toggle editing mode (Shift+Tab cycles through Edit -> Plan -> Edit)
    /// When agent teams are active, Shift+Tab toggles delegate mode instead.
    ToggleMode,
    /// Cycle active teammate (Shift+Up/Down when agent teams are active)
    TeamPrev,
    TeamNext,
    /// Plan confirmation result (Claude Code style HITL)
    PlanConfirmation(PlanConfirmationResult),
    /// Diff preview approval - apply edit changes
    DiffPreviewApply,
    /// Diff preview rejection - cancel edit changes
    DiffPreviewReject,
    /// Diff preview trust mode changed
    DiffPreviewTrustChanged {
        mode: TrustMode,
    },
    HistoryPrevious,
    HistoryNext,
}

pub type InlineEventCallback = Arc<dyn Fn(&InlineEvent) + Send + Sync + 'static>;

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
        self.send_command(InlineCommand::ReplaceLast { count, kind, lines });
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
        self.send_command(InlineCommand::SetHeaderContext { context });
    }

    pub fn set_input_status(&self, left: Option<String>, right: Option<String>) {
        self.send_command(InlineCommand::SetInputStatus { left, right });
    }

    pub fn set_theme(&self, theme: InlineTheme) {
        self.send_command(InlineCommand::SetTheme { theme });
    }

    pub fn set_queued_inputs(&self, entries: Vec<String>) {
        self.send_command(InlineCommand::SetQueuedInputs { entries });
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

    pub fn show_modal(
        &self,
        title: String,
        lines: Vec<String>,
        secure_prompt: Option<SecurePromptConfig>,
    ) {
        self.send_command(InlineCommand::ShowModal {
            title,
            lines,
            secure_prompt,
        });
    }

    pub fn show_list_modal(
        &self,
        title: String,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
    ) {
        self.send_command(InlineCommand::ShowListModal {
            title,
            lines,
            items,
            selected,
            search,
        });
    }

    pub fn show_wizard_modal_with_mode(
        &self,
        title: String,
        steps: Vec<WizardStep>,
        current_step: usize,
        search: Option<InlineListSearchConfig>,
        mode: WizardModalMode,
    ) {
        self.send_command(InlineCommand::ShowWizardModal {
            title,
            steps,
            current_step,
            search,
            mode,
        });
    }

    pub fn show_tabbed_list_modal(
        &self,
        title: String,
        steps: Vec<WizardStep>,
        current_step: usize,
        search: Option<InlineListSearchConfig>,
    ) {
        self.show_wizard_modal_with_mode(
            title,
            steps,
            current_step,
            search,
            WizardModalMode::TabbedList,
        );
    }

    /// Show a multi-step wizard modal with tabs for navigation
    pub fn show_wizard_modal(
        &self,
        title: String,
        steps: Vec<WizardStep>,
        search: Option<InlineListSearchConfig>,
    ) {
        self.show_wizard_modal_with_mode(title, steps, 0, search, WizardModalMode::MultiStep);
    }

    pub fn close_modal(&self) {
        self.send_command(InlineCommand::CloseModal);
    }

    pub fn clear_screen(&self) {
        self.send_command(InlineCommand::ClearScreen);
    }

    pub fn load_file_palette(&self, files: Vec<String>, workspace: std::path::PathBuf) {
        self.send_command(InlineCommand::LoadFilePalette { files, workspace });
    }

    pub fn open_config_palette(&self) {
        self.send_command(InlineCommand::OpenConfigPalette);
    }

    /// Show plan confirmation dialog (Claude Code style Implementation Blueprint)
    ///
    /// Displays the implementation plan and asks user to approve before execution.
    /// User can choose: Execute or Stay in Plan Mode (continue planning).
    pub fn show_plan_confirmation(&self, plan: PlanContent) {
        self.send_command(InlineCommand::ShowPlanConfirmation {
            plan: Box::new(plan),
        });
    }

    pub fn show_diff_preview(
        &self,
        file_path: String,
        before: String,
        after: String,
        hunks: Vec<DiffHunk>,
        current_hunk: usize,
    ) {
        let resolved_hunk = if hunks.is_empty() {
            0
        } else {
            current_hunk.min(hunks.len().saturating_sub(1))
        };
        self.send_command(InlineCommand::ShowDiffPreview {
            file_path,
            before,
            after,
            hunks,
            current_hunk: resolved_hunk,
        });
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
