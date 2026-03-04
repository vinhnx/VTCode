/// Default model - using stable version for production reliability
pub const DEFAULT_MODEL: &str = "gemini-3-flash-preview";

pub const SUPPORTED_MODELS: &[&str] = &[
    "gemini-3.1-pro-preview",             // Latest Gemini 3.1 Pro flagship
    "gemini-3.1-pro-preview-customtools", // Optimized for custom tools & bash
    "gemini-3.1-flash-lite-preview",      // Most cost-efficient, fastest for lightweight tasks
    "gemini-3-flash-preview",             // Fast version of Gemini 3 Pro with 3-level thinking
    "gemini-3-pro-image-preview",         // Image generation model with 4K resolution
];

/// Models that support thinking/reasoning capability with configurable thinking_level
/// Based on: https://ai.google.dev/gemini-api/docs/gemini-3
/// Gemini 3 Pro/Flash: supports low, high (default)
/// Gemini 3 Flash only: also supports minimal, medium
pub const REASONING_MODELS: &[&str] = &[
    "gemini-3.1-pro-preview",
    "gemini-3.1-pro-preview-customtools",
    "gemini-3.1-flash-lite-preview",
    "gemini-3-flash-preview",
];

/// Models that support Gemini 3 extended thinking levels (minimal, medium)
/// Only Gemini 3 Flash supports these additional levels beyond low/high
pub const EXTENDED_THINKING_MODELS: &[&str] =
    &["gemini-3-flash-preview", "gemini-3.1-flash-lite-preview"];

/// Models supporting image generation
pub const IMAGE_GENERATION_MODELS: &[&str] = &["gemini-3-pro-image-preview"];

/// Models that support context caching (min 2048 tokens required)
/// Context caching reduces costs for repeated API calls with similar contexts
/// Reference: https://ai.google.dev/gemini-api/docs/caching
pub const CACHING_MODELS: &[&str] = &[
    "gemini-3.1-pro-preview",
    "gemini-3.1-pro-preview-customtools",
    "gemini-3.1-flash-lite-preview",
    "gemini-3-flash-preview",
];

/// Models that support code execution (Python)
/// Code execution allows models to write and execute Python code
/// Reference: https://ai.google.dev/gemini-api/docs/code-execution
pub const CODE_EXECUTION_MODELS: &[&str] = &[
    "gemini-3.1-pro-preview",
    "gemini-3.1-pro-preview-customtools",
    "gemini-3.1-flash-lite-preview",
    "gemini-3-flash-preview",
];

// Convenience constants for commonly used models
pub const GEMINI_3_1_PRO_PREVIEW: &str = "gemini-3.1-pro-preview";
pub const GEMINI_3_1_PRO_PREVIEW_CUSTOMTOOLS: &str = "gemini-3.1-pro-preview-customtools";
pub const GEMINI_3_1_FLASH_LITE_PREVIEW: &str = "gemini-3.1-flash-lite-preview";
pub const GEMINI_3_FLASH_PREVIEW: &str = "gemini-3-flash-preview";
pub const GEMINI_3_PRO_IMAGE_PREVIEW: &str = "gemini-3-pro-image-preview";
