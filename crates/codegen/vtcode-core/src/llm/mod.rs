//! # LLM Integration Layer
//!
//! This module provides a unified, modular interface for integrating multiple LLM providers
//! with VT Code, supporting Gemini, OpenAI, Anthropic, xAI, and DeepSeek.
//!
//! ## Architecture Overview
//!
//! The LLM layer is designed with several key principles:
//!
//! - **Unified Interface**: Single `AnyClient` trait for all providers
//! - **Provider Agnostic**: Easy switching between providers
//! - **Configuration Driven**: TOML-based provider configuration
//! - **Error Handling**: Comprehensive error types and recovery
//! - **Async Support**: Full async/await support for all operations
//!
//! ## Supported Providers
//!
//! | Provider | Status | Models |
//! |----------|--------|---------|
//! | Gemini | ✓ | gemini-3.1-pro-preview, gemini-3-flash-preview |
//! | OpenAI | ✓ | gpt-5, o3, o4-mini, gpt-5-mini, gpt-5-nano |
//! | Anthropic | ✓ | claude-4.1-opus, claude-4-sonnet |
//! | xAI | ✓ | grok-2-latest, grok-2-mini |
//! | DeepSeek | ✓ | deepseek-chat, deepseek-reasoner |
//! | Z.AI | ✓ | glm-5 |
//! | Ollama | ✓ | gpt-oss:20b (local) |
//!
//! ## Basic Usage
//!
//! ```rust,ignore
//! use vtcode_core::llm::{AnyClient, make_client};
//! use vtcode_core::utils::dot_config::ProviderConfigs;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Configure providers
//!     let providers = ProviderConfigs {
//!         gemini: Some(vtcode_core::utils::dot_config::ProviderConfig {
//!             api_key: std::env::var("GEMINI_API_KEY")?,
//!             model: "gemini-3-flash-preview".to_string(),
//!             ..Default::default()
//!         }),
//!         ..Default::default()
//!     };
//!
//!     // Create client
//!     let client = make_client(&providers, "gemini")?;
//!
//!     // Make a request
//!     let messages = vec![
//!         vtcode_core::llm::types::Message {
//!             role: "user".to_string(),
//!             content: "Hello, how can you help me with coding?".to_string(),
//!         }
//!     ];
//!
//!     let response = client.chat(&messages, None).await?;
//!     println!("Response: {}", response.content);
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Provider Configuration
//!
//! ```rust,ignore
//! use vtcode_core::utils::dot_config::{ProviderConfigs, ProviderConfig};
//!
//! let config = ProviderConfigs {
//!     gemini: Some(ProviderConfig {
//!         api_key: "your-api-key".to_string(),
//!         model: "gemini-3-flash-preview".to_string(),
//!         temperature: Some(0.7),
//!         max_tokens: Some(4096),
//!         ..Default::default()
//!     }),
//!     openai: Some(ProviderConfig {
//!         api_key: "your-openai-key".to_string(),
//!         model: "gpt-5".to_string(),
//!         temperature: Some(0.3),
//!         max_tokens: Some(8192),
//!         ..Default::default()
//!     }),
//!     ..Default::default()
//! };
//! ```
//!
//! ## Advanced Features
//!
//! ### Streaming Responses
//! ```rust,ignore
//! use vtcode_core::llm::AnyClient;
//! use futures::StreamExt;
//!
//! let client = make_client(&providers, "gemini")?;
//!
//! let mut stream = client.chat_stream(&messages, None).await?;
//! while let Some(chunk) = stream.next().await {
//!     match chunk {
//!         Ok(response) => print!("{}", response.content),
//!         Err(e) => eprintln!("Error: {}", e),
//!     }
//! }
//! ```
//!
//! ### Function Calling
//! ```rust,ignore
//! use vtcode_core::llm::types::{FunctionDeclaration, FunctionCall};
//!
//! let functions = vec![
//!     FunctionDeclaration {
//!         name: "read_file".to_string(),
//!         description: "Read a file from the filesystem".to_string(),
//!         parameters: serde_json::json!({
//!             "type": "object",
//!             "properties": {
//!                 "path": {"type": "string", "description": "File path to read"}
//!             },
//!             "required": ["path"]
//!         }),
//!     }
//! ];
//!
//! let response = client.chat_with_functions(&messages, &functions, None).await?;
//!
//! if let Some(function_call) = response.function_call {
//!     match function_call.name.as_str() {
//!         "read_file" => {
//!             // Handle function call
//!         }
//!         _ => {}
//!     }
//! }
//! ```
//!
//! ## Error Handling
//!
//! The LLM layer provides comprehensive error handling:
//!
//! ```rust,ignore
//! use vtcode_core::llm::LLMError;
//!
//! match client.chat(&messages, None).await {
//!     Ok(response) => println!("Success: {}", response.content),
//!     Err(LLMError::Authentication) => eprintln!("Authentication failed"),
//!     Err(LLMError::RateLimit { metadata: None }) => eprintln!("Rate limit exceeded"),
//!     Err(LLMError::Network { message: e, metadata: None }) => eprintln!("Network error: {}", e),
//!     Err(LLMError::Provider { message: e, metadata: None }) => eprintln!("Provider error: {}", e),
//!     Err(e) => eprintln!("Other error: {}", e),
//! }
//! ```
//!
//! ## Performance Considerations
//!
//! - **Connection Pooling**: Efficient connection reuse
//! - **Request Batching**: Where supported by providers
//! - **Caching**: Built-in prompt caching for repeated requests
//! - **Timeout Handling**: Configurable timeouts and retries
//! - **Rate Limiting**: Automatic rate limit handling
//!
//! # LLM abstraction layer with modular architecture
//!
//! This module provides a unified interface for different LLM providers
//! with provider-specific implementations.

