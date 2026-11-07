pub(super) use std::collections::{BTreeMap, VecDeque};
pub(super) use std::sync::Arc;

pub(super) use crate::agent::runloop::model_picker::ModelPickerState;
pub(super) use crate::agent::runloop::slash_commands::ThemePaletteMode;
pub(super) use crate::agent::runloop::unified::context_manager::ContextManager;
pub(super) use crate::agent::runloop::unified::inline_events::{
    InlineEventContext, InlineInterruptCoordinator, InlineLoopAction, InlineQueueState, QueuedInput,
};
pub(super) use crate::agent::runloop::unified::palettes::{ActivePalette, MODE_ACTION_PREFIX};
pub(super) use crate::agent::runloop::unified::session_setup::{
    EditorOpenDispatcher, EditorOpenRequest, bounded_editor_open_requests,
};
pub(super) use crate::agent::runloop::unified::settings_interactive::{
    ACTION_CONFIGURE_EDITOR, ACTION_PREFIX_EDIT, SettingsPaletteState,
};
pub(super) use crate::agent::runloop::unified::state::CtrlCState;
pub(super) use crate::agent::runloop::unified::state::SessionStats;
pub(super) use crate::agent::runloop::unified::url_guard::UrlGuardPrompt;
pub(super) use crate::agent::runloop::welcome::SessionBootstrap;
pub(super) use tokio::sync::Notify;
pub(super) use vtcode_core::config::core::PromptCachingConfig;
pub(super) use vtcode_core::config::loader::VTCodeConfig;
pub(super) use vtcode_core::config::models::Provider;
pub(super) use vtcode_core::config::types::{
    AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
};
pub(super) use vtcode_core::core::agent::snapshots::{
    DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
};
pub(super) use vtcode_core::llm::provider::{self as uni, LLMRequest, LLMResponse};
pub(super) use vtcode_core::tools::registry::ToolRegistry;
pub(super) use vtcode_core::ui::theme;
pub(super) use vtcode_core::utils::ansi::AnsiRenderer;
pub(super) use vtcode_ui::tui::app::{
    ContentPart, InlineCommand, InlineEvent, InlineHandle, InlineListSelection, SubmittedInput, TransientEvent,
    TransientRequest, TransientSubmission,
};

#[derive(Clone)]
struct DummyProvider;

