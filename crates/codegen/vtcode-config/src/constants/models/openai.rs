pub const DEFAULT_MODEL: &str = "gpt-5.6-sol";
pub const SUPPORTED_MODELS: &[&str] = &[
    GPT,
    GPT_6_ASTRA,
    GPT_6_SOL,
    GPT_6_1_SOL,
    GPT_6_LUNA,
    "gpt-5.6",       // GPT-5.6 alias (routes to gpt-5.6-sol)
    "gpt-5.6-sol",   // GPT-5.6 Sol flagship model
    "gpt-5.6-terra", // GPT-5.6 Terra balanced model
    "gpt-5.6-luna",  // GPT-5.6 Luna efficient model
    "gpt-5.1",       // GPT-5.3 Codex optimized for agentic coding with xhigh reasoning support
    "gpt-5.1-mini",
    "gpt-oss-20b",
    "gpt-oss-120b",
];

/// Models that require the OpenAI Responses API
pub const RESPONSES_API_MODELS: &[&str] = &[
    GPT,
    GPT_6_ASTRA,
    GPT_6_SOL,
    GPT_6_1_SOL,
    GPT_6_LUNA,
    GPT_5_6,
    GPT_5_6_SOL,
    GPT_5_6_TERRA,
    GPT_5_6_LUNA,
    GPT_5_1,
    GPT_5_6_SOL,
    GPT_5_1_MINI,
];

/// Models that support the OpenAI reasoning parameter payload
pub const REASONING_MODELS: &[&str] = &[
    GPT,
    GPT_6_ASTRA,
    GPT_6_SOL,
    GPT_6_1_SOL,
    GPT_6_LUNA,
    GPT_5_6,
    GPT_5_6_SOL,
    GPT_5_6_TERRA,
    GPT_5_6_LUNA,
    GPT_5_1,
    GPT_5_6_SOL,
];

/// Models that support the native OpenAI `service_tier` request parameter.
pub(crate) const SERVICE_TIER_MODELS: &[&str] = RESPONSES_API_MODELS;

/// Models that do not expose structured tool calling on the OpenAI platform
pub const TOOL_UNAVAILABLE_MODELS: &[&str] = &[];

/// GPT-OSS models that use harmony tokenization
pub const HARMONY_MODELS: &[&str] = &[GPT_OSS_20B, GPT_OSS_120B];

/// Models that reject non-streaming requests on the OpenAI API. The provider
/// advertises no non-streaming capability for them and coerces generation to
/// an internally-streamed request (see `OpenAIProvider::requires_streaming_responses`).
pub const STREAMING_REQUIRED_MODELS: &[&str] = &[GPT, GPT_5_6_SOL, GPT_5_6_LUNA];

// Convenience constants for commonly used models
pub const GPT: &str = "gpt";
pub const GPT_6_ASTRA: &str = "gpt-6-astra";
pub const GPT_6_SOL: &str = "gpt-6-sol";
pub const GPT_6_1_SOL: &str = "gpt-6.1-sol";
pub const GPT_6_LUNA: &str = "gpt-6-luna";
pub const GPT_5_6_SOL: &str = "gpt-5.6-sol";
pub const GPT_5_6_TERRA: &str = "gpt-5.6-terra";
pub const GPT_5_6_LUNA: &str = "gpt-5.6-luna";
pub const GPT_5_6: &str = "gpt-5.6";
pub const GPT_5_1: &str = "gpt-5.1";
pub const GPT_5_1_MINI: &str = "gpt-5.1-mini";
pub const GPT_OSS_20B: &str = "gpt-oss-20b";
pub const GPT_OSS_120B: &str = "gpt-oss-120b";

// Deprecated constants retained for backward compatibility in tests and internal code
pub const GPT_5: &str = "gpt-5";
pub const GPT_5_MINI: &str = "gpt-5-mini";
pub const GPT_5_NANO: &str = "gpt-5-nano";
pub const GPT_5_CODEX: &str = "gpt-5-codex";
pub(crate) const GPT_5_1_CODEX: &str = "gpt-5.1-codex";
pub(crate) const GPT_5_1_CODEX_MAX: &str = "gpt-5.1-codex-max";
pub(crate) const GPT_5_1_CODEX_MINI: &str = "gpt-5.1-codex-mini";
pub const GPT_5_2_CODEX: &str = "gpt-5.2-codex";
const CODEX_MINI_LATEST: &str = "codex-mini-latest";
pub const O3: &str = "o3";
pub const O4_MINI: &str = "o4-mini";

