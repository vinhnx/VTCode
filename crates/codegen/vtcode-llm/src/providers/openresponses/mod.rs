//! OpenResponses provider module.
//!
//! This module implements a provider for the OpenResponses specification,
//! an open-source multi-provider LLM API standard based on OpenAI's Responses API.
//!
//! See <https://www.openresponses.org> for the full specification.

pub mod provider;
pub mod streaming;
pub mod types;

pub use provider::OpenResponsesProvider;
pub use types::*;
