use super::*;

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
