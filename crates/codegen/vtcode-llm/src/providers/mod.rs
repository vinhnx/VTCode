pub mod anthropic;
pub mod anthropic_types;
pub mod base;
pub mod deepseek;
pub mod error_handling;
pub mod evolink;
pub mod gemini;
pub mod huggingface;
pub mod llamacpp;
pub mod lmstudio;
pub mod local_readiness;
pub mod local_server;
pub mod mimo;
pub mod minimax;
pub mod mistral;
pub mod moonshot;
pub mod ollama;
pub mod openai;
mod openai_compat;
pub mod opencode_go;
mod opencode_shared;
pub mod opencode_zen;
pub mod openresponses;
pub mod openrouter;
pub mod poolside;
pub mod qwen;
pub mod stepfun;
pub mod streaming_progress;
pub mod tool_format;
pub mod xai;
pub mod zai;

pub mod common;
#[cfg(feature = "copilot")]
pub mod copilot;
pub mod reasoning;
pub mod shared;
pub use shared::TagStreamSanitizer;

// Re-export commonly used constants from vtcode-commons::tool_types
pub use vtcode_commons::tool_types::{
    DEFAULT_VEC_CAPACITY, ERROR_DETECTION_PATTERNS, MAX_SEARCH_RESULTS, OVERFLOW_INDICATOR_PREFIX,
};

pub use reasoning::clean_reasoning_text;
pub use reasoning::{ReasoningBuffer, ReasoningSegment, extract_reasoning_trace, split_reasoning_from_text};

pub use anthropic::AnthropicProvider;
#[cfg(feature = "copilot")]
pub use copilot::CopilotProvider;
pub use deepseek::DeepSeekProvider;
pub use evolink::EvolinkProvider;
pub use gemini::GeminiProvider;
pub use huggingface::HuggingFaceProvider;
pub use llamacpp::LlamaCppProvider;
pub use lmstudio::LmStudioProvider;
pub use mimo::MiMoProvider;
pub use minimax::MinimaxProvider;
pub use mistral::MistralProvider;
pub use moonshot::MoonshotProvider;
pub use ollama::OllamaProvider;
pub use openai::OpenAIProvider;
pub use opencode_go::OpenCodeGoProvider;
pub use opencode_zen::OpenCodeZenProvider;
pub use openresponses::OpenResponsesProvider;
pub use openrouter::OpenRouterProvider;
pub use poolside::PoolsideProvider;
pub use qwen::QwenProvider;
pub use stepfun::StepFunProvider;
pub use streaming_progress::{StreamingProgressBuilder, StreamingProgressCallback, StreamingProgressTracker};
pub use xai::XAIProvider;
pub use zai::ZAIProvider;
