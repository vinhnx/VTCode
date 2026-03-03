use super::ui_interaction::{
    PlaceholderSpinner, StreamProgressEvent, StreamSpinnerOptions, stream_and_render_response,
    stream_and_render_response_with_options, stream_and_render_response_with_options_and_progress,
};
use futures::stream;
use std::sync::Arc;
use tokio::sync::{Notify, mpsc};
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::{InlineCommand, InlineHandle};

#[derive(Clone)]
struct CompletedOnlyProvider {
    content: Option<String>,
    reasoning: Option<String>,
}

#[derive(Clone)]
struct ReasoningThenContentProvider {
    content: String,
    reasoning: String,
}

#[derive(Clone)]
struct StagedReasoningProvider {
    content: String,
    reasoning: String,
    stage: String,
}

#[derive(Clone)]
struct ReasoningThenChunkedContentProvider {
    chunks: Vec<String>,
    reasoning: String,
}

#[async_trait::async_trait]
impl uni::LLMProvider for CompletedOnlyProvider {
    fn name(&self) -> &str {
        "test-provider"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn generate(&self, _request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        Ok(uni::LLMResponse {
            content: self.content.clone(),
            model: "mock-model".to_string(),
            tool_calls: None,
            usage: None,
            finish_reason: uni::FinishReason::Stop,
            reasoning: self.reasoning.clone(),
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: vec![],
        })
    }

    async fn stream(&self, request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        let response = self.generate(request).await?;
        Ok(Box::pin(stream::once(async {
            Ok(uni::LLMStreamEvent::Completed {
                response: Box::new(response),
            })
        })))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["test-model".to_string()]
    }

    fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl uni::LLMProvider for ReasoningThenContentProvider {
    fn name(&self) -> &str {
        "test-provider"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn generate(&self, _request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        Ok(uni::LLMResponse {
            content: Some(self.content.clone()),
            model: "mock-model".to_string(),
            tool_calls: None,
            usage: None,
            finish_reason: uni::FinishReason::Stop,
            reasoning: Some(self.reasoning.clone()),
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: vec![],
        })
    }

    async fn stream(&self, request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        let response = self.generate(request).await?;
        Ok(Box::pin(stream::iter(vec![
            Ok(uni::LLMStreamEvent::Reasoning {
                delta: self.reasoning.clone(),
            }),
            Ok(uni::LLMStreamEvent::Token {
                delta: self.content.clone(),
            }),
            Ok(uni::LLMStreamEvent::Completed {
                response: Box::new(response),
            }),
        ])))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["test-model".to_string()]
    }

    fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl uni::LLMProvider for StagedReasoningProvider {
    fn name(&self) -> &str {
        "test-provider"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn generate(&self, _request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        Ok(uni::LLMResponse {
            content: Some(self.content.clone()),
            model: "mock-model".to_string(),
            tool_calls: None,
            usage: None,
            finish_reason: uni::FinishReason::Stop,
            reasoning: Some(self.reasoning.clone()),
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: vec![],
        })
    }

    async fn stream(&self, request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        let response = self.generate(request).await?;
        Ok(Box::pin(stream::iter(vec![
            Ok(uni::LLMStreamEvent::ReasoningStage {
                stage: self.stage.clone(),
            }),
            Ok(uni::LLMStreamEvent::Reasoning {
                delta: self.reasoning.clone(),
            }),
            Ok(uni::LLMStreamEvent::Token {
                delta: self.content.clone(),
            }),
            Ok(uni::LLMStreamEvent::Completed {
                response: Box::new(response),
            }),
        ])))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["test-model".to_string()]
    }

    fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl uni::LLMProvider for ReasoningThenChunkedContentProvider {
    fn name(&self) -> &str {
        "test-provider"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    async fn generate(&self, _request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        Ok(uni::LLMResponse {
            content: Some(self.chunks.concat()),
            model: "mock-model".to_string(),
            tool_calls: None,
            usage: None,
            finish_reason: uni::FinishReason::Stop,
            reasoning: Some(self.reasoning.clone()),
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: vec![],
        })
    }

    async fn stream(&self, request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        let response = self.generate(request).await?;
        let mut events = Vec::with_capacity(self.chunks.len() + 2);
        events.push(Ok(uni::LLMStreamEvent::Reasoning {
            delta: self.reasoning.clone(),
        }));
        for chunk in &self.chunks {
            events.push(Ok(uni::LLMStreamEvent::Token {
                delta: chunk.clone(),
            }));
        }
        events.push(Ok(uni::LLMStreamEvent::Completed {
            response: Box::new(response),
        }));
        Ok(Box::pin(stream::iter(events)))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["test-model".to_string()]
    }

    fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

fn build_request() -> uni::LLMRequest {
    uni::LLMRequest {
        messages: Vec::new(),
        model: "test-model".to_string(),
        stream: true,
        ..Default::default()
    }
}

fn build_spinner() -> PlaceholderSpinner {
    let (tx, _rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    PlaceholderSpinner::new(&handle, None, None, "")
}

#[tokio::test]
async fn renders_completed_only_content() {
    let provider = CompletedOnlyProvider {
        content: Some("hello world".to_string()),
        reasoning: None,
    };
    let request = build_request();
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = super::state::CtrlCState::new();
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

    assert!(
        emitted,
        "should mark emitted tokens when content is rendered"
    );
    assert_eq!(resp.content.as_deref(), Some("hello world"));
}

#[tokio::test]
async fn renders_reasoning_when_no_content() {
    let provider = CompletedOnlyProvider {
        content: None,
        reasoning: Some("because reason".to_string()),
    };
    let request = build_request();
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = super::state::CtrlCState::new();
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

    assert!(
        emitted,
        "should mark emitted tokens when reasoning is rendered"
    );
    assert!(resp.content.is_none(), "content should remain none");
}

#[tokio::test]
async fn keeps_proposed_plan_tags_visible_outside_plan_mode() {
    let provider = CompletedOnlyProvider {
        content: Some("<proposed_plan>\n- Step 1\n</proposed_plan>".to_string()),
        reasoning: None,
    };
    let request = build_request();
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = super::state::CtrlCState::new();
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

    assert!(
        emitted,
        "outside plan mode, proposed_plan blocks should remain visible"
    );
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
    let ctrl_c_state = super::state::CtrlCState::new();
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
        },
    )
    .await
    .expect("stream should succeed");

    assert!(
        !emitted,
        "when stripping is enabled, pure proposed_plan content should be hidden"
    );
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
    let ctrl_c_state = super::state::CtrlCState::new();
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

    assert!(
        emitted,
        "large token content should still be rendered after reasoning events"
    );
}

#[tokio::test]
async fn emits_progress_events_for_stream_deltas() {
    let provider = StagedReasoningProvider {
        content: "final content".to_string(),
        reasoning: "thinking".to_string(),
        stage: "analysis".to_string(),
    };
    let request = build_request();
    let spinner = build_spinner();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = super::state::CtrlCState::new();
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

    assert!(matches!(
        events.first(),
        Some(StreamProgressEvent::ReasoningStage(stage)) if stage == "analysis"
    ));
    assert!(events.iter().any(
        |event| matches!(event, StreamProgressEvent::ReasoningDelta(delta) if delta == "thinking")
    ));
    assert!(events.iter().any(
        |event| matches!(event, StreamProgressEvent::OutputDelta(delta) if delta == "final content")
    ));
}

#[tokio::test]
async fn inline_streams_small_content_deltas_after_reasoning() {
    let provider = ReasoningThenChunkedContentProvider {
        chunks: vec!["hello ".to_string(), "world".to_string()],
        reasoning: "thinking".to_string(),
    };
    let request = build_request();
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = super::state::CtrlCState::new();
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
            kind: vtcode_tui::InlineMessageKind::Agent,
            ..
        } = command
        {
            replace_last_agent_updates += 1;
        }
    }

    assert!(
        replace_last_agent_updates >= 2,
        "expected at least 2 incremental agent updates, got {}",
        replace_last_agent_updates
    );
}

#[tokio::test]
async fn inline_streams_reasoning_deltas_live() {
    let provider = ReasoningThenChunkedContentProvider {
        chunks: vec!["done".to_string()],
        reasoning: "thinking".to_string(),
    };
    let request = build_request();
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = super::state::CtrlCState::new();
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
            kind: vtcode_tui::InlineMessageKind::Policy,
            segment,
        } = command
            && segment.text.contains("thinking")
        {
            saw_reasoning_inline = true;
            break;
        }
    }

    assert!(
        saw_reasoning_inline,
        "expected inline reasoning delta to be streamed to TUI"
    );
}

#[tokio::test]
async fn inline_batches_many_token_deltas_for_performance() {
    let provider = ReasoningThenChunkedContentProvider {
        chunks: vec!["x".to_string(); 600],
        reasoning: "thinking".to_string(),
    };
    let request = build_request();
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = super::state::CtrlCState::new();
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
            kind: vtcode_tui::InlineMessageKind::Agent,
            ..
        } = command
        {
            replace_last_agent_updates += 1;
        }
    }

    assert!(
        replace_last_agent_updates < 300,
        "expected batched updates for 600 deltas, got {} replace operations",
        replace_last_agent_updates
    );
}
