use super::*;

#[tokio::test]
async fn emits_progress_events_for_stream_deltas() {
    let provider = StagedReasoningProvider {
        content: "final content".to_string(),
        reasoning: "thinking".to_string(),
        stage: "analysis".to_string(),
    };
    let request = build_request();
    let (tx, _rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());
    let mut events: Vec<StreamProgressEvent> = Vec::new();
    let mut callback = |event: StreamProgressEvent| events.push(event);

    let (_resp, _emitted) = stream_and_render_response_with_options_and_progress(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
        StreamSpinnerOptions::default(),
        Some(&mut callback),
    )
    .await
    .expect("stream should succeed");

    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamProgressEvent::ReasoningDelta(delta) if delta == "thinking"))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamProgressEvent::OutputDelta(delta) if delta == "final content"))
    );
}
#[tokio::test]
async fn suppresses_unverified_stream_output_but_returns_response_for_handling() {
    let provider = ReasoningThenContentProvider {
        content: "The change is complete.".to_string(),
        reasoning: "unverified reasoning".to_string(),
    };
    let request = build_request();
    let (tx, _rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());
    let mut events: Vec<StreamProgressEvent> = Vec::new();
    let mut callback = |event: StreamProgressEvent| events.push(event);

    let (response, emitted) = stream_and_render_response_with_options_and_progress(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
        StreamSpinnerOptions {
            suppress_output: true,
            ..StreamSpinnerOptions::default()
        },
        Some(&mut callback),
    )
    .await
    .expect("suppressed stream should still return its response");

    assert!(!emitted, "unverified assistant output must not be rendered");
    assert_eq!(response.content.as_deref(), Some("The change is complete."));
    assert!(
        !events.iter().any(|event| matches!(event, StreamProgressEvent::OutputDelta(_))),
        "unverified assistant output must not emit lifecycle OutputDelta events"
    );
}
#[tokio::test]
async fn skips_reasoning_progress_events_when_streaming_is_unavailable() {
    let provider = StagedReasoningProvider {
        content: "final content".to_string(),
        reasoning: "thinking".to_string(),
        stage: "analysis".to_string(),
    };
    let request = build_request();
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());
    let mut events: Vec<StreamProgressEvent> = Vec::new();
    let mut callback = |event: StreamProgressEvent| events.push(event);

    let (_resp, _emitted) = stream_and_render_response_with_options_and_progress(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
        StreamSpinnerOptions::default(),
        Some(&mut callback),
    )
    .await
    .expect("stream should succeed");

    assert!(
        !events
            .iter()
            .any(|event| matches!(event, StreamProgressEvent::ReasoningStage(_))),
        "reasoning stage deltas should not stream when inline reasoning streaming is unavailable"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, StreamProgressEvent::ReasoningDelta(_))),
        "reasoning deltas should not stream when inline reasoning streaming is unavailable"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamProgressEvent::OutputDelta(delta) if delta == "final content"))
    );
}
#[tokio::test]
async fn suppresses_harmony_tool_call_wire_text_from_output_deltas() {
    let provider = ReasoningThenChunkedContentProvider {
        chunks: vec![
            "<|start|>assistant<|channel|>commentary to=functions.code_search <|constrain|>json<|message|>{\"query\":\"runloop\"}<|call|>".to_string(),
            "<|start|>assistant<|channel|>final<|message|>safe answer<|end|>".to_string(),
        ],
        reasoning_chunks: vec![],
    };
    let request = build_request();
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());
    let mut events: Vec<StreamProgressEvent> = Vec::new();
    let mut callback = |event: StreamProgressEvent| events.push(event);

    let (_resp, _emitted) = stream_and_render_response_with_options_and_progress(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
        StreamSpinnerOptions::default(),
        Some(&mut callback),
    )
    .await
    .expect("stream should succeed");

    let output_deltas: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            StreamProgressEvent::OutputDelta(delta) => Some(delta.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(output_deltas, vec!["safe answer"]);
}
