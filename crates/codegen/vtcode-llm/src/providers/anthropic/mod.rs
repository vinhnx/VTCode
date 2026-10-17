//! Anthropic Claude API provider implementation
//!
//! This module provides a modular implementation of the Anthropic Claude LLM provider,
//! decomposed into focused submodules for maintainability:
//!
//! - `provider`: Main `AnthropicProvider` struct and `LLMProvider` trait impl
//! - `request_builder`: LLMRequest → Anthropic API format conversion
//! - `response_parser`: Anthropic API response → LLMResponse parsing
//! - `stream_decoder`: Server-sent events (SSE) streaming decoder
//! - `prompt_cache`: Prompt caching configuration and headers
//! - `headers`: API headers, beta features, and version management
//! - `capabilities`: Model capability detection (vision, reasoning, structured output)
//! - `validation`: Request validation and schema checking
//! - `fallback_retry`: Bounded credit-token retry when a server-side fallback could not run

#[cfg(feature = "anthropic-api")]
pub mod api;
mod block_order;
pub mod capabilities;
pub mod compat;
mod fallback_retry;
mod headers;
mod prompt_cache;
mod provider;
pub(crate) mod request_builder;
mod response_parser;
mod speed;
mod stream_decoder;
pub(crate) mod validation;

pub use provider::AnthropicProvider;

#[cfg(test)]
mod tests;
