//! OpenAI Provider Implementation
//!
//! This module provides the OpenAI API integration, supporting:
//! - Chat Completions API (GPT-4o, GPT-4, etc.)
//! - Responses API (GPT-5, GPT-5.1 Codex, etc.)
//! - Harmony encoding for GPT-OSS models
//! - Streaming and non-streaming responses
//! - Tool/function calling
//! - Reasoning models with effort configuration
//!
//! ## Module Structure
//!
//! The OpenAI provider is split into focused submodules:
//! - `types` - Shared types and constants
//! - `errors` - Error handling and formatting
//! - `streaming` - Stream processing and telemetry
//! - `responses_api` - Responses API payload handling
//! - `provider` - Main `OpenAIProvider` implementation
//!
//! ## Example
//!
//! ```rust,ignore
//! use vtcode_core::llm::providers::OpenAIProvider;
//!
//! let provider = OpenAIProvider::new("sk-...".to_string());
//! ```

pub mod errors;
pub mod message_parser;
pub mod responses_api;
pub mod request_builder;
pub mod response_parser;
pub mod streaming;
pub mod stream_decoder;
pub mod tool_serialization;
pub mod types;

// Main provider implementation
mod provider;
pub use provider::OpenAIProvider;
