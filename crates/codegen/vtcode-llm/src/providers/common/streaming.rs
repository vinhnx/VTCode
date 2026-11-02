//! OpenAI-compatible stream task ownership and dispatch.

use crate::provider::{LLMError, LLMStream, LLMStreamEvent};

/// Spawns an OpenAI-compatible streaming response handler.
/// Returns an `LLMStream` backed by a tokio task that processes chunks via
/// `process_openai_stream` with the `handle_openai_compatible_chunk` handler.
///
/// Providers with custom chunk handling (e.g., DeepSeek reasoning extraction)
/// should use the lower-level `process_openai_stream` directly.
pub(crate) fn spawn_openai_compatible_stream(
    response: reqwest::Response,
    provider_name: &'static str,
    model: String,
    reasoning_fields: &'static [&'static str],
    delta_order: crate::providers::shared::OpenAiDeltaOrder,
    include_cache_metrics: bool,
) -> LLMStream {
    use async_stream::try_stream;

    let bytes_stream = response.bytes_stream();
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<Result<LLMStreamEvent, LLMError>>();
    let tx = event_tx.clone();

    // Timeout for the entire streaming task (5 minutes).
    // Prevents indefinite hangs when upstream server stops responding.
    let stream_timeout = std::time::Duration::from_secs(300);
    let handle = tokio::spawn(async move {
        let aggregator_model = model.clone();
        let mut aggregator = crate::providers::shared::StreamAggregator::new(aggregator_model);

        let result = tokio::time::timeout(
            stream_timeout,
            crate::providers::shared::process_openai_stream(bytes_stream, provider_name, model, |value| {
                crate::providers::shared::handle_openai_compatible_chunk(
                    &value,
                    &mut aggregator,
                    &tx,
                    reasoning_fields,
                    delta_order,
                    include_cache_metrics,
                );
                Ok(())
            }),
        )
        .await;

        match result {
            Ok(Ok(_)) => {
                let response = aggregator.finalize();
                let _ = tx.send(Ok(LLMStreamEvent::Completed { response: Box::new(response) }));
            }
            Ok(Err(err)) => {
                let _ = tx.send(Err(err));
            }
            Err(_elapsed) => {
                let _ = tx.send(Err(LLMError::Provider {
                    message: format!("{provider_name}: streaming timed out after 5 minutes"),
                    metadata: None,
                }));
            }
        }
    });

    let stream = try_stream! {
        let mut receiver = event_rx;
        // Abort the stream task when the consumer drops the stream: a client
        // disconnect would otherwise leave it streaming for the full 5-minute
        // timeout, wasting network and memory. Aborting an already-finished
        // task is a no-op.
        let _abort_guard = vtcode_commons::TaskGuard::with_label(handle, "openai-compatible-stream");
        while let Some(event) = receiver.recv().await {
            yield event?;
        }
    };

    Box::pin(stream)
}
