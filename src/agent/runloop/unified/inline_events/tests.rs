use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::slash_commands::ThemePaletteMode;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::{
    InlineEventContext, InlineInterruptCoordinator, InlineLoopAction, InlineQueueState, QueuedInput,
};
use crate::agent::runloop::unified::palettes::{ActivePalette, MODE_ACTION_PREFIX};
use crate::agent::runloop::unified::session_setup::{
    EditorOpenDispatcher, EditorOpenRequest, bounded_editor_open_requests,
};
use crate::agent::runloop::unified::settings_interactive::{
    ACTION_CONFIGURE_EDITOR, ACTION_PREFIX_EDIT, SettingsPaletteState,
};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::url_guard::UrlGuardPrompt;
use crate::agent::runloop::welcome::SessionBootstrap;
use tokio::sync::Notify;
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{
    AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
};
use vtcode_core::core::agent::snapshots::{DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS};
use vtcode_core::llm::provider::{self as uni, LLMRequest, LLMResponse};
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{
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

#[tokio::test]
async fn focused_exec_session_does_not_capture_slash_commands() {
    let temp_dir = tempfile::tempdir().expect("create workspace");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let response = registry
        .execute_harness_command_session(serde_json::json!({
            "action": "run",
            "command": "read line; printf received",
            "tty": false,
            "background": true,
            "stdin": true,
            "yield_time_ms": 250,
        }))
        .await
        .expect("start focused exec session");
    let session_id = response["session_id"].as_str().expect("background session id").to_string();
    let exec_sessions = registry.exec_session_manager();
    exec_sessions
        .focus_background_session(&session_id)
        .await
        .expect("focus background session");

    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    context.set_exec_session_manager(exec_sessions.clone());
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::Submit("/jobs".into()), &mut queue)
        .await
        .expect("process slash command");
    assert!(matches!(action, InlineLoopAction::Submit(input) if input.text == "/jobs"));

    context
        .process_event(
            InlineEvent::ExecSessionAction {
                id: session_id.clone(),
                action: vtcode_ui::tui::app::ExecSessionAction::Focus,
            },
            &mut queue,
        )
        .await
        .expect("toggle focused exec session");
    assert!(exec_sessions.focused_session_id().is_none());

    drop(context);
    registry
        .close_harness_exec_session(&session_id)
        .await
        .expect("close focused exec session");
}

#[tokio::test]
async fn launch_editor_event_opens_editor_directly() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::LaunchEditor { draft: "".to_string() }, &mut queue)
        .await
        .expect("process launch editor");
    assert!(matches!(
        action,
        InlineLoopAction::LaunchEditorWithDraft { ref draft } if draft.is_empty()
    ));
}

#[tokio::test]
async fn launch_editor_event_with_draft_returns_editor_with_draft_action() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::LaunchEditor { draft: "hello world".to_string() }, &mut queue)
        .await
        .expect("process launch editor with draft");
    assert!(matches!(
        action,
        InlineLoopAction::LaunchEditorWithDraft { ref draft } if draft == "hello world"
    ));
}

#[tokio::test]
async fn open_file_in_editor_event_emits_out_of_band_request_with_path() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let (editor_sender, mut editor_requests) = bounded_editor_open_requests();
    context.set_editor_open_sink(editor_sender, Arc::new(EditorOpenDispatcher::new(true)));
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
    let path = "/tmp/demo.rs".to_string();

    let action = context
        .process_event(InlineEvent::OpenFileInEditor(path.clone()), &mut queue)
        .await
        .expect("process open file in editor");
    assert!(matches!(action, InlineLoopAction::Continue));
    assert_eq!(
        editor_requests.recv().await,
        Some(EditorOpenRequest::from_raw_target(&path, &config.workspace).expect("valid editor target"))
    );
    assert!(queued_inputs.is_empty());
}

