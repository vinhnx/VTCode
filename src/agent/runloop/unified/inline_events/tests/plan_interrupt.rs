use super::*;

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
