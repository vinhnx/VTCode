//! # vtcode-llm - LLM Provider Abstraction
//!
//! Provides a unified interface for multiple LLM providers including
//! Gemini, OpenAI, Anthropic, DeepSeek, and Ollama.

pub mod capabilities;
pub mod client;
pub mod config_adapter;
pub mod error_display;
pub mod factory_types;
pub mod http_client;
pub mod model_resolver;
pub mod optimized_client;
pub mod provider;
pub mod provider_base;
pub mod provider_config_types;
pub mod providers;
pub mod rig_adapter;
mod single_response;
pub mod system_prompt;
pub mod tool_bridge;
pub mod types;
pub mod utils;

// Re-export main types for backward compatibility
pub use capabilities::ProviderCapabilities;
pub use client::{AnyClient, ProviderClientAdapter};
pub use config_adapter::{
    AdapterEvent, AdapterHooks, AdapterHooksProvider, OwnedProviderConfig, as_factory_config,
    as_factory_config_with_hooks,
};
pub use factory_types::{ProviderConfig, infer_provider_from_model};
pub use model_resolver::{
    DynamicModelMeta, DynamicModelRef, ModelAvailability, ModelResolver, ResolvedModel,
};
pub use optimized_client::{OptimizedLLMClient, OptimizedRequest, OptimizedResponse};
pub use provider::{FinishReason, LLMStream, LLMStreamEvent, Usage};
pub use provider_config_types::ProviderConfig as ProviderConfigData;
pub use single_response::collect_single_response;
pub use tool_bridge::{
    CorrelationStats, IntentFulfillment, MessageCorrelationTracker, MessageToolCorrelation,
    ToolExecution, ToolIntent, ToolIntentExtractor,
};
pub use types::{BackendKind, LLMError, LLMResponse};
pub use vtcode_commons::delegate_components;
