//! Gemini API compatibility facade for VT Code.
//!
//! Canonical internal imports live under `crate::llm::providers::gemini::wire`.

pub use crate::llm::providers::gemini::wire::client;
pub use crate::llm::providers::gemini::wire::function_calling;
pub use crate::llm::providers::gemini::wire::models;
pub use crate::llm::providers::gemini::wire::streaming;
pub use crate::llm::providers::gemini::wire::{
    Candidate, Client, ClientConfig, Content, FunctionCall, FunctionCallingConfig,
    FunctionDeclaration, FunctionResponse, GenerateContentRequest, GenerateContentResponse, Part,
    RetryConfig, StreamingCandidate, StreamingConfig, StreamingError, StreamingMetrics,
    StreamingProcessor, StreamingResponse, Tool, ToolConfig,
};
