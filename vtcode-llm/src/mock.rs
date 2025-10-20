//! Utilities for deterministic tests that need an `LLMClient` implementation
//! without performing network calls.
//!
//! Enable the crate's `mock` feature to access the [`StaticResponseClient`],
//! queue canned responses, and verify the interaction contract used by
//! downstream integrations.

use std::collections::VecDeque;

use async_trait::async_trait;
use vtcode_core::llm::client::LLMClient;
use vtcode_core::llm::types::{BackendKind, LLMError, LLMResponse};

/// Deterministic `LLMClient` that yields queued responses.
#[derive(Debug)]
pub struct StaticResponseClient {
    backend: BackendKind,
    model: String,
    queue: VecDeque<Result<LLMResponse, LLMError>>,
}

impl StaticResponseClient {
    /// Create a mock client for the provided model/backend combination.
    pub fn new(model: impl Into<String>, backend: BackendKind) -> Self {
        Self {
            backend,
            model: model.into(),
            queue: VecDeque::new(),
        }
    }

    /// Queue a successful response. Responses are returned in FIFO order.
    pub fn enqueue_response(&mut self, response: LLMResponse) {
        self.queue.push_back(Ok(response));
    }

    /// Queue a successful response and return the client for chaining.
    pub fn with_response(mut self, response: LLMResponse) -> Self {
        self.enqueue_response(response);
        self
    }

    /// Queue an error result. Errors are returned in FIFO order alongside responses.
    pub fn enqueue_error(&mut self, error: LLMError) {
        self.queue.push_back(Err(error));
    }

    /// Queue an error result and return the client for chaining.
    pub fn with_error(mut self, error: LLMError) -> Self {
        self.enqueue_error(error);
        self
    }

    /// Consume the client and return it as a boxed trait object.
    pub fn into_client(self) -> vtcode_core::llm::client::AnyClient {
        Box::new(self)
    }
}

#[async_trait]
impl LLMClient for StaticResponseClient {
    async fn generate(&mut self, _prompt: &str) -> Result<LLMResponse, LLMError> {
        self.queue.pop_front().unwrap_or_else(|| {
            Err(LLMError::InvalidRequest(
                "StaticResponseClient has no queued responses".to_string(),
            ))
        })
    }

    fn backend_kind(&self) -> BackendKind {
        self.backend.clone()
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::StaticResponseClient;
    use vtcode_core::llm::types::{BackendKind, LLMError, LLMResponse};

    #[test]
    fn returns_responses_in_fifo_order() {
        let response_one = LLMResponse {
            content: "first".to_string(),
            model: "test".to_string(),
            usage: None,
            reasoning: None,
        };
        let response_two = LLMResponse {
            content: "second".to_string(),
            model: "test".to_string(),
            usage: None,
            reasoning: None,
        };

        let mut client = StaticResponseClient::new("test", BackendKind::OpenAI);
        client.enqueue_response(response_one.clone());
        client.enqueue_response(response_two.clone());

        let first = futures::executor::block_on(client.generate("prompt")).unwrap();
        let second = futures::executor::block_on(client.generate("prompt")).unwrap();

        assert_eq!(first.content, response_one.content);
        assert_eq!(second.content, response_two.content);
    }

    #[test]
    fn errors_when_queue_is_empty() {
        let mut client = StaticResponseClient::new("test", BackendKind::Gemini);
        let error = futures::executor::block_on(client.generate("prompt"))
            .expect_err("expected error when queue is empty");

        assert!(matches!(error, LLMError::InvalidRequest(_)));
    }
}