#[tokio::test]
async fn open_file_drain_skips_event_instance_forwarded_by_callback() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let workspace = config.workspace.clone();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let (editor_sender, mut editor_requests) = bounded_editor_open_requests();
    let dispatcher = Arc::new(EditorOpenDispatcher::new(true));
    dispatcher.set_sender(editor_sender.clone());
    context.set_editor_open_sink(editor_sender, dispatcher.clone());
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
    let path = "/tmp/demo.rs".to_string();

    // Simulate the TUI event callback firing first (immediate mid-turn
    // open), then the same event instance arriving via the main channel
    // drain after the turn.
    assert!(dispatcher.try_forward_immediate(&path, &workspace));
    let action = context
        .process_event(InlineEvent::OpenFileInEditor(path.clone()), &mut queue)
        .await
        .expect("process open file in editor");
    assert!(matches!(action, InlineLoopAction::Continue));
    assert_eq!(
        editor_requests.recv().await,
        Some(EditorOpenRequest::from_raw_target(&path, &workspace).expect("valid editor target"))
    );
    assert!(editor_requests.try_recv().is_err());
}

#[tokio::test]
async fn open_url_event_shows_guard_modal_with_deny_selected_by_default() {
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let url = "https://example.com/docs".to_string();
    {
        let mut history = Vec::<uni::Message>::new();
        let mut session_stats = SessionStats::default();
        let mut context_manager = ContextManager::default_for_test();
        let mut context = InlineEventContext::new(
            &mut renderer,
            &handle,
            interrupts,
            &mut ctrl_c_notice_displayed,
            &mut header_context,
            &mut model_picker_state,
            &mut palette_state,
            &mut config,
            &mut vt_cfg,
            &mut provider_client,
            &ctrl_c_state,
            &ctrl_c_notify,
            &session_bootstrap,
            false,
            &mut history,
            &mut session_stats,
            &mut context_manager,
            "test-session",
            "test-thread",
            None,
            None,
        );
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        let action = context
            .process_event(InlineEvent::OpenUrl(url.clone()), &mut queue)
            .await
            .expect("process open url");
        assert!(matches!(action, InlineLoopAction::Continue));
    }

    let prompt = UrlGuardPrompt::parse(url.clone()).expect("parse url guard prompt");
    let command = commands.recv().await.expect("guard overlay command");
    match command {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::List(request) => {
                assert_eq!(request.title, "Open External Link");
                assert_eq!(request.selected, Some(prompt.default_selection()));
                assert_eq!(request.lines, prompt.lines());
                let titles: Vec<_> = request.items.iter().map(|item| item.title.as_str()).collect();
                assert_eq!(titles, vec!["Cancel", "Open in browser"]);
            }
            other => panic!("expected list transient, got {other:?}"),
        },
        other => {
            panic!("expected transient command, got different command: {:?}", other_name(&other))
        }
    }

    assert!(matches!(palette_state, Some(ActivePalette::UrlGuard { .. })));
}

#[tokio::test]
async fn open_http_url_guard_modal_includes_insecure_transport_warning() {
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    {
        let mut history = Vec::<uni::Message>::new();
        let mut session_stats = SessionStats::default();
        let mut context_manager = ContextManager::default_for_test();
        let mut context = InlineEventContext::new(
            &mut renderer,
            &handle,
            interrupts,
            &mut ctrl_c_notice_displayed,
            &mut header_context,
            &mut model_picker_state,
            &mut palette_state,
            &mut config,
            &mut vt_cfg,
            &mut provider_client,
            &ctrl_c_state,
            &ctrl_c_notify,
            &session_bootstrap,
            false,
            &mut history,
            &mut session_stats,
            &mut context_manager,
            "test-session",
            "test-thread",
            None,
            None,
        );
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        let action = context
            .process_event(InlineEvent::OpenUrl("http://example.com/docs".to_string()), &mut queue)
            .await
            .expect("process insecure open url");
        assert!(matches!(action, InlineLoopAction::Continue));
    }

    let command = commands.recv().await.expect("guard overlay command");
    match command {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::List(request) => {
                assert!(request.lines.iter().any(|line| line.contains("Plain HTTP")));
            }
            other => panic!("expected list transient, got {other:?}"),
        },
        other => {
            panic!("expected transient command, got different command: {:?}", other_name(&other))
        }
    }
}

