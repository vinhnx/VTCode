use vtcode_core::config::types::ReasoningEffortLevel;

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
pub(crate) enum LocalServerAction {
    Interactive,
    Status { provider: Option<String> },
    Start { provider: Option<String> },
    Stop { provider: Option<String> },
    Configure { provider: Option<String> },
    Troubleshoot { provider: Option<String> },
    Provider { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CompactConversationCommand {
    Run {
        options: vtcode_core::compaction::ManualCompactionOptions,
        native_only: bool,
    },
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
    ShowMemoryConfig,
    ShowPermissions,
    ShowMemory,
    Exit,
    NewSession,
    OpenDocs,
    OpenDonateLinks,
    StartModelSelection,
    StartModePalette,
    SelectPrimaryAgent {
        name: String,
    },
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
    ContinueLatest {
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
    StartCheckupInteractive,
    RunCheckup {
        quick: bool,
    },
    Update {
        check_only: bool,
        install: bool,
        force: bool,
    },
    ManageLocalServer {
        action: LocalServerAction,
    },
    LaunchEditor {
        file: Option<String>,
    },
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
    RewindToTurn {
        turn: usize,
        scope: vtcode_core::core::agent::snapshots::RevertScope,
    },
    RewindLatest {
        scope: vtcode_core::core::agent::snapshots::RevertScope,
    },
    TogglePlanningWorkflow {
        enable: Option<bool>,
        prompt: Option<String>,
    },
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
    ManageSecrets {
        action: SecretCommandAction,
    },
    ShareLog {
        format: SessionLogExportFormat,
    },
}

/// Actions for the `/secret` API-key controller.
///
/// `/secret` (no args) and `/secret list` both render the status table of all
/// providers and where their credential comes from (env var / OS keyring /
/// OAuth / local / managed auth / none). `/secret add` and `/secret delete`
/// operate on the OS keyring entry for a single provider — they do not touch
/// environment variables (which the user controls in their shell) or the
/// workspace `.env`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SecretCommandAction {
    /// `/secret` with no args — render the status table (text mode) or open
    /// the interactive action menu (inline UI mode).
    Interactive,
    /// `/secret list` — render the status table of all providers.
    List,
    /// `/secret status [provider]` — detailed status for one provider, or all
    /// when no provider is given.
    Status { provider: Option<String> },
    /// `/secret add <provider>` (also replaces) — paste a key via the secure
    /// prompt modal and store it in the OS keyring.
    Add { provider: String },
    /// `/secret delete <provider>` — clear the keyring entry for a provider.
    Delete { provider: String },
    /// `/secret help` — usage.
    #[allow(dead_code)]
    Help,
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
