#[cfg(feature = "profiling")]
use super::provider::LLMNormalizedStream;
use super::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, NormalizedStreamEvent};
use futures::StreamExt as _;

#[cfg_attr(feature = "profiling", hotpath::measure)]
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
            NormalizedStreamEvent::ReasoningDelta { delta } => streamed_reasoning.push_str(&delta),
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
