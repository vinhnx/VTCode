//! Core provider configuration type.
//!
//! `ProviderConfig` is defined here so that both vtcode-llm and vtcode-core
//! can use it without circular dependencies. The `LLMFactory` and factory
//! functions live in vtcode-core since they depend on the CGP registration system.

pub use crate::provider_config_types::ProviderConfig;

use crate::model_resolver::ModelResolver;
use vtcode_config::models::Provider;

/// Infer provider from model slug using model resolver.
pub fn infer_provider_from_model(model: &str) -> Option<Provider> {
    ModelResolver::resolve_provider(None, model, &[]).or_else(|| {
        let family = vtcode_tool_types::model_family::find_family_for_model(model);
        (family.family != "unknown").then_some(family.provider)
    })
}
