use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::unified::inline_events::{
    InlineEventContext, InlineInterruptCoordinator, InlineLoopAction, InlineQueueState,
    TeamSwitchDirection,
};
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::welcome::SessionBootstrap;
use vtcode_core::config::core::PromptCachingConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{
    AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
};
use vtcode_core::core::agent::snapshots::{
    DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
};
use vtcode_core::llm::provider::{self as uni, LLMRequest, LLMResponse};
use vtcode_core::ui::tui::{InlineEvent, InlineHandle, PlanConfirmationResult};
use vtcode_core::utils::ansi::AnsiRenderer;

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
    }
}

fn renderer_with_handle() -> (InlineHandle, AnsiRenderer) {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    (handle, renderer)
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
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &session_bootstrap,
        false,
        false,
    );
    let mut queued_inputs = VecDeque::new();
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs);

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
async fn toggle_mode_event_submits_mode_command_outside_team_mode() {
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
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &session_bootstrap,
        false,
        false,
    );
    let mut queued_inputs = VecDeque::new();
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs);

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
async fn toggle_mode_event_uses_delegate_toggle_in_team_mode() {
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
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &session_bootstrap,
        false,
        true,
    );
    let mut queued_inputs = VecDeque::new();
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs);

    let action = context
        .process_event(InlineEvent::ToggleMode, &mut queue)
        .await
        .expect("process toggle mode");
    assert!(matches!(action, InlineLoopAction::ToggleDelegateMode));
}

#[tokio::test]
async fn team_navigation_events_map_to_switch_directions() {
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
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &session_bootstrap,
        false,
        true,
    );
    let mut queued_inputs = VecDeque::new();
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs);

    let prev = context
        .process_event(InlineEvent::TeamPrev, &mut queue)
        .await
        .expect("process team prev");
    let next = context
        .process_event(InlineEvent::TeamNext, &mut queue)
        .await
        .expect("process team next");

    assert!(matches!(
        prev,
        InlineLoopAction::SwitchTeammate(TeamSwitchDirection::Previous)
    ));
    assert!(matches!(
        next,
        InlineLoopAction::SwitchTeammate(TeamSwitchDirection::Next)
    ));
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
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &session_bootstrap,
        false,
        false,
    );
    let mut queued_inputs = VecDeque::new();
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs);

    let execute = context
        .process_event(
            InlineEvent::PlanConfirmation(PlanConfirmationResult::Execute),
            &mut queue,
        )
        .await
        .expect("process execute");
    let auto = context
        .process_event(
            InlineEvent::PlanConfirmation(PlanConfirmationResult::AutoAccept),
            &mut queue,
        )
        .await
        .expect("process auto");
    let clear_context_auto = context
        .process_event(
            InlineEvent::PlanConfirmation(PlanConfirmationResult::ClearContextAutoAccept),
            &mut queue,
        )
        .await
        .expect("process clear context auto");
    let edit = context
        .process_event(
            InlineEvent::PlanConfirmation(PlanConfirmationResult::EditPlan),
            &mut queue,
        )
        .await
        .expect("process edit plan");
    let cancel = context
        .process_event(
            InlineEvent::PlanConfirmation(PlanConfirmationResult::Cancel),
            &mut queue,
        )
        .await
        .expect("process cancel");

    assert!(matches!(
        execute,
        InlineLoopAction::PlanApproved {
            auto_accept: false,
            clear_context: false
        }
    ));
    assert!(matches!(
        auto,
        InlineLoopAction::PlanApproved {
            auto_accept: true,
            clear_context: false
        }
    ));
    assert!(matches!(
        clear_context_auto,
        InlineLoopAction::PlanApproved {
            auto_accept: true,
            clear_context: true
        }
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
    let mut context = InlineEventContext::new(
        &mut renderer,
        &handle,
        interrupts,
        &mut ctrl_c_notice_displayed,
        &mut model_picker_state,
        &mut palette_state,
        &mut config,
        &mut vt_cfg,
        &mut provider_client,
        &session_bootstrap,
        false,
        false,
    );
    let mut queued_inputs = VecDeque::new();
    let mut queue = InlineQueueState::new(&handle, &mut queued_inputs);

    let _ = ctrl_c_state.register_signal();
    std::thread::sleep(Duration::from_millis(250));
    let _ = ctrl_c_state.register_signal();

    let action = context
        .process_event(InlineEvent::Interrupt, &mut queue)
        .await
        .expect("process interrupt");
    assert!(matches!(action, InlineLoopAction::Exit(_)));
}
