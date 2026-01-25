#[cfg(not(docsrs))]
include!(concat!(env!("OUT_DIR"), "/openrouter_constants.rs"));

#[cfg(docsrs)]
pub const SUPPORTED_MODELS: &[&str] = &[];
#[cfg(docsrs)]
pub const REASONING_MODELS: &[&str] = &[];
#[cfg(docsrs)]
pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[];

#[cfg(docsrs)]
// Define the constants that are referenced elsewhere to avoid compile errors
pub const X_AI_GROK_CODE_FAST_1: &str = "x-ai/grok-code-fast-1";
#[cfg(docsrs)]
pub const QWEN3_CODER: &str = "qwen/qwen3-coder";
#[cfg(docsrs)]
pub const ANTHROPIC_CLAUDE_SONNET_4_5: &str = "anthropic/claude-sonnet-4.5";

#[cfg(docsrs)]
pub mod vendor {
    pub mod openrouter {
        pub const MODELS: &[&str] = &[];
    }
}