#[tokio::test]
async fn cancelling_url_guard_restores_previous_palette() {
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::Theme {
        mode: ThemePaletteMode::Select,
        original_theme_id: theme::DEFAULT_THEME_ID.to_string(),
    });
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    {
        let mut history = Vec::<uni::Message>::new();
        let mut session_stats = SessionStats::default();
        let mut context_manager = ContextManager::default_for_test();
        let mut context = InlineEventContext::new(
            &mut renderer,
            &handle,
            interrupts,
            &mut ctrl_c_notice_displayed,
            &mut header_context,
            &mut model_picker_state,
            &mut palette_state,
            &mut config,
            &mut vt_cfg,
            &mut provider_client,
            &ctrl_c_state,
            &ctrl_c_notify,
            &session_bootstrap,
            false,
            &mut history,
            &mut session_stats,
            &mut context_manager,
            "test-session",
            "test-thread",
            None,
            None,
        );
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        let action = context
            .process_event(InlineEvent::OpenUrl("https://example.com/docs".to_string()), &mut queue)
            .await
            .expect("process open url");
        assert!(matches!(action, InlineLoopAction::Continue));

        let cancel = context
            .process_event(InlineEvent::Transient(TransientEvent::Cancelled), &mut queue)
            .await
            .expect("process cancel");
        assert!(matches!(cancel, InlineLoopAction::Continue));
    }

    let initial_command = commands.recv().await.expect("url guard command");
    match initial_command {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::List(request) => assert_eq!(request.title, "Open External Link"),
            other => panic!("expected list transient, got {other:?}"),
        },
        other => {
            panic!("expected transient command, got different command: {:?}", other_name(&other))
        }
    }

    let restored_command = commands.recv().await.expect("restored palette command");
    match restored_command {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::List(request) => assert_eq!(request.title, "Theme"),
            other => panic!("expected list transient, got {other:?}"),
        },
        other => {
            panic!("expected transient command, got different command: {:?}", other_name(&other))
        }
    }

    assert!(matches!(palette_state, Some(ActivePalette::Theme { mode: ThemePaletteMode::Select, .. })));
}

#[tokio::test]
async fn previous_agent_event_cycles_primary_agent_backward() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::CyclePrimaryAgentPrevious, &mut queue)
        .await
        .expect("process previous primary agent");
    assert!(matches!(action, InlineLoopAction::CyclePrimaryAgentPrevious));
}

#[tokio::test]
async fn settings_editor_selection_submits_editor_config_command() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::Settings {
        state: Box::new(SettingsPaletteState {
            workspace: std::path::PathBuf::from("."),
            source_path: std::path::PathBuf::from("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("tools".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        }),
        esc_armed: false,
    });
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::ConfigAction(ACTION_CONFIGURE_EDITOR.to_string()),
            ))),
            &mut queue,
        )
        .await
        .expect("process configure editor selection");

    assert!(matches!(
        action,
        InlineLoopAction::Submit(ref command) if command.text == "/config tools.editor"
    ));
    assert!(palette_state.is_none());
}

#[tokio::test]
async fn settings_string_selection_opens_value_editor() {
    let temp = tempfile::tempdir().expect("temp workspace");
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::Settings {
        state: Box::new(SettingsPaletteState {
            workspace: temp.path().to_path_buf(),
            source_path: temp.path().join("vtcode.toml"),
            source_label: None,
            draft: VTCodeConfig::default(),
            view_path: Some("tools.editor".to_string()),
            last_selection: None,
            selection_by_view: BTreeMap::new(),
            pending_edit_path: None,
            status: None,
        }),
        esc_armed: false,
    });
    let mut config = runtime_config();
    config.workspace = temp.path().to_path_buf();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::ConfigAction(format!("{ACTION_PREFIX_EDIT}tools.editor.preferred_editor")),
            ))),
            &mut queue,
        )
        .await
        .expect("open string settings editor");
    assert!(matches!(action, InlineLoopAction::Continue));

    match commands.recv().await.expect("settings editor wizard command") {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::Wizard(request) => {
                assert_eq!(request.title, "Edit setting");
                assert_eq!(request.steps.len(), 1);
                assert_eq!(request.steps[0].freeform_default.as_deref(), Some(""));
            }
            other => panic!("expected settings editor wizard, got {other:?}"),
        },
        other => panic!("expected settings editor transient, got {}", other_name(&other)),
    }

    let action = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Wizard(vec![
                InlineListSelection::RequestUserInputAnswer {
                    question_id: "settings_value".to_string(),
                    selected: Vec::new(),
                    other: Some("code --wait".to_string()),
                },
            ]))),
            &mut queue,
        )
        .await
        .expect("apply string settings edit");
    assert!(matches!(action, InlineLoopAction::Continue));
    assert!(
        std::iter::from_fn(|| commands.try_recv().ok())
            .any(|command| matches!(command, InlineCommand::ShowTransient { .. }))
    );
    assert!(matches!(
        palette_state,
        Some(ActivePalette::Settings { state, .. })
            if state.pending_edit_path.is_none() && state.draft.tools.editor.preferred_editor == "code --wait"
    ));
}

