use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::unified::inline_events::{
    InlineEventContext, InlineInterruptCoordinator, InlineLoopAction, InlineQueueState,
};
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::settings_interactive::{
    ACTION_CONFIGURE_EDITOR, SettingsPaletteState,
};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::welcome::SessionBootstrap;
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{
    AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
};
use vtcode_core::core::agent::snapshots::{
    DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
};
use vtcode_core::llm::provider::{self as uni, LLMRequest, LLMResponse};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::app::{
    InlineCommand, InlineEvent, InlineHandle, InlineListSelection, TransientEvent,
    TransientRequest, TransientSubmission,
};

use super::{URL_GUARD_DENY_ACTION, UrlGuardPrompt};

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
        theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
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

fn renderer_with_handle_and_commands() -> (
    InlineHandle,
    tokio::sync::mpsc::UnboundedReceiver<InlineCommand>,
    AnsiRenderer,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    (handle, rx, renderer)
}

#[tokio::test]
async fn launch_editor_event_submits_edit_command() {
    let (handle, mut renderer) = renderer_with_handle();
    let ctrl_c_state = CtrlCState::new();
    let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_tui::app::InlineHeaderContext::default();
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
        &session_bootstrap,
        false,
        0,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::LaunchEditor, &mut queue)
        .await
        .expect("process launch editor");
    assert!(matches!(
        action,
        InlineLoopAction::Submit(ref command) if command == "/edit"
    ));
}

#[tokio::test]
async fn open_file_in_editor_event_submits_edit_command_with_path() {
    let (handle, mut renderer) = renderer_with_handle();
    let ctrl_c_state = CtrlCState::new();
    let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_tui::app::InlineHeaderContext::default();
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
        &session_bootstrap,
        false,
        0,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);
    let path = "/tmp/demo.rs".to_string();

    let action = context
        .process_event(InlineEvent::OpenFileInEditor(path.clone()), &mut queue)
        .await
        .expect("process open file in editor");
    assert!(matches!(
        action,
        InlineLoopAction::Submit(ref command) if command == &format!("/edit {}", path)
    ));
}

#[tokio::test]
async fn open_url_event_shows_guard_modal_with_deny_selected_by_default() {
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let ctrl_c_state = CtrlCState::new();
    let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_tui::app::InlineHeaderContext::default();
    let url = "https://example.com/docs".to_string();
    {
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
            &session_bootstrap,
            false,
            0,
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
                assert_eq!(
                    request.selected,
                    Some(InlineListSelection::ConfigAction(
                        URL_GUARD_DENY_ACTION.to_string()
                    ))
                );
                assert_eq!(request.lines, prompt.lines());
                let titles: Vec<_> = request
                    .items
                    .iter()
                    .map(|item| item.title.as_str())
                    .collect();
                assert_eq!(titles, vec!["Cancel", "Open in browser"]);
            }
            other => panic!("expected list transient, got {other:?}"),
        },
        other => panic!(
            "expected transient command, got different command: {:?}",
            other_name(&other)
        ),
    }

    assert!(matches!(
        palette_state,
        Some(ActivePalette::UrlGuard { .. })
    ));
}

#[tokio::test]
async fn open_http_url_guard_modal_includes_insecure_transport_warning() {
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let ctrl_c_state = CtrlCState::new();
    let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_tui::app::InlineHeaderContext::default();
    {
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
            &session_bootstrap,
            false,
            0,
        );
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        let action = context
            .process_event(
                InlineEvent::OpenUrl("http://example.com/docs".to_string()),
                &mut queue,
            )
            .await
            .expect("process insecure open url");
        assert!(matches!(action, InlineLoopAction::Continue));
    }

    let command = commands.recv().await.expect("guard overlay command");
    match command {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::List(request) => {
                assert!(
                    request
                        .lines
                        .iter()
                        .any(|line| line.contains("Plain HTTP is insecure"))
                );
            }
            other => panic!("expected list transient, got {other:?}"),
        },
        other => panic!(
            "expected transient command, got different command: {:?}",
            other_name(&other)
        ),
    }
}

