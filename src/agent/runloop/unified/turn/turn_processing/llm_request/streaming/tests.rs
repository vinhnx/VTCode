use super::*;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::ui_interaction::{PlaceholderSpinner, StreamSpinnerOptions};
use crate::agent::runloop::unified::ui_interaction_stream::{
    CopilotRuntimeRequestHandler, render_stream_with_options_and_copilot_runtime_impl,
};
use std::sync::Arc;
use tokio::sync::{Notify, mpsc, oneshot};
use vtcode_commons::ui_protocol::ProgressUpdate;
use vtcode_core::copilot::{CopilotObservedToolCall, CopilotObservedToolCallStatus, CopilotRuntimeRequest};
use vtcode_core::llm::provider::{self as uni, FinishReason, LLMResponse, LLMStreamEvent};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{InlineCommand, InlineHandle};

fn completed_response(content: &str) -> LLMStreamEvent {
    LLMStreamEvent::Completed {
        response: Box::new(LLMResponse {
            content: Some(content.into()),
            model: "mock-model".into(),
            tool_calls: None,
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: vec![],
            compaction: None,
        }),
    }
}

struct PhaseChangingRuntimeHandler {
    handle: InlineHandle,
    phase: ProgressPhase,
    settled: Option<oneshot::Sender<()>>,
}

#[async_trait::async_trait]
impl CopilotRuntimeRequestHandler for PhaseChangingRuntimeHandler {
    async fn handle_runtime_request(
        &mut self,
        _renderer: &mut AnsiRenderer,
        _request: CopilotRuntimeRequest,
    ) -> Result<(), uni::LLMError> {
        self.handle.set_progress_phase(self.phase);
        self.settled.take().unwrap().send(()).unwrap();
        Ok(())
    }
}

#[tokio::test]
async fn model_progress_is_restored_after_runtime_tools_and_approval() {
    for runtime_phase in [ProgressPhase::RunningTools, ProgressPhase::WaitingForApproval] {
        for initial_text in [false, true] {
            let (command_tx, mut command_rx) = mpsc::unbounded_channel();
            let handle = InlineHandle::new_for_tests(command_tx);
            let guard = handle.begin_progress(ProgressPhase::WaitingForModel);
            let operation = guard.operation();
            let spinner = PlaceholderSpinner::new(&handle, None, None, "");
            let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
            let (runtime_tx, mut runtime_rx) = mpsc::unbounded_channel();
            let (settled_tx, settled_rx) = oneshot::channel();
            let mut handler = PhaseChangingRuntimeHandler {
                handle: handle.clone(),
                phase: runtime_phase,
                settled: Some(settled_tx),
            };
            let mut stream: uni::LLMStream = Box::pin(async_stream::stream! {
                if initial_text {
                    yield Ok(LLMStreamEvent::Token { delta: "before ".into() });
                }
                runtime_tx.send(CopilotRuntimeRequest::ObservedToolCall(CopilotObservedToolCall {
                    tool_call_id: "runtime-call".into(),
                    tool_name: "read_file".into(),
                    status: CopilotObservedToolCallStatus::Pending,
                    arguments: None,
                    output: None,
                    terminal_id: None,
                })).unwrap();
                settled_rx.await.unwrap();
                yield Ok(LLMStreamEvent::Token { delta: "after".into() });
                yield Ok(completed_response(if initial_text { "before after" } else { "after" }));
            });
            let mut latency = ResponseLatency::new(Some(operation), Instant::now(), 1, 1);
            let mut progress = |event: StreamProgressEvent| {
                handle.update_progress(ProgressUpdate::Phase { operation, phase: latency.observe(&event) });
            };
            render_stream_with_options_and_copilot_runtime_impl(
                "copilot",
                &mut stream,
                None,
                Some(&mut runtime_rx),
                Some(&mut handler),
                None,
                &spinner,
                &mut renderer,
                &Arc::new(CtrlCState::new()),
                &Arc::new(Notify::new()),
                StreamSpinnerOptions::default(),
                Some(&mut progress),
            )
            .await
            .unwrap();
            let phases: Vec<_> = std::iter::from_fn(|| command_rx.try_recv().ok())
                .filter_map(|command| match command {
                    InlineCommand::UpdateProgress(ProgressUpdate::Phase { operation: updated, phase }) => {
                        assert_eq!(updated, operation);
                        Some(phase)
                    }
                    _ => None,
                })
                .collect();
            let runtime_index = phases.iter().position(|phase| *phase == runtime_phase).unwrap();
            assert_eq!(
                phases[runtime_index + 1],
                if initial_text {
                    ProgressPhase::ReceivingResponse
                } else {
                    ProgressPhase::Processing
                },
                "settled runtime work must restore the model phase before more text"
            );
            assert_eq!(phases.last(), Some(&ProgressPhase::ReceivingResponse));
            assert!(latency.first_visible);
            drop(guard);
            assert!(handle.current_progress_operation().is_none());
        }
    }
}

#[tokio::test]
async fn completion_only_output_reaches_latency_and_harness_observers() {
    let (command_tx, _command_rx) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(command_tx);
    let guard = handle.begin_progress(ProgressPhase::WaitingForModel);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle, Default::default());
    let mut stream: uni::LLMStream = Box::pin(futures::stream::iter([Ok(completed_response("complete answer"))]));
    let mut latency = ResponseLatency::new(Some(guard.operation()), Instant::now(), 1, 1);
    let mut bridge = HarnessStreamingBridge::new(None, "turn-1", 1, 1);
    let mut phases = Vec::new();
    let mut progress = |event: StreamProgressEvent| {
        phases.push(latency.observe(&event));
        bridge.on_progress(event);
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
        StreamSpinnerOptions::default(),
        Some(&mut progress),
    )
    .await
    .unwrap();
    assert!(rendered);
    assert_eq!(phases, [ProgressPhase::Processing, ProgressPhase::ReceivingResponse]);
    assert!(latency.first_event);
    assert!(latency.first_visible, "completion rendering must record both visible-output latency metrics");
    assert!(bridge.assistant_output_observed());
}

#[test]
fn response_latency_distinguishes_hidden_activity_from_visible_text() {
    let mut latency = ResponseLatency::new(Some(ProgressOperation::start()), Instant::now(), 2, 3);
    assert_eq!(latency.observe(&StreamProgressEvent::ProviderActivity), ProgressPhase::Processing);
    assert!(latency.first_event);
    assert!(!latency.first_visible);
    assert_eq!(
        latency.observe(&StreamProgressEvent::ToolCallStarted {
            call_id: "call-1".into(),
            name: Some("read_file".into())
        }),
        ProgressPhase::Processing
    );
    assert!(!latency.first_visible);
    assert_eq!(latency.observe(&StreamProgressEvent::OutputDelta("  ".into())), ProgressPhase::Processing);
    assert!(!latency.first_visible);
    assert_eq!(
        latency.observe(&StreamProgressEvent::OutputDelta("answer".into())),
        ProgressPhase::ReceivingResponse
    );
    assert!(latency.first_visible);
    assert_eq!(latency.observe(&StreamProgressEvent::ProviderActivity), ProgressPhase::ReceivingResponse);
    assert_eq!(
        latency.observe(&StreamProgressEvent::OutputDelta("resumed".into())),
        ProgressPhase::ReceivingResponse
    );
}
