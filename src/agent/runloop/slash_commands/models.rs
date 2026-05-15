use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::llm::provider::ResponsesCompactionOptions;
use vtcode_core::scheduler::{LoopCommand, ScheduleCreateInput};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThemePaletteMode {
    Select,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionPaletteMode {
    Resume,
    Fork,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatuslineTargetMode {
    User,
    Workspace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionModeCommand {
    Edit,
    Auto,
    Plan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OAuthProviderAction {
    Login,
    Logout,
    Refresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionLogExportFormat {
    Both,
    Json,
    Markdown,
    Html,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentDefinitionScope {
    Project,
    User,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AgentManagerAction {
    List,
    Threads,
    Inspect {
        id: String,
    },
    Close {
        id: String,
    },
    Create {
        scope: Option<AgentDefinitionScope>,
        name: Option<String>,
    },
    Edit {
        name: Option<String>,
    },
    Delete {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SubprocessManagerAction {
    List,
    ToggleDefault,
    Refresh,
    Inspect { id: String },
    Stop { id: String },
    Cancel { id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ScheduleCommandAction {
    Interactive,
    Browse,
    CreateInteractive,
    Create { input: ScheduleCreateInput },
    DeleteInteractive,
    Delete { id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CompactConversationCommand {
    Run { options: ResponsesCompactionOptions },
    EditDefaultPrompt,
    ResetDefaultPrompt,
}

pub(crate) enum SlashCommandOutcome {
    Handled,
    ThemeChanged(String),
    InitializeWorkspace {
        force: bool,
    },
    ShowSettings,
    ShowSettingsAtPath {
        path: String,
    },
    ShowHooks,
    ShowMemoryConfig,
    ShowPermissions,
    ShowMemory,
    Exit,
    NewSession,
    OpenDocs,
    StartModelSelection,
    SetEffort {
        level: Option<ReasoningEffortLevel>,
        persist: bool,
    },
    ToggleIdeContext,
    StartThemePalette {
        mode: ThemePaletteMode,
    },
    StartSessionPalette {
        mode: SessionPaletteMode,
        limit: usize,
        show_all: bool,
    },
    StartHistoryPicker,
    StartFileBrowser {
        initial_filter: Option<String>,
    },
    StartStatuslineSetup {
        instructions: Option<String>,
    },
    StartTerminalTitleSetup,
    ClearScreen,
    ClearConversation,
    CompactConversation {
        command: CompactConversationCommand,
    },
    CopyLatestAssistantReply,
    TriggerPromptSuggestions,
    ToggleTasksPanel,
    ShowJobsPanel,
    ShowStatus,
    Notify {
        message: String,
    },
    StopAgent,
    ManageMcp {
        action: McpCommandAction,
    },
    StartDoctorInteractive,
    RunDoctor {
        quick: bool,
    },
    Update {
        check_only: bool,
        install: bool,
        force: bool,
    },
    ManageLoop {
        command: LoopCommand,
    },
    ManageSchedule {
        action: ScheduleCommandAction,
    },
    LaunchEditor {
        file: Option<String>,
    },
    LaunchGit,
    ManageSkills {
        action: crate::agent::runloop::SkillCommandAction,
    },
    ManageAgents {
        action: AgentManagerAction,
    },
    ManageSubprocesses {
        action: SubprocessManagerAction,
    },
    ReplaceInput {
        content: String,
    },
    SubmitPrompt {
        prompt: String,
    },
    StartTerminalSetup,
    OpenRewindPicker,
    RewindToTurn {
        turn: usize,
        scope: vtcode_core::core::agent::snapshots::RevertScope,
    },
    RewindLatest {
        scope: vtcode_core::core::agent::snapshots::RevertScope,
    },
    TogglePlanMode {
        enable: Option<bool>,
        prompt: Option<String>,
    },
    StartModeSelection,
    SetMode {
        mode: SessionModeCommand,
    },
    CycleMode,
    OAuthLogin {
        provider: String,
    },
    StartOAuthProviderPicker {
        action: OAuthProviderAction,
    },
    OAuthLogout {
        provider: String,
    },
    RefreshOAuth {
        provider: String,
    },
    ShowAuthStatus {
        provider: Option<String>,
    },
    ShareLog {
        format: SessionLogExportFormat,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum McpCommandAction {
    Interactive,
    Overview,
    ListProviders,
    ListTools,
    RefreshTools,
    ShowConfig,
    EditConfig,
    Repair,
    Diagnose,
    Login(String),
    Logout(String),
}
