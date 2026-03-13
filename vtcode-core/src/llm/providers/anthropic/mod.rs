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

#[cfg(feature = "anthropic-api")]
pub mod api;
pub(crate) mod capabilities;
pub mod compat;
mod headers;
mod message_parser;
mod prompt_cache;
mod provider;
mod request_builder;
mod response_parser;
mod stream_decoder;
mod validation;

pub use provider::AnthropicProvider;

#[cfg(test)]
mod tests;
