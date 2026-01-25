use crate::models::{OpenRouterMetadata, openrouter_generated};

use super::ModelId;

impl ModelId {
    pub(super) fn openrouter_metadata(&self) -> Option<OpenRouterMetadata> {
        #[cfg(not(docsrs))]
        {
            openrouter_generated::metadata_for(*self)
        }
        #[cfg(docsrs)]
        {
            None
        }
    }

    pub(super) fn parse_openrouter_model(value: &str) -> Option<Self> {
        #[cfg(not(docsrs))]
        {
            openrouter_generated::parse_model(value)
        }
        #[cfg(docsrs)]
        {
            None
        }
    }

    pub(super) fn openrouter_vendor_groups() -> Vec<(&'static str, &'static [Self])> {
        #[cfg(not(docsrs))]
        {
            openrouter_generated::vendor_groups()
                .iter()
                .map(|group| (group.vendor, group.models))
                .collect()
        }
        #[cfg(docsrs)]
        {
            Vec::new()
        }
    }

    pub(super) fn openrouter_models() -> Vec<Self> {
        Self::openrouter_vendor_groups()
            .into_iter()
            .flat_map(|(_, models)| models.iter().copied())
            .collect()
    }
}
