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

pub mod openrouter_generated {
    include!(concat!(env!("OUT_DIR"), "/openrouter_metadata.rs"));
}
