//! Model configuration and identification module
//!
//! This module provides a centralized enum for model identifiers and their configurations,
//! replacing hardcoded model strings throughout the codebase for better maintainability.
//! Read the model list in `docs/models.json`.

mod errors;
mod model_id;
mod openrouter;
mod provider;

pub use errors::ModelParseError;
pub use model_id::{
    ModelCatalogEntry, ModelId, catalog_provider_keys, model_catalog_entry,
    supported_models_for_provider,
};
pub use openrouter::OpenRouterMetadata;
pub use openrouter::openrouter_generated;
pub use provider::Provider;

#[cfg(test)]
mod tests;
