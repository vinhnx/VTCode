/// Default model - using stable version for production reliability
pub const DEFAULT_MODEL: &str = "gemini-2.5-flash";

pub const SUPPORTED_MODELS: &[&str] = &[
    "gemini-3-pro-preview",       // Latest flagship model with advanced reasoning
    "gemini-3-flash-preview",     // Fast version of Gemini 3 Pro with 3-level thinking
    "gemini-3-pro-image-preview", // Image generation model with 4K resolution
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
    "gemini-2.5-flash-preview-05-20",
    "gemini-1.5-pro",
    "gemini-1.5-flash",
];

/// Models that support thinking/reasoning capability with configurable thinking_level
/// Based on: https://ai.google.dev/gemini-api/docs/gemini-3
/// Gemini 3 Pro/Flash: supports low, high (default)
/// Gemini 3 Flash only: also supports minimal, medium
pub const REASONING_MODELS: &[&str] = &[
    "gemini-3-pro-preview",
    "gemini-3-flash-preview",
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
    "gemini-2.5-flash-preview-05-20",
    "gemini-1.5-pro",
    "gemini-1.5-flash",
];

/// Models that support Gemini 3 extended thinking levels (minimal, medium)
/// Only Gemini 3 Flash supports these additional levels beyond low/high
pub const EXTENDED_THINKING_MODELS: &[&str] = &["gemini-3-flash-preview"];

/// Models supporting image generation
pub const IMAGE_GENERATION_MODELS: &[&str] = &["gemini-3-pro-image-preview"];

/// Models that support context caching (min 2048 tokens required)
/// Context caching reduces costs for repeated API calls with similar contexts
/// Reference: https://ai.google.dev/gemini-api/docs/caching
pub const CACHING_MODELS: &[&str] = &[
    "gemini-3-pro-preview",
    "gemini-3-flash-preview",
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
    "gemini-2.5-flash-preview-05-20",
    "gemini-1.5-pro",
    "gemini-1.5-flash",
];

/// Models that support code execution (Python)
/// Code execution allows models to write and execute Python code
/// Reference: https://ai.google.dev/gemini-api/docs/code-execution
pub const CODE_EXECUTION_MODELS: &[&str] = &[
    "gemini-3-pro-preview",
    "gemini-3-flash-preview",
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
    "gemini-2.5-flash-preview-05-20",
    "gemini-1.5-pro",
    "gemini-1.5-flash",
];

// Convenience constants for commonly used models
pub const GEMINI_2_5_PRO: &str = "gemini-2.5-pro";
pub const GEMINI_2_5_FLASH: &str = "gemini-2.5-flash";
pub const GEMINI_2_5_FLASH_LITE: &str = "gemini-2.5-flash-lite";
pub const GEMINI_2_5_FLASH_PREVIEW: &str = "gemini-2.5-flash-preview-05-20";
pub const GEMINI_3_PRO_PREVIEW: &str = "gemini-3-pro-preview";
pub const GEMINI_3_FLASH_PREVIEW: &str = "gemini-3-flash-preview";
pub const GEMINI_3_PRO_IMAGE_PREVIEW: &str = "gemini-3-pro-image-preview";
