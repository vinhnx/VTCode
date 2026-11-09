//! Unified LLM client trait.
//!
//! The canonical `LLMClient` trait, `AnyClient` alias, and
//! `ProviderClientAdapter` live in [`vtcode_llm::client`]. This module
//! re-exports them so the historical `crate::llm::client::*` import paths
//! continue to resolve throughout `vtcode-core`, and adds the core-side
//! `make_client` factory, which resolves core `ModelId` values through
//! core's provider factory.

pub use vtcode_llm::client::{AnyClient, LLMClient, ProviderClientAdapter};

use super::provider::LLMError;
use crate::config::models::ModelId;

/// Create a client based on the model ID
/// Uses the existing factory pattern from factory.rs
pub fn make_client(api_key: String, model: ModelId) -> Result<AnyClient, LLMError> {
    let model_id = model.to_string();
    // Use the factory to create the provider, then adapt it to the client trait.
    let provider = super::factory::create_provider_for_model(&model_id, api_key, None, None)?;
    Ok(Box::new(ProviderClientAdapter::new(provider, model_id)))
}