#[async_trait::async_trait]
impl uni::LLMProvider for DummyProvider {
    fn name(&self) -> &str {
        "dummy"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, uni::LLMError> {
        Ok(LLMResponse {
            content: None,
            model: "dummy-model".to_string(),
            tool_calls: None,
            usage: None,
            finish_reason: uni::FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: vec![],
            compaction: None,
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["dummy-model".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

fn runtime_config() -> CoreAgentConfig {
    CoreAgentConfig {
        model: vtcode_core::config::constants::models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        api_key: "test-key".to_string(),
        provider: "gemini".to_string(),
        api_key_env: Provider::Gemini.default_api_key_env().to_string(),
        workspace: std::env::current_dir().expect("current_dir"),
        verbose: false,
        quiet: false,
        theme: theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    }
}

fn renderer_with_handle() -> (InlineHandle, AnsiRenderer) {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    (handle, renderer)
}

fn renderer_with_handle_and_commands()
-> (InlineHandle, tokio::sync::mpsc::UnboundedReceiver<InlineCommand>, AnsiRenderer) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    (handle, rx, renderer)
}

fn ctrl_c_handles() -> (Arc<CtrlCState>, Arc<Notify>) {
    (Arc::new(CtrlCState::new()), Arc::new(Notify::new()))
}

fn other_name(command: &InlineCommand) -> &'static str {
    match command {
        InlineCommand::Diagnostic(_) => "Diagnostic",
        InlineCommand::ShowTransient { .. } => "ShowTransient",
        InlineCommand::AppendLine { .. } => "AppendLine",
        InlineCommand::AppendLines { .. } => "AppendLines",
        InlineCommand::AppendPastedMessage { .. } => "AppendPastedMessage",
        InlineCommand::Inline { .. } => "Inline",
        InlineCommand::ReplaceLast { .. } => "ReplaceLast",
        InlineCommand::RecordToolOutput { .. } => "RecordToolOutput",
        InlineCommand::FocusTranscriptReview { .. } => "FocusTranscriptReview",
        InlineCommand::AppendToolOutputLine { .. } => "AppendToolOutputLine",
        InlineCommand::AppendCompactActivity(_) => "AppendCompactActivity",
        InlineCommand::RecordDiffReview(_) => "RecordDiffReview",
        InlineCommand::ReplaceCompactActivity(_) => "ReplaceCompactActivity",
        InlineCommand::CollapsePtyBlock(_) => "CollapsePtyBlock",
        InlineCommand::SetKeyBindings { .. } => "SetKeyBindings",
        InlineCommand::SetPrompt { .. } => "SetPrompt",
        InlineCommand::SetPlaceholder { .. } => "SetPlaceholder",
        InlineCommand::SetMessageLabels { .. } => "SetMessageLabels",
        InlineCommand::SetHeaderContext { .. } => "SetHeaderContext",
        InlineCommand::SetInputStatus { .. } => "SetInputStatus",
        InlineCommand::SetConfiguredInputStatus { .. } => "SetConfiguredInputStatus",
        InlineCommand::ProgramStatus(_) => "ProgramStatus",
        InlineCommand::SetActivityState(_) => "SetActivityState",
        InlineCommand::UpdateProgress(_) => "UpdateProgress",
        InlineCommand::SetTerminalTitleItems { .. } => "SetTerminalTitleItems",
        InlineCommand::SetTerminalTitleThreadLabel { .. } => "SetTerminalTitleThreadLabel",
        InlineCommand::SetTerminalTitleGitBranch { .. } => "SetTerminalTitleGitBranch",
        InlineCommand::SetTheme { .. } => "SetTheme",
        InlineCommand::SetColorSchemeAuto { .. } => "SetColorSchemeAuto",
        InlineCommand::SetAppearance { .. } => "SetAppearance",
        InlineCommand::SetVimModeEnabled(_) => "SetVimModeEnabled",
        InlineCommand::SetQueuedInputs { .. } => "SetQueuedInputs",
        InlineCommand::SetSubprocessEntries { .. } => "SetSubprocessEntries",
        InlineCommand::SetSubagentPreview { .. } => "SetSubagentPreview",
        InlineCommand::SetLocalAgents { .. } => "SetLocalAgents",
        InlineCommand::SetActivePlan { .. } => "SetActivePlan",
        InlineCommand::SetPrimaryAgent { .. } => "SetPrimaryAgent",
        InlineCommand::SetArchivedHistory { .. } => "SetArchivedHistory",
        InlineCommand::SetCursorVisible(_) => "SetCursorVisible",
        InlineCommand::SetInputEnabled(_) => "SetInputEnabled",
        InlineCommand::SetImageInputEnabled(_) => "SetImageInputEnabled",
        InlineCommand::SetInput(_) => "SetInput",
        InlineCommand::RestoreInputDraft(_) => "RestoreInputDraft",
        InlineCommand::ApplySuggestedPrompt(_) => "ApplySuggestedPrompt",
        InlineCommand::SetInlinePromptSuggestion { .. } => "SetInlinePromptSuggestion",
        InlineCommand::ClearInlinePromptSuggestion => "ClearInlinePromptSuggestion",
        InlineCommand::ClearInput => "ClearInput",
        InlineCommand::ForceRedraw => "ForceRedraw",
        InlineCommand::CloseTransient => "CloseTransient",
        InlineCommand::ClearScreen => "ClearScreen",
        InlineCommand::SuspendEventLoop => "SuspendEventLoop",
        InlineCommand::ResumeEventLoop => "ResumeEventLoop",
        InlineCommand::ClearInputQueue => "ClearInputQueue",
        InlineCommand::SetSkipConfirmations(_) => "SetSkipConfirmations",
        InlineCommand::Shutdown => "Shutdown",
        InlineCommand::SetReasoningStage(_) => "SetReasoningStage",
        InlineCommand::StopEventStream => "StopEventStream",
        InlineCommand::StartEventStream => "StartEventStream",
        InlineCommand::UpdateFilePaletteSearch { .. } => "UpdateFilePaletteSearch",
        InlineCommand::SetSlashCommands { .. } => "SetSlashCommands",
        InlineCommand::SetFullscreenInteraction { .. } => "SetFullscreenInteraction",
    }
}

mod agent_settings;
mod open_guards;
mod plan_interrupt;
mod prompt_misc;
mod session_editor;
mod steering;
