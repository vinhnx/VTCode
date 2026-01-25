//! Model configuration and identification module
//!
//! This module provides a centralized enum for model identifiers and their configurations,
//! replacing hardcoded model strings throughout the codebase for better maintainability.
//! Read the model list in `docs/models.json`.

mod capabilities;
mod catalog;
mod errors;
mod model_id;
mod model_id_parse;
mod openrouter;
mod provider;
mod selection;
#[cfg(test)]
mod tests;

pub use errors::ModelParseError;
pub use model_id::ModelId;
pub use provider::{OpenRouterMetadata, Provider};
