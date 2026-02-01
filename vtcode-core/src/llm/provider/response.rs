use std::pin::Pin;

pub use vtcode_commons::llm::{FinishReason, LLMError, LLMErrorMetadata, LLMResponse, Usage};
use super::ToolCall;

#[derive(Debug, Clone)]
pub enum LLMStreamEvent {
    Token { delta: String },
    Reasoning { delta: String },
    Completed { response: Box<LLMResponse> },
}

pub type LLMStream = Pin<Box<dyn futures::Stream<Item = Result<LLMStreamEvent, LLMError>> + Send>>;