/// Provider capability declarations and feature detection.
pub mod capabilities;
/// Context-Generic Provider (CGP) wiring for the LLM factory.
pub mod cgp;
/// Simplified LLM client trait and adapter.
pub mod client;
/// Adapter between config-level and factory-level provider configurations.
pub mod config_adapter;
/// Human-readable error formatting for LLM errors.
pub mod error_display;
/// LLM provider factory and global registry.
pub mod factory;
/// Shared HTTP client utilities for provider implementations.
pub mod http_client;
/// Lightweight (cheap/fast) model routing for auxiliary features.
pub mod lightweight_routing;
#[cfg(feature = "mock")]
/// Mock LLM client for testing.
pub mod mock_client;
/// Model resolution, availability checks, and dynamic metadata.
pub mod model_resolver;
/// Optimized client with connection pooling and request deduplication.
pub mod optimized_client;
/// Core LLM provider trait and error types.
pub mod provider;
/// Shared provider utilities to eliminate duplicate code.
pub mod provider_base;
/// Generic provider builder with builder-pattern construction.
pub mod provider_builder;
/// Per-provider configuration types and the unified creation shim.
pub mod provider_config;
/// Re-exported provider implementations.
pub mod providers;
/// Shared idle-gap tracker for detecting when the provider prompt cache has
/// likely expired between dispatched LLM requests.
pub mod request_gap;
/// Adapter for the Rig agent framework.
pub mod rig_adapter;
/// RL optimization loop: adaptive action selection (bandit / actor-critic).
pub mod rl;
mod single_response;
/// Tool-call correlation and intent extraction for LLM responses.
pub mod tool_bridge;
/// LLM request/response types, errors, and backend kind.
pub mod types;
/// Provider-normalized usage accumulation and cache-aware session cost estimation.
pub mod usage_cost;
/// Shared utilities for request/response processing.
pub mod utils;

// Re-export main types for backward compatibility
pub use capabilities::ProviderCapabilities;
pub use client::{AnyClient, ProviderClientAdapter, make_client};
pub use factory::{create_provider_with_config, get_factory, get_models_manager, infer_provider_from_model};
pub use lightweight_routing::{
    LightweightFeature, LightweightRouteResolution, LightweightRouteSource, ModelRoute, auto_lightweight_model,
    create_provider_for_model_route, lightweight_model_choices, main_model_route, resolve_api_key_for_model_route,
    resolve_lightweight_route,
};
pub use model_resolver::{DynamicModelMeta, DynamicModelRef, ModelAvailability, ModelResolver, ResolvedModel};
pub use optimized_client::{OptimizedLLMClient, OptimizedRequest, OptimizedResponse};
pub use provider::{FinishReason, LLMStream, LLMStreamEvent, Usage};
pub use providers::{
    AnthropicProvider, GeminiProvider, HuggingFaceProvider, OllamaProvider, OpenAIProvider, ZAIProvider,
};
pub use single_response::collect_single_response;
pub use tool_bridge::{
    CorrelationStats, IntentFulfillment, MessageCorrelationTracker, MessageToolCorrelation, ToolExecution, ToolIntent,
    ToolIntentExtractor,
};

pub use types::{BackendKind, LLMError, LLMResponse};

pub use config_adapter::{
    AdapterEvent, AdapterHooks, AdapterHooksProvider, OwnedProviderConfig, ProviderConfig, as_factory_config,
    as_factory_config_with_hooks,
};
#[cfg(feature = "mock")]
pub use mock_client::StaticResponseClient;
