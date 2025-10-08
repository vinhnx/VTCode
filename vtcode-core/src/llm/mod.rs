//! # LLM Integration Layer
//!
//! This module provides a unified, modular interface for integrating multiple LLM providers
//! with VTCode, supporting Gemini, OpenAI, Anthropic, xAI, and DeepSeek.
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
//! | Gemini | ✓ | gemini-2.5-pro, gemini-2.5-flash-preview-05-20 |
//! | OpenAI | ✓ | gpt-5, gpt-4.1, gpt-5-mini |
//! | Anthropic | ✓ | claude-4.1-opus, claude-4-sonnet |
//! | xAI | ✓ | grok-2-latest, grok-2-mini |
//! | DeepSeek | ✓ | deepseek-chat, deepseek-reasoner |
//! | Z.AI | ✓ | glm-4.6 |
//!
//! ## Basic Usage
//!
//! ```rust,no_run
//! use vtcode_core::llm::{AnyClient, make_client};
//! use vtcode_core::utils::dot_config::ProviderConfigs;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Configure providers
//!     let providers = ProviderConfigs {
//!         gemini: Some(vtcode_core::utils::dot_config::ProviderConfig {
//!             api_key: std::env::var("GEMINI_API_KEY")?,
//!             model: "gemini-2.5-flash".to_string(),
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
//! ```rust,no_run
//! use vtcode_core::utils::dot_config::{ProviderConfigs, ProviderConfig};
//!
//! let config = ProviderConfigs {
//!     gemini: Some(ProviderConfig {
//!         api_key: "your-api-key".to_string(),
//!         model: "gemini-2.5-flash".to_string(),
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
//! ```rust,no_run
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
//! ```rust,no_run
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
//! ```rust,no_run
//! use vtcode_core::llm::LLMError;
//!
//! match client.chat(&messages, None).await {
//!     Ok(response) => println!("Success: {}", response.content),
//!     Err(LLMError::Authentication) => eprintln!("Authentication failed"),
//!     Err(LLMError::RateLimit) => eprintln!("Rate limit exceeded"),
//!     Err(LLMError::Network(e)) => eprintln!("Network error: {}", e),
//!     Err(LLMError::Provider(e)) => eprintln!("Provider error: {}", e),
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

pub mod client;
pub mod error_display;
pub mod factory;
pub mod provider;
pub mod providers;
pub mod rig_adapter;
pub mod types;

#[cfg(test)]
mod error_display_test;

// Re-export main types for backward compatibility
pub use client::{AnyClient, make_client};
pub use factory::{create_provider_with_config, get_factory};
pub use provider::{LLMStream, LLMStreamEvent};
pub use providers::{AnthropicProvider, GeminiProvider, OpenAIProvider, XAIProvider, ZAIProvider};
pub use types::{BackendKind, LLMError, LLMResponse};
