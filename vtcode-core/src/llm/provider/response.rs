use std::pin::Pin;

pub use vtcode_commons::llm::{FinishReason, LLMError, LLMResponse, Usage};

#[derive(Debug, Clone)]
pub enum LLMStreamEvent {
    Token { delta: String },
    Reasoning { delta: String },
    ReasoningSignature { signature: String },
    ReasoningStage { stage: String },
    Completed { response: Box<LLMResponse> },
}

#[derive(Debug, Clone)]
pub enum NormalizedStreamEvent {
    TextDelta {
        delta: String,
    },
    ReasoningDelta {
        delta: String,
    },
    ToolCallStart {
        call_id: String,
        name: Option<String>,
    },
    ToolCallDelta {
        call_id: String,
        delta: String,
    },
    Usage {
        usage: Usage,
    },
    Done {
        response: Box<LLMResponse>,
    },
}

pub type LLMStream = Pin<Box<dyn futures::Stream<Item = Result<LLMStreamEvent, LLMError>> + Send>>;
pub type BorrowedLLMStream<'a> =
    Pin<Box<dyn futures::Stream<Item = Result<LLMStreamEvent, LLMError>> + Send + 'a>>;
pub type LLMNormalizedStream =
    Pin<Box<dyn futures::Stream<Item = Result<NormalizedStreamEvent, LLMError>> + Send>>;

impl LLMStreamEvent {
    pub fn into_normalized(self) -> Vec<NormalizedStreamEvent> {
        match self {
            Self::Token { delta } => vec![NormalizedStreamEvent::TextDelta { delta }],
            Self::Reasoning { delta } => vec![NormalizedStreamEvent::ReasoningDelta { delta }],
            Self::ReasoningSignature { .. } => Vec::new(),
            Self::ReasoningStage { .. } => Vec::new(),
            Self::Completed { response } => {
                let mut events = Vec::new();
                if let Some(usage) = response.usage.clone() {
                    events.push(NormalizedStreamEvent::Usage { usage });
                }
                events.push(NormalizedStreamEvent::Done { response });
                events
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FinishReason, LLMResponse, LLMStreamEvent, NormalizedStreamEvent, Usage};

    #[test]
    fn completed_event_emits_usage_before_done() {
        let events = LLMStreamEvent::Completed {
            response: Box::new(LLMResponse {
                content: Some("done".to_string()),
                model: "gpt-5.4".to_string(),
                tool_calls: None,
                usage: Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                    cached_prompt_tokens: None,
                    cache_creation_tokens: None,
                    cache_read_tokens: None,
                }),
                finish_reason: FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
            }),
        }
        .into_normalized();

        assert!(matches!(
            events.first(),
            Some(NormalizedStreamEvent::Usage { .. })
        ));
        assert!(matches!(
            events.last(),
            Some(NormalizedStreamEvent::Done { .. })
        ));
    }

    #[test]
    fn token_event_maps_to_text_delta() {
        let events = LLMStreamEvent::Token {
            delta: "hello".to_string(),
        }
        .into_normalized();

        assert!(matches!(
            events.as_slice(),
            [NormalizedStreamEvent::TextDelta { delta }] if delta == "hello"
        ));
    }
}