#[tokio::test]
async fn mode_palette_selection_selects_primary_agent() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::Mode);
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::ConfigAction(format!("{MODE_ACTION_PREFIX}auto")),
            ))),
            &mut queue,
        )
        .await
        .expect("process mode palette selection");

    assert!(matches!(
        action,
        InlineLoopAction::SelectPrimaryAgent { name: Some(ref name) } if name == "auto"
    ));
    assert!(palette_state.is_none());
}

#[tokio::test]
async fn plan_confirmation_events_map_to_expected_actions() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let execute = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::PlanApprovalExecute,
            ))),
            &mut queue,
        )
        .await
        .expect("process execute");
    let fresh = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::PlanApprovalFreshContext,
            ))),
            &mut queue,
        )
        .await
        .expect("process auto");
    let edit = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::PlanApprovalEditPlan,
            ))),
            &mut queue,
        )
        .await
        .expect("process edit plan");
    let cancel = context
        .process_event(InlineEvent::Transient(TransientEvent::Cancelled), &mut queue)
        .await
        .expect("process cancel");

    assert!(matches!(
        execute,
        InlineLoopAction::PlanApproved {
            target: crate::agent::runloop::unified::planning_workflow::PlanExecutionTarget {
                destination: crate::agent::runloop::unified::planning_workflow::PlanExecutionDestination::Build,
                execution_context: crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Current,
                skip_confirmations: false,
            }
        }
    ));
    assert!(matches!(
        fresh,
        InlineLoopAction::PlanApproved {
            target: crate::agent::runloop::unified::planning_workflow::PlanExecutionTarget {
                destination: crate::agent::runloop::unified::planning_workflow::PlanExecutionDestination::Build,
                execution_context: crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Fresh,
                skip_confirmations: false,
            }
        }
    ));
    assert!(matches!(edit, InlineLoopAction::PlanEditRequested));
    assert!(matches!(cancel, InlineLoopAction::Continue));
}

#[tokio::test]
async fn single_ctrl_c_returns_continue_from_tui() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    // A TUI interrupt uses the cancellation-only local path.
    // handle_interrupt() returns Continue (cancel, not exit).
    let action = context
        .process_event(InlineEvent::Interrupt, &mut queue)
        .await
        .expect("process interrupt");
    assert!(matches!(action, InlineLoopAction::Continue));
    assert!(ctrl_c_state.is_cancel_requested());
}

#[tokio::test]
async fn repeated_tui_interrupts_only_cancel_the_current_turn() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let first_action = context
        .process_event(InlineEvent::Interrupt, &mut queue)
        .await
        .expect("process first interrupt");
    let second_action = context
        .process_event(InlineEvent::Interrupt, &mut queue)
        .await
        .expect("process second interrupt");

    assert!(matches!(first_action, InlineLoopAction::Continue));
    assert!(matches!(second_action, InlineLoopAction::Continue));
    assert!(ctrl_c_state.is_cancel_requested());
    assert!(!ctrl_c_state.is_exit_requested());
}

#[tokio::test]
async fn steering_events_are_passive_in_idle_loop() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;

    {
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
        for event in [
            InlineEvent::Pause,
            InlineEvent::Resume,
            InlineEvent::Steer("keep going".into()),
        ] {
            let action = context.process_event(event, &mut queue).await.expect("process steering event");
            assert!(matches!(action, InlineLoopAction::Continue));
        }
    }

    // Undelivered steers fall through to the durable queue so the message is
    // processed once the agent is ready instead of vanishing.
    assert_eq!(queued_inputs.len(), 1);
    assert_eq!(queued_inputs.front().map(|q| q.input.text.as_str()), Some("keep going"));
}

