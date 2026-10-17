use super::{
    CopilotRuntimeRequestHandler, FirstProgressTimeout, merge_streamed_plan_into_response, normalized_to_legacy_stream,
    render_stream_with_options_and_copilot_runtime_impl,
};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::ui_interaction::{PlaceholderSpinner, StreamProgressEvent, StreamSpinnerOptions};
use async_trait::async_trait;
use futures::{StreamExt, stream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::{Notify, mpsc};
use vtcode_core::copilot::{CopilotObservedToolCall, CopilotObservedToolCallStatus, CopilotRuntimeRequest};
use vtcode_core::llm::provider::{self as uni, FinishReason, LLMResponse, LLMStreamEvent, ReasoningSource};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{InlineCommand, InlineHandle};

struct SleepingRuntimeHandler {
    sleep_for: Duration,
}

struct ToggleSuppressionRuntimeHandler {
    signal: Arc<AtomicBool>,
}

struct EnableSuppressionRuntimeHandler {
    signal: Arc<AtomicBool>,
}

#[async_trait]
impl CopilotRuntimeRequestHandler for SleepingRuntimeHandler {
    async fn handle_runtime_request(
        &mut self,
        _renderer: &mut AnsiRenderer,
        _request: CopilotRuntimeRequest,
    ) -> Result<(), uni::LLMError> {
        tokio::time::sleep(self.sleep_for).await;
        Ok(())
    }
}

#[async_trait]
impl CopilotRuntimeRequestHandler for ToggleSuppressionRuntimeHandler {
    async fn handle_runtime_request(
        &mut self,
        _renderer: &mut AnsiRenderer,
        _request: CopilotRuntimeRequest,
    ) -> Result<(), uni::LLMError> {
        self.signal.store(false, Ordering::Release);
        Ok(())
    }
}

#[async_trait]
impl CopilotRuntimeRequestHandler for EnableSuppressionRuntimeHandler {
    async fn handle_runtime_request(
        &mut self,
        _renderer: &mut AnsiRenderer,
        _request: CopilotRuntimeRequest,
    ) -> Result<(), uni::LLMError> {
        self.signal.store(true, Ordering::Release);
        Ok(())
    }
}

fn build_spinner() -> PlaceholderSpinner {
    let (tx, _rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    PlaceholderSpinner::new(&handle, None, None, "")
}

fn completed_response_with_content(content: Option<&str>) -> LLMResponse {
    LLMResponse {
        content: content.map(str::to_string),
        model: "mock-model".to_string(),
        tool_calls: None,
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        organization_id: None,
        request_id: None,
        tool_references: vec![],
        compaction: None,
    }
}

#[tokio::test]
async fn normalized_ui_stream_forwards_only_public_reasoning_summaries() {
    let normalized: uni::LLMNormalizedStream = Box::pin(stream::iter(vec![
        Ok(uni::NormalizedStreamEvent::ReasoningDelta {
            delta: "raw reasoning".to_string(),
            source: ReasoningSource::Raw,
        }),
        Ok(uni::NormalizedStreamEvent::ReasoningDelta {
            delta: "public summary".to_string(),
            source: ReasoningSource::ProviderSummary,
        }),
        Ok(uni::NormalizedStreamEvent::ReasoningDelta {
            delta: "continuation detail".to_string(),
            source: ReasoningSource::Continuation,
        }),
        Ok(uni::NormalizedStreamEvent::Done {
            response: Box::new(completed_response_with_content(None)),
        }),
    ]));
    let (mut legacy, _progress) = normalized_to_legacy_stream(normalized);
    let mut events = Vec::new();
    while let Some(event) = legacy.next().await {
        events.push(event.expect("normalized event should convert"));
    }

    assert!(matches!(
        events.as_slice(),
        [LLMStreamEvent::Reasoning { delta }, LLMStreamEvent::Completed { .. }]
            if delta == "public summary"
    ));
}

#[tokio::test]
async fn hidden_reasoning_counts_as_first_progress_without_being_rendered() {
    let normalized: uni::LLMNormalizedStream = Box::pin(async_stream::stream! {
        yield Ok(uni::NormalizedStreamEvent::ReasoningDelta {
            delta: "private trace".to_string(),
            source: ReasoningSource::Raw,
        });
        tokio::time::sleep(Duration::from_millis(40)).await;
        yield Ok(uni::NormalizedStreamEvent::Done {
            response: Box::new(completed_response_with_content(Some("answer"))),
        });
    });
    let (mut legacy, mut progress) = normalized_to_legacy_stream(normalized);
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let mut observed = Vec::new();
    let mut on_progress = |event| observed.push(event);
    let result = render_stream_with_options_and_copilot_runtime_impl(
        "mock",
        &mut legacy,
        Some(&mut progress),
        None,
        None,
        Some(FirstProgressTimeout::starting_now(Duration::from_millis(5))),
        &spinner,
        &mut renderer,
        &ctrl_c_state,
        &ctrl_c_notify,
        StreamSpinnerOptions::default(),
        Some(&mut on_progress),
    )
    .await;

    assert!(result.is_ok(), "hidden provider progress should clear the first-progress timeout");
    assert!(
        observed
            .iter()
            .any(|event| matches!(event, StreamProgressEvent::ProviderActivity))
    );
    assert!(
        !observed
            .iter()
            .any(|event| matches!(event, StreamProgressEvent::ReasoningDelta(_))),
        "private reasoning must never become visible output"
    );
}

fn completed_response(content: &str) -> LLMResponse {
    completed_response_with_content(Some(content))
}

const STREAMED_PLAN: &str = "- Step 1";

fn count_plan_blocks(text: &str) -> usize {
    text.matches("<proposed_plan>").count()
}

fn collect_rendered_inline_text(command_rx: &mut mpsc::UnboundedReceiver<InlineCommand>) -> String {
    std::iter::from_fn(|| command_rx.try_recv().ok())
        .filter_map(|command| match command {
            InlineCommand::AppendLine { segments, .. } => {
                Some(segments.into_iter().map(|segment| segment.text).collect::<String>())
            }
            InlineCommand::Inline { segment, .. } => Some(segment.text),
            InlineCommand::ReplaceLast { lines, .. } => Some(
                lines
                    .into_iter()
                    .flat_map(|line| line.into_iter().map(|segment| segment.text))
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>()
}

#[tokio::test]
async fn completion_output_is_observed_once_and_respects_suppression() {
    for inline in [false, true] {
        for streamed in [false, true] {
            for suppressed in [false, true] {
                for content in ["answer", " \n", ""] {
                    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
                    let handle = InlineHandle::new_for_tests(command_tx);
                    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
                    let mut renderer = if inline {
                        AnsiRenderer::with_inline_ui(handle, Default::default())
                    } else {
                        AnsiRenderer::stdout()
                    };
                    let mut events = Vec::new();
                    if streamed {
                        events.push(Ok(LLMStreamEvent::Token { delta: content.into() }));
                    }
                    events.push(Ok(LLMStreamEvent::Completed { response: Box::new(completed_response(content)) }));
                    let mut stream: uni::LLMStream = Box::pin(stream::iter(events));
                    let mut observed = Vec::new();
                    let mut on_progress = |event| {
                        if let StreamProgressEvent::OutputDelta(delta) = event {
                            observed.push(delta);
                        }
                    };
                    let (_, rendered) = render_stream_with_options_and_copilot_runtime_impl(
                        "mock",
                        &mut stream,
                        None,
                        None,
                        None,
                        None,
                        &spinner,
                        &mut renderer,
                        &Arc::new(CtrlCState::new()),
                        &Arc::new(Notify::new()),
                        StreamSpinnerOptions {
                            suppress_output: suppressed,
                            ..StreamSpinnerOptions::default()
                        },
                        Some(&mut on_progress),
                    )
                    .await
                    .unwrap();
                    let visible: Vec<_> = observed
                        .iter()
                        .map(String::as_str)
                        .filter(|text| !text.trim().is_empty())
                        .collect();
                    if content == "answer" && !suppressed {
                        assert_eq!(visible, ["answer"], "no duplicate completion observation after tokens");
                        assert!(rendered);
                        if inline {
                            assert!(collect_rendered_inline_text(&mut command_rx).contains("answer"));
                        }
                    } else {
                        assert!(visible.is_empty(), "blank or suppressed content is not visible output");
                    }
                    if suppressed {
                        assert!(!rendered);
                        assert!(observed.is_empty());
                        assert!(!collect_rendered_inline_text(&mut command_rx).contains("answer"));
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn completion_only_plan_observation_contains_only_visible_prose() {
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let mut stream: uni::LLMStream = Box::pin(stream::iter([Ok(LLMStreamEvent::Completed {
        response: Box::new(completed_response("answer\n<proposed_plan>\n- hidden plan\n</proposed_plan>")),
    })]));
    let mut observed = Vec::new();
    let mut on_progress = |event| {
        if let StreamProgressEvent::OutputDelta(delta) = event {
            observed.push(delta);
        }
    };
    render_stream_with_options_and_copilot_runtime_impl(
        "mock",
        &mut stream,
        None,
        None,
        None,
        None,
        &spinner,
        &mut renderer,
        &Arc::new(CtrlCState::new()),
        &Arc::new(Notify::new()),
        StreamSpinnerOptions {
            strip_proposed_plan_blocks: true,
            ..StreamSpinnerOptions::default()
        },
        Some(&mut on_progress),
    )
    .await
    .unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].trim(), "answer");
    let rendered = collect_rendered_inline_text(&mut command_rx);
    assert!(rendered.contains("answer"));
    assert!(!rendered.contains("hidden plan"));
}

#[tokio::test]
async fn split_streamed_plan_is_forwarded_into_completed_response() {
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let mut stream: uni::LLMStream = Box::pin(async_stream::stream! {
        yield Ok(LLMStreamEvent::Token { delta: "Intro\n<propo".to_string() });
        yield Ok(LLMStreamEvent::Token {
            delta: "sed_plan>\n- Step 1\n</proposed_plan>\nOutro".to_string(),
        });
        yield Ok(LLMStreamEvent::Completed {
            response: Box::new(completed_response_with_content(None)),
        });
    });

    let (response, _) = render_stream_with_options_and_copilot_runtime_impl(
        "mock",
        &mut stream,
        None,
        None,
        None,
        None,
        &spinner,
        &mut renderer,
        &ctrl_c_state,
        &ctrl_c_notify,
        StreamSpinnerOptions {
            strip_proposed_plan_blocks: true,
            ..StreamSpinnerOptions::default()
        },
        None,
    )
    .await
    .expect("stream should return its completed response");

    let response_content = response
        .content
        .as_deref()
        .expect("streamed plan should be merged into content");
    assert_eq!(count_plan_blocks(response_content), 1);
    assert!(response_content.contains("Intro"));
    assert!(response_content.contains(STREAMED_PLAN));
    assert!(response_content.contains("</proposed_plan>"));
}

#[tokio::test]
async fn split_streamed_alternate_plan_is_forwarded_as_canonical_plan() {
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let mut stream: uni::LLMStream = Box::pin(async_stream::stream! {
        yield Ok(LLMStreamEvent::Token { delta: "Intro\n<pl".to_string() });
        yield Ok(LLMStreamEvent::Token {
            delta: "an>\n- Step 1\n</plan>\nOutro".to_string(),
        });
        yield Ok(LLMStreamEvent::Completed {
            response: Box::new(completed_response_with_content(None)),
        });
    });

    let (response, _) = render_stream_with_options_and_copilot_runtime_impl(
        "mock",
        &mut stream,
        None,
        None,
        None,
        None,
        &spinner,
        &mut renderer,
        &ctrl_c_state,
        &ctrl_c_notify,
        StreamSpinnerOptions {
            strip_proposed_plan_blocks: true,
            ..StreamSpinnerOptions::default()
        },
        None,
    )
    .await
    .expect("stream should return its completed response");

    let response_content = response
        .content
        .as_deref()
        .expect("alternate streamed plan should be merged into content");
    assert_eq!(count_plan_blocks(response_content), 1);
    assert!(!response_content.contains("<plan>"));
    assert!(response_content.contains(STREAMED_PLAN));
    assert!(response_content.contains("</proposed_plan>"));
}

#[tokio::test]
async fn suppressed_stream_still_forwards_streamed_plan_without_rendering() {
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let mut stream: uni::LLMStream = Box::pin(async_stream::stream! {
        yield Ok(LLMStreamEvent::Token {
            delta: "<proposed_plan>\n- Step 1\n</proposed_plan>".to_string(),
        });
        yield Ok(LLMStreamEvent::Completed {
            response: Box::new(completed_response_with_content(None)),
        });
    });

    let (response, rendered) = render_stream_with_options_and_copilot_runtime_impl(
        "mock",
        &mut stream,
        None,
        None,
        None,
        None,
        &spinner,
        &mut renderer,
        &ctrl_c_state,
        &ctrl_c_notify,
        StreamSpinnerOptions {
            strip_proposed_plan_blocks: true,
            suppress_output: true,
            ..StreamSpinnerOptions::default()
        },
        None,
    )
    .await
    .expect("suppressed stream should return its completed response");

    assert!(!rendered, "suppression must still prevent rendering");
    let response_content = response.content.as_deref().expect("suppressed plan should be forwarded");
    assert_eq!(count_plan_blocks(response_content), 1);
    assert!(response_content.contains(STREAMED_PLAN));
}

#[tokio::test]
async fn streamed_prose_remains_visible_without_plan_markup() {
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let mut stream: uni::LLMStream = Box::pin(async_stream::stream! {
        yield Ok(LLMStreamEvent::Token {
            delta: "Intro\n<proposed_plan>\n- Step 1\n</proposed_plan>\nOutro".to_string(),
        });
        yield Ok(LLMStreamEvent::Completed {
            response: Box::new(completed_response_with_content(Some("Intro\n\nOutro"))),
        });
    });

    let (response, rendered) = render_stream_with_options_and_copilot_runtime_impl(
        "mock",
        &mut stream,
        None,
        None,
        None,
        None,
        &spinner,
        &mut renderer,
        &ctrl_c_state,
        &ctrl_c_notify,
        StreamSpinnerOptions {
            strip_proposed_plan_blocks: true,
            ..StreamSpinnerOptions::default()
        },
        None,
    )
    .await
    .expect("stream should render its completed response");

    assert!(rendered);
    let response_content = response.content.as_deref().expect("response content should be present");
    assert_eq!(count_plan_blocks(response_content), 1);
    let rendered_text = collect_rendered_inline_text(&mut command_rx);
    assert!(rendered_text.contains("Intro"), "rendered text: {rendered_text:?}");
    assert!(rendered_text.contains("Outro"), "rendered text: {rendered_text:?}");
    assert!(!rendered_text.contains("<proposed_plan>"));
    assert!(!rendered_text.contains(STREAMED_PLAN));
}

#[test]
fn completed_plan_block_is_not_duplicated_during_stream_handoff() {
    let mut response =
        completed_response_with_content(Some("Intro\n<proposed_plan>\n- Completed step\n</proposed_plan>\nOutro"));

    merge_streamed_plan_into_response(&mut response, Some(STREAMED_PLAN.to_string()), "Intro\n\nOutro");

    let content = response.content.expect("completed response content should be preserved");
    assert_eq!(count_plan_blocks(&content), 1);
    assert!(content.contains("- Completed step"));
    assert!(!content.contains(STREAMED_PLAN));
}

#[test]
fn completed_content_takes_precedence_and_alternate_plan_is_not_duplicated() {
    let mut response = completed_response_with_content(Some("completed prose"));
    merge_streamed_plan_into_response(&mut response, Some(STREAMED_PLAN.to_string()), "streamed prose");
    let content = response.content.expect("completed content should be preserved");
    assert!(content.starts_with("completed prose"));
    assert!(!content.contains("streamed prose"));
    assert_eq!(count_plan_blocks(&content), 1);

    let mut response = completed_response_with_content(Some("Intro\n<plan>\n- Existing step\n</plan>\nOutro"));
    merge_streamed_plan_into_response(&mut response, Some(STREAMED_PLAN.to_string()), "visible prose");
    let content = response.content.expect("alternate plan content should be preserved");
    assert_eq!(content.matches("<plan>").count(), 1);
    assert_eq!(count_plan_blocks(&content), 0);
    assert!(content.contains("- Existing step"));
    assert!(!content.contains(STREAMED_PLAN));
}

#[tokio::test]
async fn copilot_runtime_request_counts_as_first_progress() {
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let mut stream: uni::LLMStream = Box::pin(stream::once(async {
        tokio::time::sleep(Duration::from_millis(5)).await;
        Ok(LLMStreamEvent::Completed { response: Box::new(completed_response("ok")) })
    }));

    let (runtime_tx, mut runtime_rx) = mpsc::unbounded_channel();
    runtime_tx
        .send(CopilotRuntimeRequest::ObservedToolCall(CopilotObservedToolCall {
            tool_call_id: "call_1".to_string(),
            tool_name: "copilot_tool".to_string(),
            status: CopilotObservedToolCallStatus::Pending,
            arguments: None,
            output: None,
            terminal_id: None,
        }))
        .expect("send runtime request");
    drop(runtime_tx);

    let mut handler = SleepingRuntimeHandler { sleep_for: Duration::from_millis(40) };

    let result = render_stream_with_options_and_copilot_runtime_impl(
        "copilot",
        &mut stream,
        None,
        Some(&mut runtime_rx),
        Some(&mut handler),
        Some(FirstProgressTimeout::starting_now(Duration::from_millis(20))),
        &spinner,
        &mut renderer,
        &ctrl_c_state,
        &ctrl_c_notify,
        StreamSpinnerOptions::default(),
        None,
    )
    .await;

    assert!(result.is_ok(), "runtime request should clear the first-progress timeout");
}

#[tokio::test]
async fn stream_times_out_before_first_progress() {
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());

    let mut stream: uni::LLMStream = Box::pin(stream::once(async {
        tokio::time::sleep(Duration::from_millis(40)).await;
        Ok(LLMStreamEvent::Completed { response: Box::new(completed_response("ok")) })
    }));

    let result = render_stream_with_options_and_copilot_runtime_impl(
        "mock",
        &mut stream,
        None,
        None,
        None,
        Some(FirstProgressTimeout::starting_now(Duration::from_millis(5))),
        &spinner,
        &mut renderer,
        &ctrl_c_state,
        &ctrl_c_notify,
        StreamSpinnerOptions::default(),
        None,
    )
    .await;

    let err = result.expect_err("stream should time out before first progress");
    assert!(err.to_string().contains("first token timed out"));
}

#[tokio::test]
async fn dynamic_suppression_hides_output_until_copilot_verification_clears_it() {
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let suppress_output_signal = Arc::new(AtomicBool::new(true));
    let handler_signal = suppress_output_signal.clone();

    let mut stream: uni::LLMStream = Box::pin(async_stream::stream! {
        yield Ok(LLMStreamEvent::Token { delta: "hidden before verification".to_string() });
        tokio::time::sleep(Duration::from_millis(20)).await;
        yield Ok(LLMStreamEvent::Completed { response: Box::new(completed_response("final response")) });
    });
    let (runtime_tx, mut runtime_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        runtime_tx
            .send(CopilotRuntimeRequest::ObservedToolCall(CopilotObservedToolCall {
                tool_call_id: "call_1".to_string(),
                tool_name: "apply_patch".to_string(),
                status: CopilotObservedToolCallStatus::Completed,
                arguments: None,
                output: None,
                terminal_id: None,
            }))
            .expect("send runtime request");
    });

    let mut handler = ToggleSuppressionRuntimeHandler { signal: handler_signal };
    let mut output_deltas = Vec::new();
    let mut on_progress = |event| {
        if let StreamProgressEvent::OutputDelta(delta) = event {
            output_deltas.push(delta);
        }
    };

    let (response, rendered) = render_stream_with_options_and_copilot_runtime_impl(
        "copilot",
        &mut stream,
        None,
        Some(&mut runtime_rx),
        Some(&mut handler),
        None,
        &spinner,
        &mut renderer,
        &ctrl_c_state,
        &ctrl_c_notify,
        StreamSpinnerOptions {
            suppress_output_signal: Some(suppress_output_signal),
            ..StreamSpinnerOptions::default()
        },
        Some(&mut on_progress),
    )
    .await
    .expect("Copilot stream should return its final response");

    assert_eq!(response.content.as_deref(), Some("final response"));
    assert!(rendered);
    assert_eq!(output_deltas, ["final response"], "only the visible completion may trigger OutputDelta");

    let rendered_text = std::iter::from_fn(|| command_rx.try_recv().ok())
        .filter_map(|command| match command {
            InlineCommand::AppendLine { segments, .. } => {
                Some(segments.into_iter().map(|segment| segment.text).collect::<String>())
            }
            InlineCommand::Inline { segment, .. } => Some(segment.text),
            InlineCommand::ReplaceLast { lines, .. } => Some(
                lines
                    .into_iter()
                    .flat_map(|line| line.into_iter().map(|segment| segment.text))
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>();
    assert!(rendered_text.contains("final response"));
    assert!(!rendered_text.contains("hidden before verification"));
}

#[tokio::test]
async fn dynamic_suppression_does_not_flush_pending_reasoning_after_mutation() {
    let (command_tx, mut command_rx) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    renderer.set_reasoning_visible(true);
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let suppress_output_signal = Arc::new(AtomicBool::new(false));
    let handler_signal = suppress_output_signal.clone();

    let mut stream: uni::LLMStream = Box::pin(async_stream::stream! {
        yield Ok(LLMStreamEvent::Reasoning { delta: "visible reasoning".to_string() });
        yield Ok(LLMStreamEvent::Reasoning { delta: "hidden pending reasoning".to_string() });
        tokio::time::sleep(Duration::from_millis(20)).await;
        yield Ok(LLMStreamEvent::ReasoningStage { stage: "tool execution".to_string() });
        yield Ok(LLMStreamEvent::Completed { response: Box::new(completed_response("final response")) });
    });
    let (runtime_tx, mut runtime_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        runtime_tx
            .send(CopilotRuntimeRequest::ObservedToolCall(CopilotObservedToolCall {
                tool_call_id: "call_1".to_string(),
                tool_name: "apply_patch".to_string(),
                status: CopilotObservedToolCallStatus::Completed,
                arguments: None,
                output: None,
                terminal_id: None,
            }))
            .expect("send runtime request");
    });

    let mut handler = EnableSuppressionRuntimeHandler { signal: handler_signal };
    let (response, rendered) = render_stream_with_options_and_copilot_runtime_impl(
        "copilot",
        &mut stream,
        None,
        Some(&mut runtime_rx),
        Some(&mut handler),
        None,
        &spinner,
        &mut renderer,
        &ctrl_c_state,
        &ctrl_c_notify,
        StreamSpinnerOptions {
            suppress_output_signal: Some(suppress_output_signal),
            ..StreamSpinnerOptions::default()
        },
        None,
    )
    .await
    .expect("Copilot stream should return its final response");

    assert_eq!(response.content.as_deref(), Some("final response"));
    assert!(!rendered, "the response must remain suppressed while verification is pending");

    let rendered_text = std::iter::from_fn(|| command_rx.try_recv().ok())
        .filter_map(|command| match command {
            InlineCommand::AppendLine { segments, .. } => {
                Some(segments.into_iter().map(|segment| segment.text).collect::<String>())
            }
            InlineCommand::Inline { segment, .. } => Some(segment.text),
            InlineCommand::ReplaceLast { lines, .. } => Some(
                lines
                    .into_iter()
                    .flat_map(|line| line.into_iter().map(|segment| segment.text))
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>();
    assert!(rendered_text.contains("visible reasoning"));
    assert!(!rendered_text.contains("hidden pending reasoning"));
    assert!(!rendered_text.contains("final response"));
}

struct ResumeWakeProvider {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl uni::LLMProvider for ResumeWakeProvider {
    fn name(&self) -> &str {
        "mock"
    }
    fn supports_streaming(&self) -> bool {
        true
    }
    async fn generate(&self, _: uni::LLMRequest) -> Result<LLMResponse, uni::LLMError> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(completed_response_with_content(Some("resumed planning")))
    }
    async fn stream_normalized(&self, request: uni::LLMRequest) -> Result<uni::LLMNormalizedStream, uni::LLMError> {
        let response = self.generate(request).await?;
        Ok(Box::pin(stream::iter(vec![Ok(uni::NormalizedStreamEvent::Done { response: Box::new(response) })])))
    }
    fn supported_models(&self) -> Vec<String> {
        vec!["mock-model".into()]
    }
    fn validate_request(&self, _: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

#[tokio::test]
async fn resume_wake_during_stream_establishment_is_not_cancellation() {
    for cancel in [false, true] {
        let provider = ResumeWakeProvider {
            entered: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        };
        let state = Arc::new(CtrlCState::new());
        let notify = Arc::new(Notify::new());
        let spinner = build_spinner();
        let mut renderer = AnsiRenderer::stdout();
        let request = super::stream_and_render_response_with_options_impl(
            &provider,
            uni::LLMRequest::default(),
            &spinner,
            &mut renderer,
            &state,
            &notify,
            StreamSpinnerOptions::default(),
            None,
        );
        let wake = async {
            provider.entered.notified().await;
            if cancel {
                state.request_local_cancel();
            }
            notify.notify_one();
            tokio::task::yield_now().await;
            provider.release.notify_one();
        };
        let (result, ()) = tokio::join!(request, wake);
        if cancel {
            assert!(result.is_err());
        } else {
            assert_eq!(
                result.expect("fresh input must remain runnable").0.content.as_deref(),
                Some("resumed planning")
            );
        }
    }
}

#[tokio::test]
async fn resume_wake_during_stream_events_is_not_cancellation() {
    for cancel in [false, true] {
        let state = Arc::new(CtrlCState::new());
        let notify = Arc::new(Notify::new());
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let stream_entered = entered.clone();
        let stream_release = release.clone();
        let mut events: uni::LLMStream = Box::pin(async_stream::stream! {
            stream_entered.notify_one();
            stream_release.notified().await;
            yield Ok(LLMStreamEvent::Completed { response: Box::new(completed_response_with_content(Some("resumed proposal"))) });
        });
        let spinner = build_spinner();
        let mut renderer = AnsiRenderer::stdout();
        let request = render_stream_with_options_and_copilot_runtime_impl(
            "mock",
            &mut events,
            None,
            None,
            None,
            None,
            &spinner,
            &mut renderer,
            &state,
            &notify,
            StreamSpinnerOptions::default(),
            None,
        );
        let wake = async {
            entered.notified().await;
            if cancel {
                state.request_local_cancel();
            }
            notify.notify_one();
            tokio::task::yield_now().await;
            release.notify_one();
        };
        let (result, ()) = tokio::join!(request, wake);
        if cancel {
            assert!(result.is_err());
        } else {
            assert_eq!(
                result.expect("wake cannot stop stream delivery").0.content.as_deref(),
                Some("resumed proposal")
            );
        }
    }
}
