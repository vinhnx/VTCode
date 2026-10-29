use super::*;

#[tokio::test]
async fn renders_completed_only_content() {
    let provider = CompletedOnlyProvider {
        content: Some("hello world".to_string()),
        reasoning: None,
    };
    let request = build_request();
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());

    let (resp, emitted) = stream_and_render_response(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
    )
    .await
    .expect("stream should succeed");

    assert!(emitted, "should mark emitted tokens when content is rendered");
    assert_eq!(resp.content.as_deref(), Some("hello world"));
}
#[tokio::test]
async fn hides_unclassified_completed_reasoning_when_no_content() {
    let provider = CompletedOnlyProvider {
        content: None,
        reasoning: Some("because reason".to_string()),
    };
    let request = build_request();
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());

    let (resp, emitted) = stream_and_render_response(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
    )
    .await
    .expect("stream should succeed");

    assert!(!emitted, "unclassified completed reasoning must not be rendered");
    assert!(resp.content.is_none(), "content should remain none");
}
#[tokio::test]
async fn keeps_proposed_plan_tags_visible_outside_planning_workflow() {
    let provider = CompletedOnlyProvider {
        content: Some("<proposed_plan>\n- Step 1\n</proposed_plan>".to_string()),
        reasoning: None,
    };
    let request = build_request();
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());

    let (_resp, emitted) = stream_and_render_response(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
    )
    .await
    .expect("stream should succeed");

    assert!(emitted, "outside planning workflow, proposed_plan blocks should remain visible");
}
#[tokio::test]
async fn strips_proposed_plan_tags_when_option_enabled() {
    let provider = CompletedOnlyProvider {
        content: Some("<proposed_plan>\n- Step 1\n</proposed_plan>".to_string()),
        reasoning: None,
    };
    let request = build_request();
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());

    let (_resp, emitted) = stream_and_render_response_with_options(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
        StreamSpinnerOptions {
            defer_finish: false,
            strip_proposed_plan_blocks: true,
            suppress_output: false,
            suppress_output_signal: None,
        },
    )
    .await
    .expect("stream should succeed");

    assert!(!emitted, "when stripping is enabled, pure proposed_plan content should be hidden");
}
#[tokio::test]
async fn does_not_suppress_large_content_after_reasoning_stream() {
    let provider = ReasoningThenContentProvider {
        content: "x".repeat(5_000),
        reasoning: "thinking".to_string(),
    };
    let request = build_request();
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());

    let (_resp, emitted) = stream_and_render_response(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
    )
    .await
    .expect("stream should succeed");

    assert!(emitted, "large token content should still be rendered after reasoning events");
}
#[tokio::test]
async fn marks_reasoning_only_streams_as_rendered() {
    let provider = ReasoningThenChunkedContentProvider {
        chunks: vec![],
        reasoning_chunks: vec!["Applying formatting".to_string()],
    };
    let request = build_request();
    let (tx, _rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());

    let (_resp, emitted) = stream_and_render_response(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
    )
    .await
    .expect("stream should succeed");

    assert!(emitted, "reasoning-only streams should count as rendered to avoid duplicate replay");
}
