//! HTTP client utilities
//!
//! Consolidated re-exports from vtcode-commons and LLM-specific factories.

pub use vtcode_commons::http::*;
pub use crate::llm::http_client::HttpClientFactory;
