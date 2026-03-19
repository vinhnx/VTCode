//! Model configuration and identification module
//!
//! This module provides a centralized enum for model identifiers and their configurations,
//! replacing hardcoded model strings throughout the codebase for better maintainability.
//! Read the model list in `docs/models.json`.

#[cfg(test)]
mod tests;

pub use vtcode_config::models::{
    ModelCatalogEntry, ModelId, ModelParseError, OpenRouterMetadata, Provider,
    catalog_provider_keys, model_catalog_entry, supported_models_for_provider,
};
