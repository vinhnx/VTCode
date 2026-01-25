//! OpenRouter metadata helpers for model identifiers.

use super::{ModelId, OpenRouterMetadata};

mod openrouter_generated {
    include!(concat!(env!("OUT_DIR"), "/openrouter_metadata.rs"));
}

impl ModelId {
    pub(super) fn openrouter_metadata(&self) -> Option<OpenRouterMetadata> {
        openrouter_generated::metadata_for(*self)
    }

    pub(super) fn parse_openrouter_model(value: &str) -> Option<Self> {
        openrouter_generated::parse_model(value)
    }

    pub(super) fn openrouter_vendor_groups() -> Vec<(&'static str, &'static [Self])> {
        openrouter_generated::vendor_groups()
            .iter()
            .map(|group| (group.vendor, group.models))
            .collect()
    }

    pub(super) fn openrouter_models() -> Vec<Self> {
        Self::openrouter_vendor_groups()
            .into_iter()
            .flat_map(|(_, models)| models.iter().copied())
            .collect()
    }
}
