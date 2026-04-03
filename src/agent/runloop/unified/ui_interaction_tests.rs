use super::status_line::InputStatusState;
use super::ui_interaction::{
    PlaceholderSpinner, StreamProgressEvent, StreamSpinnerOptions, start_loading_status,
    stream_and_render_response, stream_and_render_response_with_options,
    stream_and_render_response_with_options_and_progress,
};
use futures::stream;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, mpsc};
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::app::{InlineCommand, InlineHandle};

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
    reasoning_chunks: Vec<String>,
}

#[derive(Clone)]
struct NormalizedToolCallProvider {
    content: String,
    tool_name: String,
    tool_call_id: String,
    tool_argument_chunks: Vec<String>,
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
            reasoning: Some(self.reasoning_chunks.concat()),
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: vec![],
        })
    }

    async fn stream(&self, request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        let response = self.generate(request).await?;
        let mut events = Vec::with_capacity(self.reasoning_chunks.len() + self.chunks.len() + 1);
        for chunk in &self.reasoning_chunks {
            events.push(Ok(uni::LLMStreamEvent::Reasoning {
                delta: chunk.clone(),
            }));
        }
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

#[async_trait::async_trait]
impl uni::LLMProvider for NormalizedToolCallProvider {
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
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: vec![],
        })
    }

    async fn stream(&self, _request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        Err(uni::LLMError::Provider {
            message: "legacy stream should not be used in this test".to_string(),
            metadata: None,
        })
    }

    async fn stream_normalized(
        &self,
        request: uni::LLMRequest,
    ) -> Result<uni::LLMNormalizedStream, uni::LLMError> {
        let response = self.generate(request).await?;
        let mut events = Vec::with_capacity(self.tool_argument_chunks.len() + 2);
        events.push(Ok(uni::NormalizedStreamEvent::ToolCallStart {
            call_id: self.tool_call_id.clone(),
            name: Some(self.tool_name.clone()),
        }));
        for chunk in &self.tool_argument_chunks {
            events.push(Ok(uni::NormalizedStreamEvent::ToolCallDelta {
                call_id: self.tool_call_id.clone(),
                delta: chunk.clone(),
            }));
        }
        events.push(Ok(uni::NormalizedStreamEvent::TextDelta {
            delta: self.content.clone(),
        }));
        events.push(Ok(uni::NormalizedStreamEvent::Done {
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
async fn placeholder_spinner_restores_previous_input_status() {
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    handle.set_input_status(Some("provider".to_string()), Some("model".to_string()));

    {
        let spinner = PlaceholderSpinner::new(
            &handle,
            Some("provider".to_string()),
            Some("model".to_string()),
            "Loading model lists...",
        );
        spinner.finish();
    }

    let mut last_status: Option<(Option<String>, Option<String>)> = None;
    while let Ok(command) = rx.try_recv() {
        if let InlineCommand::SetInputStatus { left, right } = command {
            last_status = Some((left, right));
        }
    }

    assert_eq!(
        last_status,
        Some((Some("provider".to_string()), Some("model".to_string())))
    );
}

#[tokio::test]
async fn start_loading_status_uses_current_input_status_for_restore() {
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let state = InputStatusState {
        left: Some("provider".to_string()),
        right: Some("model".to_string()),
        ..Default::default()
    };

    {
        let spinner = start_loading_status(&handle, &state, "Saving memory note...");
        spinner.finish();
    }

    let mut saw_loading = false;
    let mut last_status: Option<(Option<String>, Option<String>)> = None;
    while let Ok(command) = rx.try_recv() {
        if let InlineCommand::SetInputStatus { left, right } = command {
            if left
                .as_deref()
                .map(|text| text.contains("Saving memory note"))
                .unwrap_or(false)
            {
                saw_loading = true;
            }
            last_status = Some((left, right));
        }
    }

    assert!(saw_loading);
    assert_eq!(
        last_status,
        Some((Some("provider".to_string()), Some("model".to_string())))
    );
}

#[tokio::test]
async fn placeholder_spinner_applies_message_updates_before_next_tick() {
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::with_progress(&handle, None, None, "Loading...", None);

    let _ = rx.recv().await;
    spinner.update_message("Still working");

    let updated_status = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            match rx.recv().await {
                Some(InlineCommand::SetInputStatus { left, .. })
                    if left
                        .as_deref()
                        .map(|text| text.contains("Still working"))
                        .unwrap_or(false) =>
                {
                    return left;
                }
                Some(_) => continue,
                None => panic!("spinner status channel closed unexpectedly"),
            }
        }
    })
    .await
    .expect("spinner message update should not wait for the polling interval");

    spinner.finish();

    assert!(
        updated_status
            .as_deref()
            .map(|text| text.contains("Still working"))
            .unwrap_or(false)
    );
}

#[tokio::test]
async fn placeholder_spinner_uses_latest_message_update() {
    let (tx, mut rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::with_progress(&handle, None, None, "Loading...", None);

    let _ = rx.recv().await;
    spinner.update_message("First update");
    spinner.update_message("Final update");

    let updated_status = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            match rx.recv().await {
                Some(InlineCommand::SetInputStatus { left, .. })
                    if left
                        .as_deref()
                        .map(|text| text.contains("Final update"))
                        .unwrap_or(false) =>
                {
                    return left;
                }
                Some(_) => continue,
                None => panic!("spinner status channel closed unexpectedly"),
            }
        }
    })
    .await
    .expect("spinner should surface the latest message update");

    spinner.finish();

    assert!(
        updated_status
            .as_deref()
            .map(|text| text.contains("Final update"))
            .unwrap_or(false)
    );
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
    let (tx, _rx) = mpsc::unbounded_channel::<InlineCommand>();
    let handle = InlineHandle::new_for_tests(tx);
    let spinner = PlaceholderSpinner::new(&handle, None, None, "");
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
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

    assert!(events.iter().any(
        |event| matches!(event, StreamProgressEvent::ReasoningDelta(delta) if delta == "thinking")
    ));
    assert!(events.iter().any(
        |event| matches!(event, StreamProgressEvent::OutputDelta(delta) if delta == "final content")
    ));
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
    assert!(events.iter().any(
        |event| matches!(event, StreamProgressEvent::OutputDelta(delta) if delta == "final content")
    ));
}

