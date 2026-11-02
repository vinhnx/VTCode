pub(super) use super::*;
pub(super) use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;
pub(super) use std::collections::VecDeque;
pub(super) use std::sync::{Arc, Mutex};
pub(super) use tempfile::TempDir;
pub(super) use vtcode_core::config::map_prompt_cache_key_for_provider;

struct ScriptedProvider {
    provider_name: &'static str,
    supports_responses_compaction: bool,
    supports_streaming: bool,
    recorded_previous_response_ids: Arc<Mutex<Vec<Option<String>>>>,
    outcomes: Mutex<VecDeque<ScriptedProviderOutcome>>,
    recorded_messages: Option<Arc<Mutex<Vec<Vec<uni::Message>>>>>,
    resume_wake: Option<ResumeWake>,
}

struct ResumeWake {
    notify: Arc<tokio::sync::Notify>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

enum ScriptedProviderOutcome {
    Success {
        content: Option<&'static str>,
        request_id: Option<&'static str>,
    },
    Error(uni::LLMError),
}

impl ScriptedProvider {
    fn new(
        provider_name: &'static str,
        supports_responses_compaction: bool,
        recorded_previous_response_ids: Arc<Mutex<Vec<Option<String>>>>,
        outcomes: Vec<ScriptedProviderOutcome>,
    ) -> Self {
        Self {
            provider_name,
            supports_responses_compaction,
            supports_streaming: false,
            recorded_previous_response_ids,
            outcomes: Mutex::new(VecDeque::from(outcomes)),
            recorded_messages: None,
            resume_wake: None,
        }
    }

    fn with_recorded_messages(mut self, messages: Arc<Mutex<Vec<Vec<uni::Message>>>>) -> Self {
        self.recorded_messages = Some(messages);
        self
    }

    fn with_streaming(mut self, supports_streaming: bool) -> Self {
        self.supports_streaming = supports_streaming;
        self
    }
}

#[async_trait::async_trait]
impl uni::LLMProvider for ScriptedProvider {
    fn name(&self) -> &str {
        self.provider_name
    }

    fn supports_streaming(&self) -> bool {
        self.supports_streaming
    }

    fn supports_responses_compaction(&self, _model: &str) -> bool {
        self.supports_responses_compaction
    }

    async fn generate(&self, request: uni::LLMRequest) -> std::result::Result<uni::LLMResponse, uni::LLMError> {
        self.recorded_previous_response_ids
            .lock()
            .expect("previous_response_id recorder")
            .push(request.previous_response_id.clone());

        if let Some(messages) = &self.recorded_messages {
            messages
                .lock()
                .expect("request recorder")
                .push(request.messages.as_ref().clone());
        }

        if let Some(wake) = &self.resume_wake {
            wake.entered.notify_one();
            wake.notify.notify_one();
            wake.release.notified().await;
        }

        match self
            .outcomes
            .lock()
            .expect("provider script")
            .pop_front()
            .expect("provider script should have enough outcomes")
        {
            ScriptedProviderOutcome::Success { content, request_id } => Ok(uni::LLMResponse {
                content: content.map(str::to_string),
                model: "noop-model".to_string(),
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: request_id.map(str::to_string),
                tool_references: Vec::new(),
                compaction: None,
            }),
            ScriptedProviderOutcome::Error(error) => Err(error),
        }
    }

    async fn stream(&self, _request: uni::LLMRequest) -> std::result::Result<uni::LLMStream, uni::LLMError> {
        Err(uni::LLMError::Provider {
            message: "LLM first token timed out after 1 seconds".to_string(),
            metadata: None,
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["noop-model".to_string()]
    }

    fn validate_request(&self, _request: &uni::LLMRequest) -> std::result::Result<(), uni::LLMError> {
        Ok(())
    }
}

mod prompt_cache;
mod retry_errors;
mod retry_timeouts;
mod stale_misconfig;
mod streaming_capacity;
