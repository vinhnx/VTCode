use std::sync::Arc;

use anstyle::{Color as AnsiColorEnum, Effects, Style as AnsiStyle};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::config::{constants::ui, types::ReasoningEffortLevel};

#[derive(Clone, Debug)]
pub struct InlineHeaderContext {
    pub provider: String,
    pub model: String,
    pub version: String,
    pub git: String,
    pub mode: String,
    pub reasoning: String,
    pub reasoning_stage: Option<String>,
    pub workspace_trust: String,
    pub tools: String,
    pub mcp: String,
    pub highlights: Vec<InlineHeaderHighlight>,
    /// Current editing mode for display in header
    pub editing_mode: EditingMode,
    /// Current autonomous mode status
    pub autonomous_mode: bool,
}

impl Default for InlineHeaderContext {
    fn default() -> Self {
        let version = env!("CARGO_PKG_VERSION").to_string();
        let git = format!(
            "{}{}",
            ui::HEADER_GIT_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        );
        let reasoning = format!(
            "{}{}",
            ui::HEADER_REASONING_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        );
        let trust = format!(
            "{}{}",
            ui::HEADER_TRUST_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        );
        let tools = format!(
            "{}{}",
            ui::HEADER_TOOLS_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        );
        let mcp = format!(
            "{}{}",
            ui::HEADER_MCP_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        );

        Self {
            provider: format!(
                "{}{}",
                ui::HEADER_PROVIDER_PREFIX,
                ui::HEADER_UNKNOWN_PLACEHOLDER
            ),
            model: format!(
                "{}{}",
                ui::HEADER_MODEL_PREFIX,
                ui::HEADER_UNKNOWN_PLACEHOLDER
            ),
            version,
            git,
            mode: ui::HEADER_MODE_INLINE.to_string(),
            reasoning,
            reasoning_stage: None,
            workspace_trust: trust,
            tools,
            mcp,
            highlights: Vec::new(),
            editing_mode: EditingMode::default(),
            autonomous_mode: false,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct InlineHeaderHighlight {
    pub title: String,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InlineTextStyle {
    pub color: Option<AnsiColorEnum>,
    pub bg_color: Option<AnsiColorEnum>,
    pub effects: Effects,
}

impl InlineTextStyle {
    #[must_use]
    pub fn with_color(mut self, color: Option<AnsiColorEnum>) -> Self {
        self.color = color;
        self
    }

    #[must_use]
    pub fn with_bg_color(mut self, color: Option<AnsiColorEnum>) -> Self {
        self.bg_color = color;
        self
    }

    #[must_use]
    pub fn merge_color(mut self, fallback: Option<AnsiColorEnum>) -> Self {
        if self.color.is_none() {
            self.color = fallback;
        }
        self
    }

    #[must_use]
    pub fn merge_bg_color(mut self, fallback: Option<AnsiColorEnum>) -> Self {
        if self.bg_color.is_none() {
            self.bg_color = fallback;
        }
        self
    }

    #[must_use]
    pub fn bold(mut self) -> Self {
        self.effects |= Effects::BOLD;
        self
    }

    #[must_use]
    pub fn italic(mut self) -> Self {
        self.effects |= Effects::ITALIC;
        self
    }

    #[must_use]
    pub fn underline(mut self) -> Self {
        self.effects |= Effects::UNDERLINE;
        self
    }

    #[must_use]
    pub fn dim(mut self) -> Self {
        self.effects |= Effects::DIMMED;
        self
    }

    #[must_use]
    pub fn to_ansi_style(&self, fallback: Option<AnsiColorEnum>) -> AnsiStyle {
        let mut style = AnsiStyle::new();
        if let Some(color) = self.color.or(fallback) {
            style = style.fg_color(Some(color));
        }
        if let Some(bg) = self.bg_color {
            style = style.bg_color(Some(bg));
        }
        // Apply effects
        if self.effects.contains(Effects::BOLD) {
            style = style.bold();
        }
        if self.effects.contains(Effects::ITALIC) {
            style = style.italic();
        }
        if self.effects.contains(Effects::UNDERLINE) {
            style = style.underline();
        }
        if self.effects.contains(Effects::DIMMED) {
            style = style.dimmed();
        }
        style
    }
}

#[derive(Clone, Debug, Default)]
pub struct InlineSegment {
    pub text: String,
    pub style: std::sync::Arc<InlineTextStyle>,
}

#[derive(Clone, Debug, Default)]
pub struct InlineTheme {
    pub foreground: Option<AnsiColorEnum>,
    pub background: Option<AnsiColorEnum>,
    pub primary: Option<AnsiColorEnum>,
    pub secondary: Option<AnsiColorEnum>,
    pub tool_accent: Option<AnsiColorEnum>,
    pub tool_body: Option<AnsiColorEnum>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineListSelection {
    Model(usize),
    DynamicModel(usize),
    RefreshDynamicModels,
    Reasoning(ReasoningEffortLevel),
    DisableReasoning,
    CustomModel,
    Theme(String),
    Session(String),
    SlashCommand(String),
    ToolApproval(bool),
    ToolApprovalDenyOnce,
    ToolApprovalSession,
    ToolApprovalPermanent,
    SessionLimitIncrease(usize),

    /// Selection shape used by legacy tabbed HITL flows.
    AskUserChoice {
        tab_id: String,
        choice_id: String,
        text: Option<String>,
    },

    /// Selection returned from the `request_user_input` HITL tool.
    RequestUserInputAnswer {
        question_id: String,
        selected: Vec<String>,
        other: Option<String>,
    },

    /// Plan confirmation dialog result (Claude Code style HITL)
    PlanApprovalExecute,
    /// Clear conversation context and auto-accept edits
    PlanApprovalClearContextAutoAccept,
    /// Return to planning to edit the plan
    PlanApprovalEditPlan,
    /// Cancel execution and stay in plan mode
    PlanApprovalCancel,
    /// Auto-accept all future plans in this session
    PlanApprovalAutoAccept,
    /// Diff preview approval - apply edit changes
    DiffPreviewApply,
    /// Diff preview rejection - cancel edit changes
    DiffPreviewReject,
    /// Diff preview trust mode changed
    DiffPreviewTrustChanged {
        mode: TrustMode,
    },
}

/// A diff hunk representing a contiguous block of changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: usize,
    pub new_start: usize,
    pub old_lines: usize,
    pub new_lines: usize,
    pub display: String,
}

impl DiffHunk {
    pub fn summary(&self) -> String {
        if self.old_lines > 0 && self.new_lines > 0 {
            format!("-{} +{}", self.old_lines, self.new_lines)
        } else if self.old_lines > 0 {
            format!("-{}", self.old_lines)
        } else if self.new_lines > 0 {
            format!("+{}", self.new_lines)
        } else {
            "No changes".to_string()
        }
    }

    pub fn old_position(&self) -> String {
        if self.old_lines == 0 {
            format!("{}", self.old_start + 1)
        } else {
            format!("{}-{}", self.old_start + 1, self.old_start + self.old_lines)
        }
    }

    pub fn new_position(&self) -> String {
        if self.new_lines == 0 {
            format!("{}", self.new_start + 1)
        } else {
            format!("{}-{}", self.new_start + 1, self.new_start + self.new_lines)
        }
    }
}

/// Trust mode for diff preview - how to handle file edit approval
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustMode {
    Once,
    Session,
    Always,
    AutoTrust,
}

impl TrustMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Once => "Once",
            Self::Session => "Session",
            Self::Always => "Always",
            Self::AutoTrust => "AutoTrust",
        }
    }
}

/// State for diff preview modal
#[derive(Debug, Clone)]
pub struct DiffPreviewState {
    pub file_path: String,
    pub before: String,
    pub after: String,
    pub hunks: Vec<DiffHunk>,
    pub current_hunk: usize,
    pub trust_mode: TrustMode,
}

impl DiffPreviewState {
    pub fn new(file_path: String, before: String, after: String, hunks: Vec<DiffHunk>) -> Self {
        Self {
            file_path,
            before,
            after,
            hunks,
            current_hunk: 0,
            trust_mode: TrustMode::Once,
        }
    }

