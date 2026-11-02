use super::*;

#[tokio::test]
async fn emits_tool_call_progress_events_from_normalized_stream() {
    let provider = NormalizedToolCallProvider {
        content: "final content".to_string(),
        tool_name: "shell".to_string(),
        tool_call_id: "call_123".to_string(),
        tool_argument_chunks: vec!["{\"cmd\":\"ec".to_string(), "ho hi\"}".to_string()],
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

    assert!(events.iter().any(|event| {
        matches!(
            event,
            StreamProgressEvent::ToolCallStarted { call_id, name }
            if call_id == "call_123" && name.as_deref() == Some("shell")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            StreamProgressEvent::ToolCallDelta { call_id, delta }
            if call_id == "call_123" && delta == "{\"cmd\":\"ec"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            StreamProgressEvent::ToolCallDelta { call_id, delta }
            if call_id == "call_123" && delta == "ho hi\"}"
        )
    }));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, StreamProgressEvent::OutputDelta(delta) if delta == "final content"))
    );
}
#[tokio::test]
async fn inline_streams_small_content_deltas_after_reasoning() {
    let provider = ReasoningThenChunkedContentProvider {
        chunks: vec!["hello ".to_string(), "world".to_string()],
        reasoning_chunks: vec!["thinking".to_string()],
    };
    let request = build_request();
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
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

    assert!(emitted, "inline renderer should emit streamed content");

    let mut replace_last_agent_updates = 0usize;
    while let Ok(command) = rx.try_recv() {
        if let InlineCommand::ReplaceLast {
            kind: vtcode_ui::tui::app::InlineMessageKind::Agent,
            ..
        } = command
        {
            replace_last_agent_updates += 1;
        }
    }

    assert!(
        replace_last_agent_updates >= 2,
        "expected at least 2 incremental agent updates, got {replace_last_agent_updates}"
    );
}
#[tokio::test]
async fn inline_streams_reasoning_deltas_live() {
    let provider = ReasoningThenChunkedContentProvider {
        chunks: vec!["done".to_string()],
        reasoning_chunks: vec!["thinking".to_string()],
    };
    let request = build_request();
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());

    let (_resp, _emitted) = stream_and_render_response(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
    )
    .await
    .expect("stream should succeed");

    let mut saw_reasoning_inline = false;
    while let Ok(command) = rx.try_recv() {
        if let InlineCommand::Inline {
            kind: vtcode_ui::tui::app::InlineMessageKind::Policy,
            segment,
        } = command
            && segment.text.contains("thinking")
        {
            saw_reasoning_inline = true;
            break;
        }
    }

    assert!(saw_reasoning_inline, "expected inline reasoning delta to be streamed to TUI");
}
#[tokio::test]
async fn inline_streaming_compacts_blank_lines_and_highlights_decisions() {
    let provider = ReasoningThenChunkedContentProvider {
        chunks: vec!["done".to_string()],
        // Blank-line spam around a decision line, split across chunks.
        reasoning_chunks: vec![
            "surveying the repo\n\n\n\n".to_string(),
            "I will run the tests now".to_string(),
            "\n\n\nthen summarize".to_string(),
        ],
    };
    let request = build_request();
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());

    let (_resp, _emitted) = stream_and_render_response(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
    )
    .await
    .expect("stream should succeed");

    let mut reasoning_text = String::new();
    while let Ok(command) = rx.try_recv() {
        if let InlineCommand::Inline { segment, .. } = command {
            reasoning_text.push_str(&segment.text);
        }
    }

    assert!(!reasoning_text.contains("\n\n"), "blank-line spam should be collapsed, got: {reasoning_text:?}");
    assert!(
        reasoning_text.contains("I will run the tests now"),
        "decision line must be present, got: {reasoning_text:?}"
    );
}
#[tokio::test]
async fn inline_streaming_does_not_replay_reasoning_on_completion() {
    let provider = ReasoningThenChunkedContentProvider {
        chunks: vec![],
        reasoning_chunks: vec!["thinking".to_string()],
    };
    let request = build_request();
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());

    let (_resp, _emitted) = stream_and_render_response(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
    )
    .await
    .expect("stream should succeed");

    let mut reasoning_occurrences = 0usize;
    while let Ok(command) = rx.try_recv() {
        let text = match command {
            InlineCommand::AppendLine { segments, .. } => {
                segments.into_iter().map(|segment| segment.text).collect::<String>()
            }
            InlineCommand::AppendPastedMessage { text, .. } => text,
            InlineCommand::Inline { segment, .. } => segment.text,
            InlineCommand::ReplaceLast { lines, .. } => lines
                .into_iter()
                .flat_map(|line| line.into_iter().map(|segment| segment.text))
                .collect::<String>(),
            _ => String::new(),
        };
        reasoning_occurrences += text.matches("thinking").count();
    }

    assert_eq!(reasoning_occurrences, 1, "expected live reasoning to render once, without replay at completion");
}
#[tokio::test]
async fn inline_batches_many_token_deltas_for_performance() {
    let provider = ReasoningThenChunkedContentProvider {
        chunks: vec!["x".to_string(); 600],
        reasoning_chunks: vec!["thinking".to_string()],
    };
    let request = build_request();
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
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

    assert!(emitted, "inline renderer should emit streamed content");

    let mut replace_last_agent_updates = 0usize;
    while let Ok(command) = rx.try_recv() {
        if let InlineCommand::ReplaceLast {
            kind: vtcode_ui::tui::app::InlineMessageKind::Agent,
            ..
        } = command
        {
            replace_last_agent_updates += 1;
        }
    }

    assert!(
        replace_last_agent_updates < 300,
        "expected batched updates for 600 deltas, got {replace_last_agent_updates} replace operations"
    );
}
#[tokio::test]
async fn inline_batches_many_reasoning_deltas_for_performance() {
    let provider = ReasoningThenChunkedContentProvider {
        chunks: vec!["done".to_string()],
        reasoning_chunks: vec!["x".to_string(); 600],
    };
    let request = build_request();
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = super::super::state::CtrlCState::new();
    let ctrl_c_notify = Arc::new(Notify::new());

    let (_resp, _emitted) = stream_and_render_response(
        &provider,
        request,
        &spinner,
        &mut renderer,
        &Arc::new(ctrl_c_state),
        &ctrl_c_notify,
    )
    .await
    .expect("stream should succeed");

    let mut policy_inline_updates = 0usize;
    while let Ok(command) = rx.try_recv() {
        if let InlineCommand::Inline {
            kind: vtcode_ui::tui::app::InlineMessageKind::Policy,
            ..
        } = command
        {
            policy_inline_updates += 1;
        }
    }

    assert!(
        policy_inline_updates < 300,
        "expected batched reasoning updates for 600 deltas, got {policy_inline_updates} inline updates"
    );
}
