pub use super::ModelId;

#[derive(Clone, Copy)]
pub struct OpenRouterMetadata {
    pub(crate) id: &'static str,
    pub(crate) vendor: &'static str,
    pub(crate) display: &'static str,
    pub(crate) description: &'static str,
    pub(crate) efficient: bool,
    pub(crate) top_tier: bool,
    pub(crate) generation: &'static str,
    pub(crate) reasoning: bool,
    pub(crate) tool_call: bool,
}

#[cfg(not(docsrs))]
pub mod openrouter_generated {
    include!(concat!(env!("OUT_DIR"), "/openrouter_metadata.rs"));
}

#[cfg(docsrs)]
pub mod openrouter_generated {
    #[derive(Clone, Copy)]
    pub struct Entry {
        pub variant: super::super::ModelId,
        pub id: &'static str,
        pub vendor: &'static str,
        pub display: &'static str,
        pub description: &'static str,
        pub efficient: bool,
        pub top_tier: bool,
        pub generation: &'static str,
        pub reasoning: bool,
        pub tool_call: bool,
    }

    pub const ENTRIES: &[Entry] = &[];

    #[derive(Clone, Copy)]
    pub struct VendorModels {
        pub vendor: &'static str,
        pub models: &'static [super::super::ModelId],
    }

    pub const VENDOR_MODELS: &[VendorModels] = &[];

    pub fn metadata_for(_model: super::super::ModelId) -> Option<super::OpenRouterMetadata> {
        None
    }

    pub fn parse_model(_value: &str) -> Option<super::super::ModelId> {
        None
    }

    pub fn vendor_groups() -> &'static [VendorModels] {
        VENDOR_MODELS
    }
}