#[tokio::test]
async fn cancelling_url_guard_restores_previous_palette() {
    let (handle, mut commands, mut renderer) = renderer_with_handle_and_commands();
    let ctrl_c_state = CtrlCState::new();
    let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::ModelTarget);
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_tui::app::InlineHeaderContext::default();
    {
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
            &session_bootstrap,
            false,
            0,
        );
        let mut queued_inputs = VecDeque::new();
        let mut prefer_latest_once = false;
        let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

        let action = context
            .process_event(
                InlineEvent::OpenUrl("https://example.com/docs".to_string()),
                &mut queue,
            )
            .await
            .expect("process open url");
        assert!(matches!(action, InlineLoopAction::Continue));

        let cancel = context
            .process_event(
                InlineEvent::Transient(TransientEvent::Cancelled),
                &mut queue,
            )
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
        other => panic!(
            "expected transient command, got different command: {:?}",
            other_name(&other)
        ),
    }

    let restored_command = commands.recv().await.expect("restored palette command");
    match restored_command {
        InlineCommand::ShowTransient { request } => match *request {
            TransientRequest::List(request) => assert_eq!(request.title, "Model"),
            other => panic!("expected list transient, got {other:?}"),
        },
        other => panic!(
            "expected transient command, got different command: {:?}",
            other_name(&other)
        ),
    }

    assert!(matches!(palette_state, Some(ActivePalette::ModelTarget)));
}

#[tokio::test]
async fn toggle_mode_event_submits_mode_command() {
    let (handle, mut renderer) = renderer_with_handle();
    let ctrl_c_state = CtrlCState::new();
    let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_tui::app::InlineHeaderContext::default();
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
        &session_bootstrap,
        false,
        0,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(InlineEvent::ToggleMode, &mut queue)
        .await
        .expect("process toggle mode");
    assert!(matches!(
        action,
        InlineLoopAction::Submit(ref command) if command == "/mode"
    ));
}

#[tokio::test]
async fn settings_editor_selection_submits_editor_config_command() {
    let (handle, mut renderer) = renderer_with_handle();
    let ctrl_c_state = CtrlCState::new();
    let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = Some(ActivePalette::Settings {
        state: Box::new(SettingsPaletteState {
            workspace: std::path::PathBuf::from("."),
            source_path: std::path::PathBuf::from("vtcode.toml"),
            source_label: "test".to_string(),
            draft: VTCodeConfig::default(),
            view_path: Some("tools".to_string()),
        }),
        esc_armed: false,
    });
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_tui::app::InlineHeaderContext::default();
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
        &session_bootstrap,
        false,
        0,
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
        InlineLoopAction::Submit(ref command) if command == "/config tools.editor"
    ));
    assert!(palette_state.is_none());
}

#[tokio::test]
async fn plan_confirmation_events_map_to_expected_actions() {
    let (handle, mut renderer) = renderer_with_handle();
    let ctrl_c_state = CtrlCState::new();
    let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_tui::app::InlineHeaderContext::default();
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
        &session_bootstrap,
        false,
        0,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let execute = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                vtcode_tui::app::InlineListSelection::PlanApprovalExecute,
            ))),
            &mut queue,
        )
        .await
        .expect("process execute");
    let auto = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                vtcode_tui::app::InlineListSelection::PlanApprovalAutoAccept,
            ))),
            &mut queue,
        )
        .await
        .expect("process auto");
    let edit = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                vtcode_tui::app::InlineListSelection::PlanApprovalEditPlan,
            ))),
            &mut queue,
        )
        .await
        .expect("process edit plan");
    let cancel = context
        .process_event(
            InlineEvent::Transient(TransientEvent::Cancelled),
            &mut queue,
        )
        .await
        .expect("process cancel");

    assert!(matches!(
        execute,
        InlineLoopAction::PlanApproved { auto_accept: false }
    ));
    assert!(matches!(
        auto,
        InlineLoopAction::PlanApproved { auto_accept: true }
    ));
    assert!(matches!(edit, InlineLoopAction::PlanEditRequested));
    assert!(matches!(cancel, InlineLoopAction::Continue));
}