    pub fn current_hunk_ref(&self) -> Option<&DiffHunk> {
        self.hunks.get(self.current_hunk)
    }

    pub fn hunk_count(&self) -> usize {
        self.hunks.len()
    }
}

#[derive(Clone, Debug)]
pub struct InlineListItem {
    pub title: String,
    pub subtitle: Option<String>,
    pub badge: Option<String>,
    pub indent: u8,
    pub selection: Option<InlineListSelection>,
    pub search_value: Option<String>,
}

#[derive(Clone)]
pub struct InlineListSearchConfig {
    pub label: String,
    pub placeholder: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SecurePromptConfig {
    pub label: String,

    /// Optional placeholder shown when input is empty.
    pub placeholder: Option<String>,

    /// Whether the input should be masked (e.g., API keys).
    pub mask_input: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WizardModalMode {
    /// Traditional multi-step wizard behavior (Enter advances/collects answers).
    MultiStep,
    /// Tabbed list behavior (tabs switch categories; Enter submits immediately).
    TabbedList,
}

// ============================================================================
// Plan Content (Claude Code Style Implementation Blueprint)
// ============================================================================

/// A step in an implementation plan
#[derive(Clone, Debug)]
pub struct PlanStep {
    /// Step number (1-indexed)
    pub number: usize,
    /// Short description of the step
    pub description: String,
    /// Detailed notes or context
    pub details: Option<String>,
    /// Files to be modified in this step
    pub files: Vec<String>,
    /// Whether this step is completed
    pub completed: bool,
}

/// A phase in an implementation plan (groups related steps)
#[derive(Clone, Debug)]
pub struct PlanPhase {
    /// Phase name (e.g., "Phase 1: Initial Understanding")
    pub name: String,
    /// Steps in this phase
    pub steps: Vec<PlanStep>,
    /// Whether all steps in this phase are completed
    pub completed: bool,
}

/// Structured plan content for display in Implementation Blueprint panel
#[derive(Clone, Debug)]
pub struct PlanContent {
    /// Plan title/name
    pub title: String,
    /// Summary description
    pub summary: String,
    /// Path to the plan file on disk
    pub file_path: Option<String>,
    /// Phases containing implementation steps
    pub phases: Vec<PlanPhase>,
    /// Open questions or issues
    pub open_questions: Vec<String>,
    /// Raw markdown content (for fallback display)
    pub raw_content: String,
    /// Total number of steps
    pub total_steps: usize,
    /// Number of completed steps
    pub completed_steps: usize,
}

impl PlanContent {
    /// Parse plan content from markdown
    pub fn from_markdown(title: String, content: &str, file_path: Option<String>) -> Self {
        let mut phases = Vec::new();
        let mut open_questions = Vec::new();
        let mut current_phase: Option<PlanPhase> = None;
        let mut total_steps = 0;
        let mut completed_steps = 0;
        let mut summary = String::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Extract summary from first paragraph
            if summary.is_empty() && !trimmed.is_empty() && !trimmed.starts_with('#') {
                summary = trimmed.to_string();
                continue;
            }

            // Phase headers (## Phase X: ...)
            if let Some(phase_name) = trimmed.strip_prefix("## ") {
                // Save previous phase
                if let Some(phase) = current_phase.take() {
                    phases.push(phase);
                }
                current_phase = Some(PlanPhase {
                    name: phase_name.to_string(),
                    steps: Vec::new(),
                    completed: false,
                });
                continue;
            }

            // Open questions section
            if trimmed == "## Open Questions" {
                if let Some(phase) = current_phase.take() {
                    phases.push(phase);
                }
                continue;
            }

            // Step items ([ ] or [x] prefixed)
            if let Some(rest) = trimmed.strip_prefix("[ ] ") {
                total_steps += 1;
                if let Some(ref mut phase) = current_phase {
                    phase.steps.push(PlanStep {
                        number: phase.steps.len() + 1,
                        description: rest.to_string(),
                        details: None,
                        files: Vec::new(),
                        completed: false,
                    });
                }
                continue;
            }

            if let Some(rest) = trimmed
                .strip_prefix("[x] ")
                .or_else(|| trimmed.strip_prefix("[X] "))
            {
                total_steps += 1;
                completed_steps += 1;
                if let Some(ref mut phase) = current_phase {
                    phase.steps.push(PlanStep {
                        number: phase.steps.len() + 1,
                        description: rest.to_string(),
                        details: None,
                        files: Vec::new(),
                        completed: true,
                    });
                }
                continue;
            }

            // Numbered steps (1. **Step 1** ...)
            if trimmed.starts_with(|c: char| c.is_ascii_digit()) && trimmed.contains('.') {
                total_steps += 1;
                if let Some(ref mut phase) = current_phase {
                    let desc = trimmed.split_once('.').map(|x| x.1).unwrap_or("").trim();
                    phase.steps.push(PlanStep {
                        number: phase.steps.len() + 1,
                        description: desc.to_string(),
                        details: None,
                        files: Vec::new(),
                        completed: false,
                    });
                }
                continue;
            }

            // Question items (- (...)
            if trimmed.starts_with("- (") || trimmed.starts_with("- ?") {
                open_questions.push(trimmed.trim_start_matches("- ").to_string());
            }
        }

        // Save last phase
        if let Some(mut phase) = current_phase.take() {
            phase.completed = phase.steps.iter().all(|s| s.completed);
            phases.push(phase);
        }

        // Update phase completion status
        for phase in &mut phases {
            phase.completed = !phase.steps.is_empty() && phase.steps.iter().all(|s| s.completed);
        }

        Self {
            title,
            summary,
            file_path,
            phases,
            open_questions,
            raw_content: content.to_string(),
            total_steps,
            completed_steps,
        }
    }

    /// Get progress as a percentage
    pub fn progress_percent(&self) -> u8 {
        if self.total_steps == 0 {
            0
        } else {
            ((self.completed_steps as f32 / self.total_steps as f32) * 100.0) as u8
        }
    }
}

/// A single step in a wizard modal flow
#[derive(Clone, Debug)]
pub struct WizardStep {
    /// Title displayed in the tab header
    pub title: String,
    /// Question or instruction shown above the list
    pub question: String,
    /// Selectable items for this step
    pub items: Vec<InlineListItem>,
    /// Whether this step has been completed
    pub completed: bool,
    /// The selected answer for this step (if completed)
    pub answer: Option<InlineListSelection>,

    pub allow_freeform: bool,
    pub freeform_label: Option<String>,
    pub freeform_placeholder: Option<String>,
}

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

/// Editing mode for the agent session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditingMode {
    /// Full tool access - can edit files and run commands
    #[default]
    Edit,
    /// Read-only mode - produces implementation plans without executing
    Plan,
}

impl EditingMode {
    /// Cycle to the next mode: Edit -> Plan -> Edit
    pub fn next(self) -> Self {
        match self {
            Self::Edit => Self::Plan,
            Self::Plan => Self::Edit,
        }
    }

    /// Get display name for the mode
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Edit => "Edit",
            Self::Plan => "Plan",
        }
    }
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

/// Result of plan confirmation dialog
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanConfirmationResult {
    /// Execute the plan - transition to Edit mode
    Execute,
    /// Clear conversation context and execute with auto-accept enabled
    ClearContextAutoAccept,
    /// Return to planning to edit the plan
    EditPlan,
    /// Cancel execution and stay in Plan mode
    Cancel,
    /// Auto-accept all future plans in this session
    AutoAccept,
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