#[tokio::test]
async fn steered_input_already_delivered_is_not_queued_twice() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;

    {
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
        // Simulate the UI callback accepting this steer on the live channel.
        ctrl_c_state.mark_steer_delivered();
        let action = context
            .process_event(InlineEvent::Steer("already steered".into()), &mut queue)
            .await
            .expect("process delivered steer");
        assert!(matches!(action, InlineLoopAction::Continue));

        // A subsequent undelivered steer must still queue.
        let action = context
            .process_event(InlineEvent::Steer("needs queue".into()), &mut queue)
            .await
            .expect("process undelivered steer");
        assert!(matches!(action, InlineLoopAction::Continue));
    }
    // Only the undelivered steer landed in the queue.
    assert_eq!(queued_inputs.len(), 1);
    assert_eq!(queued_inputs.front().map(|q| q.input.text.as_str()), Some("needs queue"));
}

#[tokio::test]
async fn rapid_delivered_steers_are_not_double_queued() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;

    {
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
        // Both steers are delivered to the live channel before the runloop
        // drains either event — the common rapid-influx ordering.
        ctrl_c_state.mark_steer_delivered();
        ctrl_c_state.mark_steer_delivered();
        for text in ["steer one", "steer two"] {
            let action = context
                .process_event(InlineEvent::Steer(text.into()), &mut queue)
                .await
                .expect("process delivered steer");
            assert!(matches!(action, InlineLoopAction::Continue));
        }
    }
    assert!(
        queued_inputs.is_empty(),
        "burst of delivered steers must not fall through to the queue, got {queued_inputs:?}"
    );
}

#[tokio::test]
async fn steer_with_attachments_restores_full_draft_and_continues() {
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let first_attachment = ContentPart::image("first-image", "image/png");
    let second_attachment = ContentPart::image("second-image", "image/png");
    let input = SubmittedInput::new("keep going", vec![first_attachment.clone(), second_attachment.clone()]);
    let action = context
        .process_event(InlineEvent::Steer(input.clone()), &mut queue)
        .await
        .expect("process steer with attachment");

    assert!(matches!(action, InlineLoopAction::Continue));
    let mut restored_draft = false;
    let mut warning_rendered = false;
    while let Ok(command) = commands.try_recv() {
        if let InlineCommand::AppendLine { segments, .. } = &command
            && segments.iter().any(|segment| {
                segment
                    .text
                    .contains("Live steering supports text only. Remove image attachments before steering.")
            })
        {
            warning_rendered = true;
        }
        if let InlineCommand::RestoreInputDraft(restored_input) = command {
            assert_eq!(restored_input.text, "keep going");
            assert_eq!(restored_input.attachments, vec![first_attachment.clone(), second_attachment.clone()]);
            assert_eq!(restored_input, input);
            restored_draft = true;
        }
    }
    assert!(warning_rendered);
    assert!(restored_draft);
}

#[tokio::test]
async fn process_latest_queued_event_primes_newest_queue_priority() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::from([
        QueuedInput::new("first".into(), Some("duck".to_string())),
        QueuedInput::new("latest".into(), Some("builder".to_string())),
    ]);
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::ProcessLatestQueued, &mut queue)
        .await
        .expect("process latest queued");

    assert!(matches!(action, InlineLoopAction::Continue));
    assert!(prefer_latest_once);
}

#[tokio::test]
async fn inline_prompt_suggestion_event_maps_to_inline_action() {
    let (handle, mut renderer) = renderer_with_handle();
    let (ctrl_c_state, ctrl_c_notify) = ctrl_c_handles();
    let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_ui::tui::app::InlineHeaderContext::default();
    let mut history = Vec::<uni::Message>::new();
    let mut session_stats = SessionStats::default();
    let mut context_manager = ContextManager::default_for_test();
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut header_context,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &ctrl_c_state,
        &ctrl_c_notify,
        &session_bootstrap,
        false,
        &mut history,
        &mut session_stats,
        &mut context_manager,
        "test-session",
        "test-thread",
        None,
        None,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::RequestInlinePromptSuggestion("Review the current".to_string()), &mut queue)
        .await
        .expect("process inline prompt suggestion request");

    assert!(matches!(
        action,
        InlineLoopAction::RequestInlinePromptSuggestion(ref draft)
            if draft == "Review the current"
    ));
}

fn other_name(command: &InlineCommand) -> &'static str {
    match command {
        InlineCommand::ShowTransient { .. } => "ShowTransient",
        InlineCommand::AppendLine { .. } => "AppendLine",
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
