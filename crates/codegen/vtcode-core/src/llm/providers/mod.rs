//! LLM provider implementations.
//!
//! All provider implementations live in [`vtcode_llm::providers`]. This module
//! re-exports them so the historical `crate::llm::providers::*` import paths
//! continue to resolve throughout `vtcode-core`.

pub use vtcode_llm::providers::reasoning::{
    ReasoningBuffer, ReasoningSegment, extract_reasoning_trace, split_reasoning_from_text,
};
pub use vtcode_llm::providers::{
    AnthropicProvider, CopilotProvider, DeepSeekProvider, EvolinkProvider, GeminiProvider, HuggingFaceProvider,
    LlamaCppProvider, LmStudioProvider, MiMoProvider, MinimaxProvider, MistralProvider, MoonshotProvider,
    OllamaProvider, OpenAIProvider, OpenCodeGoProvider, OpenCodeZenProvider, OpenResponsesProvider, OpenRouterProvider,
    PoolsideProvider, QwenProvider, StepFunProvider, StreamingProgressBuilder, StreamingProgressCallback,
    StreamingProgressTracker, TagStreamSanitizer, XAIProvider, ZAIProvider, clean_reasoning_text, reasoning,
};

// Re-export commonly used constants from vtcode-commons::tool_types
pub use vtcode_commons::tool_types::{
    DEFAULT_VEC_CAPACITY, ERROR_DETECTION_PATTERNS, MAX_SEARCH_RESULTS, OVERFLOW_INDICATOR_PREFIX,
};

// Submodule re-exports for code that accesses provider internals (e.g. gemini wire,
// anthropic request_builder, shared stream utilities).
pub use vtcode_llm::providers::anthropic;
pub use vtcode_llm::providers::anthropic_types;
pub use vtcode_llm::providers::base;
pub use vtcode_llm::providers::common;
pub use vtcode_llm::providers::copilot;
pub use vtcode_llm::providers::error_handling;
pub use vtcode_llm::providers::evolink;
pub use vtcode_llm::providers::gemini;
pub use vtcode_llm::providers::huggingface;
pub use vtcode_llm::providers::llamacpp;
pub use vtcode_llm::providers::lmstudio;
pub use vtcode_llm::providers::local_server;
pub use vtcode_llm::providers::mimo;
pub use vtcode_llm::providers::minimax;
pub use vtcode_llm::providers::mistral;
pub use vtcode_llm::providers::moonshot;
pub use vtcode_llm::providers::ollama;
pub use vtcode_llm::providers::opencode_go;
pub use vtcode_llm::providers::opencode_zen;
pub use vtcode_llm::providers::openresponses;
pub use vtcode_llm::providers::openrouter;
pub use vtcode_llm::providers::poolside;
pub use vtcode_llm::providers::qwen;
pub use vtcode_llm::providers::shared;
pub use vtcode_llm::providers::stepfun;
pub use vtcode_llm::providers::streaming_progress;
pub use vtcode_llm::providers::zai;