#[tokio::test]
async fn interrupt_event_returns_exit_after_double_ctrl_c() {
    let (handle, mut renderer) = renderer_with_handle();
    let ctrl_c_state = CtrlCState::new();
    let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_tui::app::InlineHeaderContext::default();
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
        &session_bootstrap,
        false,
        0,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let _ = ctrl_c_state.register_signal();
    std::thread::sleep(Duration::from_millis(250));
    let _ = ctrl_c_state.register_signal();

    let action = context
        .process_event(InlineEvent::Interrupt, &mut queue)
        .await
        .expect("process interrupt");
    assert!(matches!(action, InlineLoopAction::Exit(_)));
}

#[tokio::test]
async fn steering_events_are_passive_in_idle_loop() {
    let (handle, mut renderer) = renderer_with_handle();
    let ctrl_c_state = CtrlCState::new();
    let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_tui::app::InlineHeaderContext::default();
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
        &session_bootstrap,
        false,
        0,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    for event in [
        InlineEvent::Pause,
        InlineEvent::Resume,
        InlineEvent::Steer("keep going".to_string()),
    ] {
        let action = context
            .process_event(event, &mut queue)
            .await
            .expect("process steering event");
        assert!(matches!(action, InlineLoopAction::Continue));
    }
}

#[tokio::test]
async fn process_latest_queued_event_primes_newest_queue_priority() {
    let (handle, mut renderer) = renderer_with_handle();
    let ctrl_c_state = CtrlCState::new();
    let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_tui::app::InlineHeaderContext::default();
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
        &session_bootstrap,
        false,
        0,
    );
    let mut queued_inputs = VecDeque::from(["first".to_string(), "latest".to_string()]);
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
    let ctrl_c_state = CtrlCState::new();
    let interrupts = InlineInterruptCoordinator::new(&ctrl_c_state);
    let mut ctrl_c_notice_displayed = false;
    let mut model_picker_state: Option<ModelPickerState> = None;
    let mut palette_state: Option<ActivePalette> = None;
    let mut config = runtime_config();
    let mut vt_cfg = None;
    let mut provider_client: Box<dyn uni::LLMProvider> = Box::new(DummyProvider);
    let session_bootstrap = SessionBootstrap::default();
    let mut header_context = vtcode_tui::app::InlineHeaderContext::default();
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
        &session_bootstrap,
        false,
        0,
    );
    let mut queued_inputs = VecDeque::new();
    let mut prefer_latest_once = false;
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs, &mut prefer_latest_once);

    let action = context
        .process_event(
            InlineEvent::RequestInlinePromptSuggestion("Review the current".to_string()),
            &mut queue,
        )
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
        InlineCommand::SetPrompt { .. } => "SetPrompt",
        InlineCommand::SetPlaceholder { .. } => "SetPlaceholder",
        InlineCommand::SetMessageLabels { .. } => "SetMessageLabels",
        InlineCommand::SetHeaderContext { .. } => "SetHeaderContext",
        InlineCommand::SetInputStatus { .. } => "SetInputStatus",
        InlineCommand::SetTheme { .. } => "SetTheme",
        InlineCommand::SetAppearance { .. } => "SetAppearance",
        InlineCommand::SetVimModeEnabled(_) => "SetVimModeEnabled",
        InlineCommand::SetQueuedInputs { .. } => "SetQueuedInputs",
        InlineCommand::SetSubprocessEntries { .. } => "SetSubprocessEntries",
        InlineCommand::SetSubagentPreview { .. } => "SetSubagentPreview",
        InlineCommand::SetLocalAgents { .. } => "SetLocalAgents",
        InlineCommand::SetCursorVisible(_) => "SetCursorVisible",
        InlineCommand::SetInputEnabled(_) => "SetInputEnabled",
        InlineCommand::SetInput(_) => "SetInput",
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
        InlineCommand::SetEditingMode(_) => "SetEditingMode",
        InlineCommand::SetAutonomousMode(_) => "SetAutonomousMode",
        InlineCommand::SetSkipConfirmations(_) => "SetSkipConfirmations",
        InlineCommand::Shutdown => "Shutdown",
        InlineCommand::SetReasoningStage(_) => "SetReasoningStage",
    }
}
