#[cfg(not(docsrs))]
include!(concat!(env!("OUT_DIR"), "/openrouter_constants.rs"));

// For docs.rs builds, define placeholder constants to avoid compilation errors
// The generated constants file includes these when not building for docs.rs
#[cfg(docsrs)]
mod generated_constants {
    pub const DEFAULT_MODEL: &str = "openrouter/auto";
    pub const SUPPORTED_MODELS: &[&str] = &[];
    pub const REASONING_MODELS: &[&str] = &[];
    pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[];

    // Define the constants that are referenced elsewhere to avoid compile errors
    pub const QWEN3_CODER: &str = "qwen/qwen3-coder";
    pub const QWEN3_CODER_NEXT: &str = "qwen/qwen3-coder-next";
    pub const ANTHROPIC_CLAUDE_SONNET_4_5: &str = "anthropic/claude-sonnet-4.5";
    pub const MINIMAX_M2_5: &str = "minimax/minimax-m2.5";
    pub const GOOGLE_GEMINI_3_1_PRO_PREVIEW: &str = "google/gemini-3.1-pro-preview";

    pub mod vendor {
        pub mod openrouter {
            pub const MODELS: &[&str] = &[];
        }
    }
}

// Re-export all constants to make them available at the module level
#[cfg(docsrs)]
pub use generated_constants::*;