/// Mapping of deprecated OpenAI model IDs to their recommended replacements.
///
/// Mappings follow the official OpenAI deprecation table:
/// <https://developers.openai.com/api/docs/deprecations>.
/// Replacements are chosen from models currently in `SUPPORTED_MODELS`.
pub const DEPRECATED_MODEL_REPLACEMENTS: &[(&str, &str, &str)] = &[
    // (deprecated_id, replacement_id, human-readable reason)
    // GPT-5 / o3 family — shut down Dec 11, 2026
    (GPT_5, "gpt-5.6-sol", "GPT-5 is deprecated; use GPT-5.6 Sol for frontier work"),
    (GPT_5_MINI, "gpt-5.6-terra", "GPT-5 Mini is deprecated; use GPT-5.6 Terra for balanced tasks"),
    (GPT_5_NANO, "gpt-5.6-luna", "GPT-5 Nano is deprecated; use GPT-5.6 Luna for lightweight tasks"),
    (O3, "gpt-5.6-sol", "o3 is deprecated; use GPT-5.6 Sol for reasoning tasks"),
    (O4_MINI, "gpt-5.6-terra", "o4-mini is deprecated; use GPT-5.6 Terra for fast reasoning"),
    // Codex variants — shut down July 23, 2026 (Apr 1, 2026 for 5.x/5.1.x).
    // Per OpenAI/GitHub Copilot deprecation notices, the direct successor is
    // gpt-5.3-codex (the current Codex-optimised model).
    (GPT_5_CODEX, GPT_5_6_SOL, "GPT-5 Codex is deprecated; use GPT-5.3 Codex for agentic coding"),
    (GPT_5_1_CODEX, GPT_5_6_SOL, "GPT-5.1 Codex is deprecated; use GPT-5.3 Codex for agentic coding"),
    (
        GPT_5_1_CODEX_MAX,
        GPT_5_6_SOL,
        "GPT-5.1 Codex Max is deprecated; use GPT-5.3 Codex for long-running coding",
    ),
    (
        GPT_5_1_CODEX_MINI,
        GPT_5_6_SOL,
        "GPT-5.1 Codex Mini is deprecated; use GPT-5.3 Codex for cost-effective coding",
    ),
    (GPT_5_2_CODEX, GPT_5_6_SOL, "GPT-5.2 Codex is deprecated; use GPT-5.3 Codex for agentic coding"),
    (
        CODEX_MINI_LATEST,
        GPT_5_6_SOL,
        "codex-mini-latest is deprecated; use GPT-5.3 Codex for cost-effective coding",
    ),
];

/// Returns the recommended replacement for a deprecated OpenAI model, if known.
///
/// Returns `Some((replacement_id, reason))` when the model is a known
/// deprecated ID, or `None` when it is either current or unrecognized.
#[must_use]
pub fn deprecated_model_replacement(model: &str) -> Option<(&'static str, &'static str)> {
    DEPRECATED_MODEL_REPLACEMENTS
        .iter()
        .find(|(deprecated, _, _)| *deprecated == model)
        .map(|(_, replacement, reason)| (*replacement, *reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deprecated_gpt5_maps_to_gpt56_sol() {
        let (replacement, reason) = deprecated_model_replacement(GPT_5).expect("gpt-5 is deprecated");
        assert_eq!(replacement, "gpt-5.6-sol");
        assert!(reason.contains("GPT-5.6 Sol"));
    }

    #[test]
    fn deprecated_codex_models_map_to_gpt53_codex() {
        for (deprecated, expected) in [
            (GPT_5_CODEX, GPT_5_6_SOL),
            (GPT_5_1_CODEX, GPT_5_6_SOL),
            (GPT_5_1_CODEX_MAX, GPT_5_6_SOL),
            (GPT_5_1_CODEX_MINI, GPT_5_6_SOL),
            (GPT_5_2_CODEX, GPT_5_6_SOL),
            (CODEX_MINI_LATEST, GPT_5_6_SOL),
        ] {
            let (replacement, _reason) =
                deprecated_model_replacement(deprecated).unwrap_or_else(|| panic!("{deprecated} should be deprecated"));
            assert_eq!(replacement, expected, "replacement for {deprecated}");
        }
    }

    #[test]
    fn current_models_have_no_replacement() {
        assert!(deprecated_model_replacement(GPT_5_6_SOL).is_none());
        assert!(deprecated_model_replacement(GPT_5_6_SOL).is_none());
        assert!(deprecated_model_replacement("gpt-5.6-sol").is_none());
    }

    #[test]
    fn unknown_models_have_no_replacement() {
        assert!(deprecated_model_replacement("nonexistent-model").is_none());
        assert!(deprecated_model_replacement("").is_none());
    }

    #[test]
    fn supported_models_excludes_deprecated() {
        let deprecated_ids: Vec<&str> = DEPRECATED_MODEL_REPLACEMENTS.iter().map(|(id, _, _)| *id).collect();
        for deprecated in &deprecated_ids {
            assert!(
                !SUPPORTED_MODELS.contains(deprecated),
                "deprecated model {deprecated} should not appear in SUPPORTED_MODELS"
            );
        }
    }

    #[test]
    fn replacement_targets_are_current_models() {
        // Every replacement must be a currently supported (non-deprecated) model.
        for (_, replacement, _) in DEPRECATED_MODEL_REPLACEMENTS {
            assert!(
                SUPPORTED_MODELS.contains(replacement),
                "replacement target {replacement} must be in SUPPORTED_MODELS"
            );
            assert!(
                deprecated_model_replacement(replacement).is_none(),
                "replacement target {replacement} must not itself be deprecated"
            );
        }
    }
}
