/// Default model - using stable version for production reliability
pub const DEFAULT_MODEL: &str = "gemini-3.7-flash";

pub const SUPPORTED_MODELS: &[&str] = &[
    "gemini-4-argon", // Gemini 4 Argon - flagship for hardest problems (unified 1M window, 64k sync output, low/medium/high)
    "gemini-3.6-flash", // Gemini 3.6 Flash - flash model with improved capabilities
    "gemini-3.7-flash", // Gemini 3.7 Flash - flash model with 1M context and tunable thinking
    "gemini-3.8-flash", // Gemini 3.8 Flash - most intelligent Flash for long-horizon SWE/agents (1M, 64k output, low/medium/high)
    "gemini-3-flash-preview", // Fast version of Gemini 3 Pro with 3-level thinking
    "gemini-3-pro-image-preview", // Image generation model with 4K resolution
];

/// Models that support thinking/reasoning capability with configurable thinking_level
/// Based on: <https://ai.google.dev/gemini-api/docs/gemini-3>
/// Gemini 3 Pro: supports low, high
/// Gemini 3 Flash Preview: supports minimal, low, medium, high
/// Gemini 3.7/3.8 Flash: supports low, medium (default), high — `minimal` not supported on 3.8 and will error
pub const REASONING_MODELS: &[&str] = &[
    "gemini-4-argon",
    "gemini-3.6-flash",
    "gemini-3.7-flash",
    "gemini-3.8-flash",
    "gemini-3-flash-preview",
];

/// Models that support Gemini 3 extended thinking levels (minimal, medium)
/// Only `gemini-3-flash-preview` supports `minimal`; stable 3.7/3.8 Flash support low/medium/high with medium as default
pub const EXTENDED_THINKING_MODELS: &[&str] = &["gemini-3-flash-preview"];

/// Models supporting image generation
pub const IMAGE_GENERATION_MODELS: &[&str] = &["gemini-3-pro-image-preview"];

/// Models that support context caching
/// Context caching reduces costs for repeated API calls with similar contexts
/// Reference: <https://ai.google.dev/gemini-api/docs/caching>
/// Gemini 4 Argon supports implicit caching only — explicit `cachedContents`
/// is not supported and requests referencing it fail; prefer Implicit mode.
pub const CACHING_MODELS: &[&str] = &[
    "gemini-4-argon",
    "gemini-3.6-flash",
    "gemini-3.7-flash",
    "gemini-3.8-flash",
    "gemini-3-flash-preview",
];

/// Models that reject explicit `cachedContents` entirely and support implicit
/// caching only. The provider must skip its explicit-cache lifecycle for these
/// models even when `[prompt_cache] mode = "explicit"` is configured —
/// creation requests fail and references in generateContent bodies error.
/// Reference: <https://ai.google.dev/gemini-api/docs/models/gemini-4-argon>
pub const IMPLICIT_CACHING_ONLY_MODELS: &[&str] = &["gemini-4-argon"];

/// Models that support code execution (Python)
/// Code execution allows models to write and execute Python code
/// Reference: <https://ai.google.dev/gemini-api/docs/code-execution>
pub const CODE_EXECUTION_MODELS: &[&str] = &[
    "gemini-4-argon",
    "gemini-3.6-flash",
    "gemini-3.7-flash",
    "gemini-3.8-flash",
    "gemini-3-flash-preview",
];

// Convenience constants for commonly used models
pub const GEMINI_4_ARGON: &str = "gemini-4-argon";
pub const GEMINI_3_6_FLASH: &str = "gemini-3.6-flash";
pub const GEMINI_3_7_FLASH: &str = "gemini-3.7-flash";
pub const GEMINI_3_8_FLASH: &str = "gemini-3.8-flash";
pub const GEMINI_3_FLASH_PREVIEW: &str = "gemini-3-flash-preview";
pub const GEMINI_3_PRO_IMAGE_PREVIEW: &str = "gemini-3-pro-image-preview";
