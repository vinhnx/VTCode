#[cfg(not(docsrs))]
include!(concat!(env!("OUT_DIR"), "/openrouter_constants.rs"));

// For docs.rs builds, define placeholder constants to avoid compilation errors
#[cfg(docsrs)]
mod generated_constants {
    pub const DEFAULT_MODEL: &str = "openrouter/auto";
    pub const SUPPORTED_MODELS: &[&str] = &[];
    pub const REASONING_MODELS: &[&str] = &[];
    pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[];

    pub mod vendor {
        pub mod openrouter {
            pub const MODELS: &[&str] = &[];
        }
    }
}

#[cfg(docsrs)]
pub use generated_constants::*;
