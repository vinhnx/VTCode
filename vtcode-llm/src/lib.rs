//! Prototype crate that re-exports VT Code's LLM integration layer while
//! introducing decoupled configuration traits for downstream consumers.
//!
//! The goal is to let external applications supply their own configuration
//! sources without depending on VT Code's dot-config structures. Consumers can
//! implement [`config::ProviderConfig`] for their own types and then convert
//! them into the factory configuration used internally by `vtcode-core`.
//!
//! This crate exposes feature flags so downstream projects can opt into
//! provider-specific exports, function calling helpers, or streaming telemetry
//! utilities without pulling additional API surface by default. Consult
//! `docs/modules/vtcode_llm_environment.md` for a full overview of environment
//! variables, configuration patterns, and the optional mock client helpers.

pub mod config;

pub use vtcode_commons::{
    ErrorFormatter, ErrorReporter, PathResolver, PathScope, TelemetrySink, WorkspacePaths,
};

pub use vtcode_core::llm::client::{AnyClient, make_client};
pub use vtcode_core::llm::error_display;
pub use vtcode_core::llm::factory::{
    ProviderConfig as CoreProviderConfig, create_provider_with_config, get_factory,
};
pub use vtcode_core::llm::rig_adapter;
pub use vtcode_core::llm::types::{BackendKind, LLMError, LLMResponse, Usage};

pub mod provider {
    //! Re-export the provider abstraction and shared request/response types.
    pub use vtcode_core::llm::provider::{
        LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent, Message, MessageRole,
        ParallelToolConfig,
    };

    #[cfg(feature = "functions")]
    pub use vtcode_core::llm::provider::{
        FunctionCall, FunctionDefinition, SpecificFunctionChoice, SpecificToolChoice, ToolCall,
        ToolChoice, ToolDefinition,
    };
}

pub use provider::{
    LLMProvider, LLMRequest, LLMResponse as ProviderLLMResponse, LLMStream, LLMStreamEvent,
    Message, MessageRole,
};

#[cfg(feature = "functions")]
pub use provider::{
    FunctionCall, FunctionDefinition, SpecificFunctionChoice, SpecificToolChoice, ToolCall,
    ToolChoice, ToolDefinition,
};

#[cfg(feature = "anthropic")]
pub use vtcode_core::llm::providers::AnthropicProvider;
#[cfg(feature = "deepseek")]
pub use vtcode_core::llm::providers::DeepSeekProvider;
#[cfg(feature = "google")]
pub use vtcode_core::llm::providers::GeminiProvider;
#[cfg(feature = "moonshot")]
pub use vtcode_core::llm::providers::MoonshotProvider;
#[cfg(feature = "ollama")]
pub use vtcode_core::llm::providers::OllamaProvider;
#[cfg(feature = "openai")]
pub use vtcode_core::llm::providers::OpenAIProvider;
#[cfg(feature = "openrouter")]
pub use vtcode_core::llm::providers::OpenRouterProvider;
#[cfg(feature = "xai")]
pub use vtcode_core::llm::providers::XAIProvider;
#[cfg(feature = "zai")]
pub use vtcode_core::llm::providers::ZAIProvider;

#[cfg(feature = "mock")]
pub mod mock;

#[cfg(feature = "mock")]
pub use mock::StaticResponseClient;

pub mod providers {
    //! Provider-specific exports gated behind feature flags so consumers can
    //! depend on a minimal surface when only a subset of providers is needed.
    #[cfg(feature = "anthropic")]
    pub use vtcode_core::llm::providers::anthropic::*;
    #[cfg(feature = "deepseek")]
    pub use vtcode_core::llm::providers::deepseek::*;
    #[cfg(feature = "google")]
    pub use vtcode_core::llm::providers::gemini::*;
    #[cfg(feature = "moonshot")]
    pub use vtcode_core::llm::providers::moonshot::*;
    #[cfg(feature = "ollama")]
    pub use vtcode_core::llm::providers::ollama::*;
    #[cfg(feature = "openai")]
    pub use vtcode_core::llm::providers::openai::*;
    #[cfg(feature = "openrouter")]
    pub use vtcode_core::llm::providers::openrouter::*;
    #[cfg(feature = "xai")]
    pub use vtcode_core::llm::providers::xai::*;
    #[cfg(feature = "zai")]
    pub use vtcode_core::llm::providers::zai::*;
}

#[cfg(feature = "telemetry")]
pub mod telemetry {
    //! Streaming telemetry helpers shared across provider implementations.
    pub use vtcode_core::llm::providers::shared::{
        NoopStreamTelemetry, StreamAssemblyError, StreamDelta, StreamFragment, StreamTelemetry,
        ToolCallBuilder, append_reasoning_segments, append_text_with_reasoning,
        finalize_tool_calls, update_tool_calls,
    };
}

#[cfg(feature = "telemetry")]
pub use telemetry::{NoopStreamTelemetry, StreamTelemetry};
