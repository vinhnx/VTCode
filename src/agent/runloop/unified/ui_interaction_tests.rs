use super::ui_interaction::{
    PlaceholderSpinner, StreamSpinnerOptions, stream_and_render_response,
    stream_and_render_response_with_options,
};
use futures::stream;
use std::sync::Arc;
use tokio::sync::{Notify, mpsc};
use vtcode_core::llm::provider as uni;
use vtcode_core::ui::tui::{InlineCommand, InlineHandle};
use vtcode_core::utils::ansi::AnsiRenderer;

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

fn build_request() -> uni::LLMRequest {
    uni::LLMRequest {
        messages: Vec::new(),
        system_prompt: None,
        tools: None,
        model: "test-model".to_string(),
        max_tokens: None,
        temperature: None,
        stream: true,
        output_format: None,
        tool_choice: None,
        parallel_tool_calls: None,
        parallel_tool_config: None,
        reasoning_effort: None,
        effort: None,
        verbosity: None,
        do_sample: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        stop_sequences: None,
        thinking_budget: None,
        betas: None,
        context_management: None,
        prefill: None,
        character_reinforcement: false,
        character_name: None,
        coding_agent_settings: None,
        metadata: None,
        prompt_cache_key: None,
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
