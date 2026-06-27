//! Temporary adapter from core-owned provider types to `vtcode-llm`.
//!
//! Delete this once `vtcode-core` no longer owns duplicate `LLMProvider`,
//! request, response, and stream-event types and can depend on the `vtcode-llm`
//! provider trait directly.

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};

use super::provider;
use super::provider::{
    LLMNormalizedStream, LLMProvider, LLMRequest, LLMStream, LLMStreamEvent, NormalizedStreamEvent,
};

pub(crate) struct VtcodeLlmProviderAdapter {
    inner: Box<dyn vtcode_llm::provider::LLMProvider>,
}

impl VtcodeLlmProviderAdapter {
    pub(crate) fn new(provider: impl vtcode_llm::provider::LLMProvider + 'static) -> Self {
        Self {
            inner: Box::new(provider),
        }
    }

    fn convert_request(
        request: &LLMRequest,
    ) -> Result<vtcode_llm::provider::LLMRequest, provider::LLMError> {
        let mut converted: vtcode_llm::provider::LLMRequest =
            convert_serializable(request, "LLM request")?;

        if let Some(tools) = &request.tools {
            converted.tools = Some(std::sync::Arc::new(convert_tools(tools)?));
        }

        Ok(converted)
    }

    fn convert_messages(
        messages: &[provider::Message],
    ) -> Result<Vec<vtcode_llm::provider::Message>, provider::LLMError> {
        convert_serializable(messages, "conversation history")
    }

    fn convert_options(
        options: &provider::ResponsesCompactionOptions,
    ) -> Result<vtcode_llm::provider::ResponsesCompactionOptions, provider::LLMError> {
        convert_serializable(options, "Responses compaction options")
    }
}

#[async_trait]
impl LLMProvider for VtcodeLlmProviderAdapter {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn backend_kind(&self) -> super::types::BackendKind {
        self.inner.backend_kind()
    }

    fn supports_streaming(&self) -> bool {
        self.inner.supports_streaming()
    }

    fn supports_non_streaming(&self, model: &str) -> bool {
        self.inner.supports_non_streaming(model)
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        self.inner.supports_reasoning(model)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        self.inner.supports_reasoning_effort(model)
    }

    fn supports_tools(&self, model: &str) -> bool {
        self.inner.supports_tools(model)
    }

    fn supports_parallel_tool_config(&self, model: &str) -> bool {
        self.inner.supports_parallel_tool_config(model)
    }

    fn supports_structured_output(&self, model: &str) -> bool {
        self.inner.supports_structured_output(model)
    }

    fn supports_context_caching(&self, model: &str) -> bool {
        self.inner.supports_context_caching(model)
    }

    fn supports_vision(&self, model: &str) -> bool {
        self.inner.supports_vision(model)
    }

    fn supports_responses_compaction(&self, model: &str) -> bool {
        self.inner.supports_responses_compaction(model)
    }

    fn supports_context_edits(&self, model: &str) -> bool {
        self.inner.supports_context_edits(model)
    }

    fn supports_manual_compaction(&self, model: &str) -> bool {
        self.inner.supports_manual_openai_compaction(model)
    }

    fn manual_compaction_unavailable_message(&self, model: &str) -> String {
        self.inner
            .manual_openai_compaction_unavailable_message(model)
    }

    fn effective_context_size(&self, model: &str) -> usize {
        self.inner.effective_context_size(model)
    }

    async fn compact_history(
        &self,
        model: &str,
        history: &[provider::Message],
    ) -> Result<Vec<provider::Message>, provider::LLMError> {
        let converted_history = Self::convert_messages(history)?;
        let compacted = self
            .inner
            .compact_history(model, &converted_history)
            .await?;
        convert_serializable(&compacted, "compacted history")
    }

    async fn compact_history_with_options(
        &self,
        model: &str,
        history: &[provider::Message],
        options: &provider::ResponsesCompactionOptions,
    ) -> Result<Vec<provider::Message>, provider::LLMError> {
        let converted_history = Self::convert_messages(history)?;
        let converted_options = Self::convert_options(options)?;
        let compacted = self
            .inner
            .compact_history_with_options(model, &converted_history, &converted_options)
            .await?;
        convert_serializable(&compacted, "compacted history")
    }

