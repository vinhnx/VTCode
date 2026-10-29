pub(super) use super::status_line::InputStatusState;
pub(super) use super::ui_interaction::{
    PlaceholderSpinner, StreamProgressEvent, StreamSpinnerOptions, start_loading_status, stream_and_render_response,
    stream_and_render_response_with_options, stream_and_render_response_with_options_and_progress,
};
pub(super) use futures::stream;
pub(super) use std::sync::Arc;
pub(super) use std::time::Duration;
pub(super) use tokio::sync::{Notify, mpsc};
pub(super) use vtcode_core::llm::provider as uni;
pub(super) use vtcode_core::utils::ansi::AnsiRenderer;
pub(super) use vtcode_ui::tui::app::{InlineCommand, InlineHandle};

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
            compaction: None,
        })
    }

    async fn stream(&self, request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        let response = self.generate(request).await?;
        Ok(Box::pin(stream::once(async {
            Ok(uni::LLMStreamEvent::Completed { response: Box::new(response) })
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
            compaction: None,
        })
    }

    async fn stream(&self, request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        let response = self.generate(request).await?;
        Ok(Box::pin(stream::iter(vec![
            Ok(uni::LLMStreamEvent::Reasoning { delta: self.reasoning.clone() }),
            Ok(uni::LLMStreamEvent::Token { delta: self.content.clone() }),
            Ok(uni::LLMStreamEvent::Completed { response: Box::new(response) }),
        ])))
    }

    async fn stream_normalized(&self, request: uni::LLMRequest) -> Result<uni::LLMNormalizedStream, uni::LLMError> {
        let response = self.generate(request).await?;
        Ok(Box::pin(stream::iter(vec![
            Ok(uni::NormalizedStreamEvent::ReasoningDelta {
                delta: self.reasoning.clone(),
                source: uni::ReasoningSource::ProviderSummary,
            }),
            Ok(uni::NormalizedStreamEvent::TextDelta { delta: self.content.clone() }),
            Ok(uni::NormalizedStreamEvent::Done { response: Box::new(response) }),
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
            compaction: None,
        })
    }

    async fn stream(&self, request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        let response = self.generate(request).await?;
        Ok(Box::pin(stream::iter(vec![
            Ok(uni::LLMStreamEvent::ReasoningStage { stage: self.stage.clone() }),
            Ok(uni::LLMStreamEvent::Reasoning { delta: self.reasoning.clone() }),
            Ok(uni::LLMStreamEvent::Token { delta: self.content.clone() }),
            Ok(uni::LLMStreamEvent::Completed { response: Box::new(response) }),
        ])))
    }

    async fn stream_normalized(&self, request: uni::LLMRequest) -> Result<uni::LLMNormalizedStream, uni::LLMError> {
        let response = self.generate(request).await?;
        Ok(Box::pin(stream::iter(vec![
            Ok(uni::NormalizedStreamEvent::ReasoningStage { stage: self.stage.clone() }),
            Ok(uni::NormalizedStreamEvent::ReasoningDelta {
                delta: self.reasoning.clone(),
                source: uni::ReasoningSource::ProviderSummary,
            }),
            Ok(uni::NormalizedStreamEvent::TextDelta { delta: self.content.clone() }),
            Ok(uni::NormalizedStreamEvent::Done { response: Box::new(response) }),
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
            compaction: None,
        })
    }

    async fn stream(&self, request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        let response = self.generate(request).await?;
        let mut events = Vec::with_capacity(self.reasoning_chunks.len() + self.chunks.len() + 1);
        for chunk in &self.reasoning_chunks {
            events.push(Ok(uni::LLMStreamEvent::Reasoning { delta: chunk.clone() }));
        }
        for chunk in &self.chunks {
            events.push(Ok(uni::LLMStreamEvent::Token { delta: chunk.clone() }));
        }
        events.push(Ok(uni::LLMStreamEvent::Completed { response: Box::new(response) }));
        Ok(Box::pin(stream::iter(events)))
    }

    async fn stream_normalized(&self, request: uni::LLMRequest) -> Result<uni::LLMNormalizedStream, uni::LLMError> {
        let response = self.generate(request).await?;
        let mut events = Vec::with_capacity(self.reasoning_chunks.len() + self.chunks.len() + 1);
        for chunk in &self.reasoning_chunks {
            events.push(Ok(uni::NormalizedStreamEvent::ReasoningDelta {
                delta: chunk.clone(),
                source: uni::ReasoningSource::ProviderSummary,
            }));
        }
        for chunk in &self.chunks {
            events.push(Ok(uni::NormalizedStreamEvent::TextDelta { delta: chunk.clone() }));
        }
        events.push(Ok(uni::NormalizedStreamEvent::Done { response: Box::new(response) }));
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
            compaction: None,
        })
    }

    async fn stream(&self, _request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        Err(uni::LLMError::Provider {
            message: "legacy stream should not be used in this test".to_string(),
            metadata: None,
        })
    }

    async fn stream_normalized(&self, request: uni::LLMRequest) -> Result<uni::LLMNormalizedStream, uni::LLMError> {
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
        events.push(Ok(uni::NormalizedStreamEvent::TextDelta { delta: self.content.clone() }));
        events.push(Ok(uni::NormalizedStreamEvent::Done { response: Box::new(response) }));
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
        messages: Arc::new(Vec::new()),
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

mod inline;
mod reasoning_render;
mod spinner;
mod stream_suppress;