#[tokio::test]
async fn suppresses_harmony_tool_call_wire_text_from_output_deltas() {
    let provider = ReasoningThenChunkedContentProvider {
        chunks: vec![
            "<|start|>assistant<|channel|>commentary to=functions.unified_search <|constrain|>json<|message|>{\"pattern\":\"runloop\"}<|call|>".to_string(),
            "<|start|>assistant<|channel|>final<|message|>safe answer<|end|>".to_string(),
        ],
        reasoning_chunks: vec![],
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

    let output_deltas: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            StreamProgressEvent::OutputDelta(delta) => Some(delta.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(output_deltas, vec!["safe answer"]);
}

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
    assert!(events.iter().any(
        |event| matches!(event, StreamProgressEvent::OutputDelta(delta) if delta == "final content")
    ));
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
            kind: vtcode_tui::app::InlineMessageKind::Agent,
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
        reasoning_chunks: vec!["thinking".to_string()],
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
            kind: vtcode_tui::app::InlineMessageKind::Policy,
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

    let mut reasoning_occurrences = 0usize;
    while let Ok(command) = rx.try_recv() {
        let text = match command {
            InlineCommand::AppendLine { segments, .. } => segments
                .into_iter()
                .map(|segment| segment.text)
                .collect::<String>(),
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

    assert_eq!(
        reasoning_occurrences, 1,
        "expected live reasoning to render once, without replay at completion"
    );
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
            kind: vtcode_tui::app::InlineMessageKind::Agent,
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

    let mut policy_inline_updates = 0usize;
    while let Ok(command) = rx.try_recv() {
        if let InlineCommand::Inline {
            kind: vtcode_tui::app::InlineMessageKind::Policy,
            ..
        } = command
        {
            policy_inline_updates += 1;
        }
    }

    assert!(
        policy_inline_updates < 300,
        "expected batched reasoning updates for 600 deltas, got {} inline updates",
        policy_inline_updates
    );
}
