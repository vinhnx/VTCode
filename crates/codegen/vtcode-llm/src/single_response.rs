#[cfg(feature = "profiling")]
use super::provider::LLMNormalizedStream;
use super::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, NormalizedStreamEvent};
use futures::StreamExt as _;

#[cfg_attr(feature = "profiling", hotpath::measure)]
#[cfg_attr(
    feature = "profiling",
    allow(
        unsafe_code,
        reason = "hotpath `label =` emits an `#[unsafe(export_name)]` marker so duplicate labels fail to link."
    )
)]
pub async fn collect_single_response(
    provider: &(impl LLMProvider + ?Sized),
    request: LLMRequest,
) -> Result<LLMResponse, LLMError> {
    if provider.supports_non_streaming(&request.model) {
        #[cfg(feature = "profiling")]
        return hotpath::future!(provider.generate(request), label = "llm_non_streaming").await;
        #[cfg(not(feature = "profiling"))]
        return provider.generate(request).await;
    }

    // `stream_normalized` is the fallback transport for this request, so make
    // the selected wire mode explicit for adapters that inspect the request
    // instead of unconditionally setting it in their transport layer.
    let mut request = request;
    request.stream = true;

    #[cfg(feature = "profiling")]
    let mut stream: LLMNormalizedStream =
        hotpath::future!(provider.stream_normalized(request), label = "llm_streaming").await?;
    #[cfg(not(feature = "profiling"))]
    let mut stream = provider.stream_normalized(request).await?;
    let mut streamed_content = String::with_capacity(4096);
    let mut streamed_reasoning = String::with_capacity(1024);
    let mut streamed_usage = None;
    let mut completed = None;

    while let Some(event) = stream.next().await {
        match event? {
            NormalizedStreamEvent::TextDelta { delta } => streamed_content.push_str(&delta),
            NormalizedStreamEvent::ReasoningDelta { delta, source } if source.is_public_summary() => {
                streamed_reasoning.push_str(&delta)
            }
            NormalizedStreamEvent::ReasoningDelta { .. } => {}
            NormalizedStreamEvent::ReasoningStage { .. } => {}
            NormalizedStreamEvent::ToolCallStart { .. } | NormalizedStreamEvent::ToolCallDelta { .. } => {}
            NormalizedStreamEvent::Usage { usage } => streamed_usage = Some(usage),
            NormalizedStreamEvent::Done { response } => {
                completed = Some(*response);
                break;
            }
        }
    }

    let mut response = completed.ok_or_else(|| LLMError::Provider {
        message: format!("{} stream ended without a completed response", provider.name()),
        metadata: None,
    })?;
    if response.usage.is_none() {
        response.usage = streamed_usage;
    }
    if response.content.as_deref().unwrap_or_default().is_empty() && !streamed_content.is_empty() {
        response.content = Some(streamed_content);
    }
    if response.reasoning.is_none() && !streamed_reasoning.is_empty() {
        response.reasoning = Some(streamed_reasoning);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::collect_single_response;
    use crate::provider::{
        FinishReason, LLMError, LLMNormalizedStream, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
        NormalizedStreamEvent, ReasoningSource, Usage,
    };
    use async_trait::async_trait;
    use futures::stream;

    #[derive(Clone)]
    struct StreamingOnlyProvider;

    #[async_trait]
    impl LLMProvider for StreamingOnlyProvider {
        fn name(&self) -> &str {
            "test"
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        fn supports_non_streaming(&self, _model: &str) -> bool {
            false
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            panic!("generate should not be called for streaming-only provider")
        }

        async fn stream(&self, _request: LLMRequest) -> Result<LLMStream, LLMError> {
            Ok(Box::pin(stream::iter(vec![
                Ok(LLMStreamEvent::Token { delta: "hello ".to_string() }),
                Ok(LLMStreamEvent::Token { delta: "world".to_string() }),
                Ok(LLMStreamEvent::Completed {
                    response: Box::new(LLMResponse {
                        content: None,
                        model: "gpt-5.2-codex".to_string(),
                        tool_calls: None,
                        usage: None,
                        finish_reason: FinishReason::Stop,
                        reasoning: None,
                        reasoning_details: None,
                        organization_id: None,
                        request_id: None,
                        tool_references: Vec::new(),
                        compaction: None,
                    }),
                }),
            ])))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["gpt-5.2-codex".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn collect_single_response_uses_stream_when_non_streaming_is_unsupported() {
        let provider = StreamingOnlyProvider;
        let response = collect_single_response(
            &provider,
            LLMRequest {
                model: "gpt-5.2-codex".to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("stream collection should succeed");

        assert_eq!(response.content.as_deref(), Some("hello world"));
    }

    #[derive(Clone)]
    struct NormalizedOnlyProvider;

    #[async_trait]
    impl LLMProvider for NormalizedOnlyProvider {
        fn name(&self) -> &str {
            "test"
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        fn supports_non_streaming(&self, _model: &str) -> bool {
            false
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            panic!("generate should not be called for streaming-only provider")
        }

        async fn stream(&self, _request: LLMRequest) -> Result<LLMStream, LLMError> {
            panic!("legacy stream should not be used when normalized stream is available")
        }

        async fn stream_normalized(&self, _request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
            Ok(Box::pin(stream::iter(vec![
                Ok(NormalizedStreamEvent::TextDelta { delta: "hello ".to_string() }),
                Ok(NormalizedStreamEvent::ReasoningDelta {
                    delta: "thinking ".to_string(),
                    source: ReasoningSource::ProviderSummary,
                }),
                Ok(NormalizedStreamEvent::Usage {
                    usage: Usage {
                        prompt_tokens: 10,
                        completion_tokens: 2,
                        total_tokens: 12,
                        cached_prompt_tokens: None,
                        cache_creation_tokens: None,
                        cache_read_tokens: None,
                        iterations: None,
                    },
                }),
                Ok(NormalizedStreamEvent::Done {
                    response: Box::new(LLMResponse {
                        content: None,
                        model: "gpt-5.2-codex".to_string(),
                        tool_calls: None,
                        usage: None,
                        finish_reason: FinishReason::Stop,
                        reasoning: None,
                        reasoning_details: None,
                        organization_id: None,
                        request_id: None,
                        tool_references: Vec::new(),
                        compaction: None,
                    }),
                }),
            ])))
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["gpt-5.2-codex".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn collect_single_response_prefers_normalized_stream() {
        let provider = NormalizedOnlyProvider;
        let response = collect_single_response(
            &provider,
            LLMRequest {
                model: "gpt-5.2-codex".to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("normalized stream collection should succeed");

        assert_eq!(response.content.as_deref(), Some("hello "));
        assert_eq!(response.reasoning.as_deref(), Some("thinking "));
        assert_eq!(response.usage.as_ref().map(|usage| usage.total_tokens), Some(12));
    }
}
