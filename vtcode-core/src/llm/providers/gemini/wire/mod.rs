//! Gemini wire types and transport helpers owned by the provider tree.

pub mod client;
pub mod function_calling;
pub mod models;
pub mod streaming;

pub use client::{Client, ClientConfig, RetryConfig};
pub use function_calling::{FunctionCall, FunctionCallingConfig, FunctionResponse};
pub use models::request::{GenerationConfig, ThinkingConfig};
pub use models::{
    Candidate, Content, FunctionDeclaration, GenerateContentRequest, GenerateContentResponse,
    InlineData, Part, SystemInstruction, Tool, ToolConfig,
};
pub use streaming::{
    StreamingCandidate, StreamingConfig, StreamingError, StreamingMetrics, StreamingProcessor,
    StreamingResponse,
};
