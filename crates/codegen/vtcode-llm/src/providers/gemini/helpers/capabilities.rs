//! Gemini model capability predicates and token limits.

use super::*;

impl GeminiProvider {
    pub(crate) fn is_gemini_3_pro_model(model: &str) -> bool {
        let rest = match model.strip_prefix("gemini-") {
            Some(r) => r,
            None => return false,
        };
        let version_part = rest.split('-').next().unwrap_or("");
        let Some(major_str) = version_part.split('.').next() else {
            return false;
        };
        major_str == "3" && model.contains("pro") && !model.contains("flash")
    }

    /// Check if model supports context caching
    pub fn supports_caching(model: &str) -> bool {
        models::google::CACHING_MODELS.contains(&model)
    }

    /// Check if model supports code execution
    pub fn supports_code_execution(model: &str) -> bool {
        models::google::CODE_EXECUTION_MODELS.contains(&model)
    }

    /// Get maximum input token limit for a model
    pub fn max_input_tokens(model: &str) -> usize {
        if model.contains("gemini-4-argon") || model.contains("gemini-3.1") {
            // Gemini 4 Argon: unified 1M limit shared by input, thinking, and output
            1_048_576
        } else if model.contains("3") || model.contains("4") || model.contains("1.5-pro") {
            2_097_152 // 2M tokens for Gemini 1.5 Pro, 3.x, and 4.x models
        } else {
            1_048_576 // 1M tokens for other current models
        }
    }

    /// Get maximum output token limit for a model
    pub fn max_output_tokens(model: &str) -> usize {
        if model.contains("3") || model.contains("4") {
            65_536 // 65K tokens for Gemini 3 and 4 models
        } else {
            8_192 // Conservative default
        }
    }

    /// Check if model supports extended thinking levels (minimal, medium)
    /// Only Gemini 3 Flash Preview supports the `minimal` level; stable
    /// Gemini 3.7/3.8 Flash support low/medium/high with medium as default.
    pub(crate) fn supports_extended_thinking(model: &str) -> bool {
        model.contains("gemini-3-flash-preview")
    }

    /// Determine whether a Gemini model uses the latest API behavior that
    /// deprecates sampling parameters (`temperature`, `top_p`, `top_k`) and
    /// disallows prefilled model turns (last turn with role `"model"`).
    ///
    /// Affected: all `gemini-3.5+` models and all future model releases.
    pub(crate) fn uses_latest_gemini_api(model: &str) -> bool {
        // Extract version from model name like "gemini-3.7-flash" -> "3.5"
        let rest = match model.strip_prefix("gemini-") {
            Some(r) => r,
            None => return false,
        };
        let version_part = rest.split('-').next().unwrap_or("");
        let mut parts = version_part.splitn(2, '.');
        let major: u32 = match parts.next().and_then(|s| s.parse().ok()) {
            Some(m) => m,
            None => return false,
        };
        if major > 3 {
            return true;
        }
        if major < 3 {
            return false;
        }
        // major == 3: check minor >= 5 (3.5, 3.6, and all future 3.x)
        match parts.next().and_then(|s| s.parse::<u32>().ok()) {
            Some(minor) => minor >= 5,
            None => false,
        }
    }

    /// Get supported thinking levels for a model
    /// Reference: <https://ai.google.dev/gemini-api/docs/gemini-3>
    /// Gemini 3.8 Flash: low, medium (default), high — `minimal` is not supported and will error.
    /// Gemini 4 Argon: low, medium (default), high.
    pub(crate) fn supported_thinking_levels(model: &str) -> Vec<&'static str> {
        if model.contains("gemini-3-flash-preview") {
            // Preview supports all levels including minimal
            vec!["minimal", "low", "medium", "high"]
        } else if model.contains("gemini-4-argon") {
            // Gemini 4 Argon supports low/medium/high (medium default)
            vec!["low", "medium", "high"]
        } else if model.contains("gemini-3") && model.contains("flash") {
            // Stable Flash models (3.6, 3.7, 3.8) support low/medium/high (medium default)
            vec!["low", "medium", "high"]
        } else if model.contains("gemini-3") {
            // Gemini 3 Pro supports low and high
            vec!["low", "high"]
        } else {
            // Unknown model, conservative default
            vec!["low", "high"]
        }
    }
}
