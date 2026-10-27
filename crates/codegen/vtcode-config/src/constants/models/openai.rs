pub const DEFAULT_MODEL: &str = "gpt-5.6-sol";
pub const SUPPORTED_MODELS: &[&str] = &[
    GPT,
    GPT_6_ASTRA,
    GPT_6_SOL,
    GPT_6_1_SOL,
    GPT_6_1_TERRA,
    GPT_6_LUNA,
    GPT_5_6,       // GPT-5.6 alias (routes to gpt-5.6-sol)
    GPT_5_6_SOL,   // GPT-5.6 Sol flagship model
    GPT_5_6_TERRA, // GPT-5.6 Terra balanced model
    GPT_5_6_LUNA,  // GPT-5.6 Luna efficient model
    GPT_5_1_MINI,
    GPT_OSS_20B,
    GPT_OSS_120B,
];

/// Models that require the OpenAI Responses API
pub const RESPONSES_API_MODELS: &[&str] = &[
    GPT,
    GPT_6_ASTRA,
    GPT_6_SOL,
    GPT_6_1_SOL,
    GPT_6_1_TERRA,
    GPT_6_LUNA,
    GPT_5_6,
    GPT_5_6_SOL,
    GPT_5_6_TERRA,
    GPT_5_6_LUNA,
    GPT_5_1_MINI,
];

