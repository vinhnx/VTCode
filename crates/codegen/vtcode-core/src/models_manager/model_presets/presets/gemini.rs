//! gemini_presets — provider preset definitions for gemini.

use super::super::ModelPreset;
use super::reasoning_ladder_low_medium_high_verbose;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn gemini_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "gemini-4-argon".to_string(),
            model: "gemini-4-argon".to_string(),
            display_name: "Gemini 4 Argon".to_string(),
            description:
                "Flagship reasoning model for the hardest coding, agentic, math, and science problems with a unified 1M context window and long decode"
                    .to_string(),
            provider: Provider::Gemini,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: reasoning_ladder_low_medium_high_verbose(),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
        ModelPreset {
            id: "gemini-3.8-flash".to_string(),
            model: "gemini-3.8-flash".to_string(),
            display_name: "Gemini 3.8 Flash".to_string(),
            description:
                "Most intelligent Flash for long-horizon SWE, agents, and enterprise workflows with 1M context"
                    .to_string(),
            provider: Provider::Gemini,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: reasoning_ladder_low_medium_high_verbose(),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
        ModelPreset {
            id: "gemini-3-flash-preview".to_string(),
            model: "gemini-3-flash-preview".to_string(),
            display_name: "Gemini 3 Flash Preview".to_string(),
            description: "Most intelligent model built for speed with superior search and grounding".to_string(),
            provider: Provider::Gemini,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: reasoning_ladder_low_medium_high_verbose(),
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
    ]
}