    async fn generate(
        &self,
        request: LLMRequest,
    ) -> Result<provider::LLMResponse, provider::LLMError> {
        self.inner.generate(Self::convert_request(&request)?).await
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, provider::LLMError> {
        let mut stream = self.inner.stream(Self::convert_request(&request)?).await?;
        let stream = async_stream::try_stream! {
            while let Some(event) = futures::StreamExt::next(&mut stream).await {
                yield convert_stream_event(event?);
            }
        };
        Ok(Box::pin(stream))
    }

    async fn stream_normalized(
        &self,
        request: LLMRequest,
    ) -> Result<LLMNormalizedStream, provider::LLMError> {
        let mut stream = self
            .inner
            .stream_normalized(Self::convert_request(&request)?)
            .await?;
        let stream = async_stream::try_stream! {
            while let Some(event) = futures::StreamExt::next(&mut stream).await {
                yield convert_normalized_stream_event(event?);
            }
        };
        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        self.inner.supported_models()
    }

    async fn get_balance(
        &self,
    ) -> Result<Option<vtcode_commons::llm::BalanceInfo>, provider::LLMError> {
        self.inner.get_balance().await
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), provider::LLMError> {
        self.inner
            .validate_request(&Self::convert_request(request)?)
    }
}

fn convert_serializable<T, U>(value: &T, context: &'static str) -> Result<U, provider::LLMError>
where
    T: Serialize + ?Sized,
    U: DeserializeOwned,
{
    let value = serde_json::to_value(value).map_err(|error| conversion_error(context, error))?;
    serde_json::from_value(value).map_err(|error| conversion_error(context, error))
}

fn convert_tools(
    tools: &[provider::ToolDefinition],
) -> Result<Vec<vtcode_llm::provider::ToolDefinition>, provider::LLMError> {
    tools
        .iter()
        .map(|tool| {
            let mut converted: vtcode_llm::provider::ToolDefinition =
                convert_serializable(tool, "tool definition")?;
            converted.hosted_tool_config = tool.hosted_tool_config.clone();
            Ok(converted)
        })
        .collect()
}

fn convert_stream_event(event: vtcode_llm::provider::LLMStreamEvent) -> LLMStreamEvent {
    match event {
        vtcode_llm::provider::LLMStreamEvent::Token { delta } => LLMStreamEvent::Token { delta },
        vtcode_llm::provider::LLMStreamEvent::Reasoning { delta } => {
            LLMStreamEvent::Reasoning { delta }
        }
        vtcode_llm::provider::LLMStreamEvent::ReasoningSignature { signature } => {
            LLMStreamEvent::ReasoningSignature { signature }
        }
        vtcode_llm::provider::LLMStreamEvent::ReasoningStage { stage } => {
            LLMStreamEvent::ReasoningStage { stage }
        }
        vtcode_llm::provider::LLMStreamEvent::Completed { response } => {
            LLMStreamEvent::Completed { response }
        }
    }
}

fn convert_normalized_stream_event(
    event: vtcode_llm::provider::NormalizedStreamEvent,
) -> NormalizedStreamEvent {
    match event {
        vtcode_llm::provider::NormalizedStreamEvent::TextDelta { delta } => {
            NormalizedStreamEvent::TextDelta { delta }
        }
        vtcode_llm::provider::NormalizedStreamEvent::ReasoningDelta { delta } => {
            NormalizedStreamEvent::ReasoningDelta { delta }
        }
        vtcode_llm::provider::NormalizedStreamEvent::ToolCallStart { call_id, name } => {
            NormalizedStreamEvent::ToolCallStart { call_id, name }
        }
        vtcode_llm::provider::NormalizedStreamEvent::ToolCallDelta { call_id, delta } => {
            NormalizedStreamEvent::ToolCallDelta { call_id, delta }
        }
        vtcode_llm::provider::NormalizedStreamEvent::Usage { usage } => {
            NormalizedStreamEvent::Usage { usage }
        }
        vtcode_llm::provider::NormalizedStreamEvent::Done { response } => {
            NormalizedStreamEvent::Done { response }
        }
    }
}

fn conversion_error(context: &'static str, error: serde_json::Error) -> provider::LLMError {
    provider::LLMError::Provider {
        message: format!("failed to bridge {context} to vtcode-llm provider types: {error}"),
        metadata: None,
    }
}