/// Models that support the OpenAI reasoning parameter payload
pub const REASONING_MODELS: &[&str] = &[
    GPT,
    GPT_6_ASTRA,
    GPT_6_SOL,
    GPT_6_1_SOL,
    GPT_6_1_TERRA,
    GPT_6_LUNA,
    GPT_5_6,
    GPT_5_6_SOL,
    GPT_5_6_TERRA,
    GPT_5_6_LUNA,
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
pub const GPT_6_1_TERRA: &str = "gpt-6.1-terra";
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

// Oct 2026 deprecation batch (announced 2026-10-01 / 2026-10-14).
pub(crate) const GPT_5_3_CODEX: &str = "gpt-5.3-codex";
pub(crate) const GPT_5_4_NANO: &str = "gpt-5.4-nano";
pub(crate) const GPT_4O: &str = "gpt-4o";
pub(crate) const GPT_4O_MINI: &str = "gpt-4o-mini";
pub(crate) const GPT_4_1: &str = "gpt-4.1";
pub(crate) const GPT_4_1_MINI: &str = "gpt-4.1-mini";

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
    // Oct 1, 2026 batch — shut down Apr 1, 2027.
    (GPT_5_1, "gpt-6-sol", "GPT-5.1 is deprecated and shuts down Apr 1, 2027; use GPT-6 Sol"),
    (
        GPT_5_3_CODEX,
        "gpt-6-sol",
        "GPT-5.3 Codex is deprecated and shuts down Apr 1, 2027; use GPT-6 Sol for agentic coding",
    ),
    (GPT_5_4_NANO, "gpt-6-luna", "GPT-5.4 Nano is deprecated and shuts down Apr 1, 2027; use GPT-6 Luna"),
    // Oct 14, 2026 GPT-4 series batch — shut down Jan 14, 2027.
    (GPT_4O, "gpt-6-luna", "GPT-4o is deprecated and shuts down Jan 14, 2027; use GPT-6 Luna"),
    (
        "gpt-4o-2024-08-06",
        "gpt-6-luna",
        "GPT-4o (2024-08-06) is deprecated and shuts down Jan 14, 2027; use GPT-6 Luna",
    ),
    (
        "gpt-4o-2024-11-20",
        "gpt-6-luna",
        "GPT-4o (2024-11-20) is deprecated and shuts down Jan 14, 2027; use GPT-6 Luna",
    ),
    (GPT_4O_MINI, "gpt-6-luna", "GPT-4o mini is deprecated and shuts down Jan 14, 2027; use GPT-6 Luna"),
    (
        "gpt-4o-mini-2024-07-18",
        "gpt-6-luna",
        "GPT-4o mini (2024-07-18) is deprecated and shuts down Jan 14, 2027; use GPT-6 Luna",
    ),
    (GPT_4_1, "gpt-6-luna", "GPT-4.1 is deprecated and shuts down Jan 14, 2027; use GPT-6 Luna"),
    (
        "gpt-4.1-2025-04-14",
        "gpt-6-luna",
        "GPT-4.1 (2025-04-14) is deprecated and shuts down Jan 14, 2027; use GPT-6 Luna",
    ),
    (GPT_4_1_MINI, "gpt-6-luna", "GPT-4.1 mini is deprecated and shuts down Jan 14, 2027; use GPT-6 Luna"),
    (
        "gpt-4.1-mini-2025-04-14",
        "gpt-6-luna",
        "GPT-4.1 mini (2025-04-14) is deprecated and shuts down Jan 14, 2027; use GPT-6 Luna",
    ),
    // Jun 11, 2026 GPT-5/o3 snapshots — shut down Dec 11, 2026.
    (
        "gpt-5-2025-08-07",
        "gpt-5.6-sol",
        "GPT-5 snapshot 2025-08-07 is deprecated and shuts down Dec 11, 2026; use GPT-5.6 Sol",
    ),
    (
        "gpt-5-mini-2025-08-07",
        "gpt-5.6-terra",
        "GPT-5 Mini snapshot 2025-08-07 is deprecated and shuts down Dec 11, 2026; use GPT-5.6 Terra",
    ),
    (
        "gpt-5-nano-2025-08-07",
        "gpt-5.6-luna",
        "GPT-5 Nano snapshot 2025-08-07 is deprecated and shuts down Dec 11, 2026; use GPT-5.6 Luna",
    ),
    (
        "gpt-5-pro-2025-10-06",
        "gpt-5.6-sol",
        "GPT-5 Pro snapshot 2025-10-06 is deprecated and shuts down Dec 11, 2026; use GPT-5.6 Sol",
    ),
    (
        "o3-2025-04-16",
        "gpt-5.6-sol",
        "o3 snapshot 2025-04-16 is deprecated and shuts down Dec 11, 2026; use GPT-5.6 Sol",
    ),
    (
        "o3-pro-2025-06-10",
        "gpt-5.6-sol",
        "o3 Pro snapshot 2025-06-10 is deprecated and shuts down Dec 11, 2026; use GPT-5.6 Sol",
    ),
    // Apr 22, 2026 legacy snapshots — shut down Oct 23, 2026.
    (
        "gpt-3.5-turbo-0125",
        "gpt-5.6-terra",
        "GPT-3.5 Turbo snapshot 0125 is deprecated and shuts down Oct 23, 2026; use GPT-5.6 Terra",
    ),
    (
        "gpt-4-0613",
        "gpt-5.6-sol",
        "GPT-4 snapshot 0613 is deprecated and shuts down Oct 23, 2026; use GPT-5.6 Sol",
    ),
    (
        "gpt-4-turbo-2024-04-09",
        "gpt-5.6-sol",
        "GPT-4 Turbo snapshot 2024-04-09 is deprecated and shuts down Oct 23, 2026; use GPT-5.6 Sol",
    ),
    (
        "gpt-4.1-nano-2025-04-14",
        "gpt-5.6-luna",
        "GPT-4.1 Nano is deprecated and shuts down Oct 23, 2026; use GPT-5.6 Luna",
    ),
    (
        "gpt-4o-2024-05-13",
        "gpt-5.6-sol",
        "GPT-4o snapshot 2024-05-13 is deprecated and shuts down Oct 23, 2026; use GPT-5.6 Sol",
    ),
    ("o1-2024-12-17", "gpt-5.6-sol", "o1 is deprecated and shuts down Oct 23, 2026; use GPT-5.6 Sol"),
    (
        "o1-pro-2025-03-19",
        "gpt-5.6-sol",
        "o1 Pro is deprecated and shuts down Oct 23, 2026; use GPT-5.6 Sol",
    ),
    (
        "o3-mini-2025-01-31",
        "gpt-5.6-sol",
        "o3 Mini is deprecated and shuts down Oct 23, 2026; use GPT-5.6 Sol",
    ),
    (
        "o4-mini-2025-04-16",
        "gpt-5.6-terra",
        "o4 Mini snapshot 2025-04-16 is deprecated and shuts down Oct 23, 2026; use GPT-5.6 Terra",
    ),
    // Chat / specialized variants already shut down.
    ("gpt-5-chat-latest", "gpt-5.6-sol", "GPT-5 Chat is deprecated; use GPT-5.6 Sol"),
    ("gpt-5.1-chat-latest", "gpt-5.6-sol", "GPT-5.1 Chat is deprecated; use GPT-5.6 Sol"),
    (
        "gpt-5.2-chat-latest",
        "gpt-5.6-sol",
        "GPT-5.2 Chat is deprecated and shut down Aug 10, 2026; use GPT-5.6 Sol",
    ),
    (
        "gpt-5.3-chat-latest",
        "gpt-5.6-sol",
        "GPT-5.3 Chat is deprecated and shut down Aug 10, 2026; use GPT-5.6 Sol",
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

    #[test]
    fn capability_lists_have_no_duplicates() {
        // `RESPONSES_API_MODELS` is joined into user-facing help text and
        // `SERVICE_TIER_MODELS` aliases it, so duplicates reach users.
        for (list_name, list) in [
            ("SUPPORTED_MODELS", SUPPORTED_MODELS),
            ("RESPONSES_API_MODELS", RESPONSES_API_MODELS),
            ("REASONING_MODELS", REASONING_MODELS),
            ("STREAMING_REQUIRED_MODELS", STREAMING_REQUIRED_MODELS),
            ("HARMONY_MODELS", HARMONY_MODELS),
        ] {
            let mut seen = std::collections::HashSet::new();
            for model in list {
                assert!(seen.insert(*model), "{list_name} lists {model} more than once");
            }
        }
    }

    #[test]
    fn capability_lists_stay_within_supported_models() {
        // A capability list must never advertise a model the catalog dropped.
        for (list_name, list) in [
            ("RESPONSES_API_MODELS", RESPONSES_API_MODELS),
            ("REASONING_MODELS", REASONING_MODELS),
            ("STREAMING_REQUIRED_MODELS", STREAMING_REQUIRED_MODELS),
            ("HARMONY_MODELS", HARMONY_MODELS),
        ] {
            for model in list {
                assert!(
                    SUPPORTED_MODELS.contains(model),
                    "{list_name} advertises {model}, which is not in SUPPORTED_MODELS"
                );
            }
        }
    }
}
