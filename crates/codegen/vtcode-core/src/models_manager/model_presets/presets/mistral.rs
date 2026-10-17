//! mistral_presets — provider preset definitions for mistral.

use super::super::{ModelPreset, ReasoningEffortPreset};
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn mistral_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "mistral-large-2512".to_string(),
            model: "mistral-large-2512".to_string(),
            display_name: "Mistral Large 3".to_string(),
            description: "State-of-the-art open-weight general-purpose multimodal model (41B active, 675B total)"
                .to_string(),
            provider: Provider::Mistral,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep".to_string(),
                },
            ],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(256_000),
        },
        ModelPreset {
            id: "mistral-large-4".to_string(),
            model: "mistral-large-4".to_string(),
            display_name: "Mistral Large 4".to_string(),
            description: "Open-weight MoE flagship (49B active, 1.05T total) with 1M context (Public Preview)"
                .to_string(),
            provider: Provider::Mistral,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep".to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "mistral-medium-3-5".to_string(),
            model: "mistral-medium-3-5".to_string(),
            display_name: "Mistral Medium 3.5".to_string(),
            description: "Frontier-class multimodal model optimized for agentic and coding use cases (256k context)"
                .to_string(),
            provider: Provider::Mistral,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep".to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(256_000),
        },
        ModelPreset {
            id: "mistral-small-2603".to_string(),
            model: "mistral-small-2603".to_string(),
            display_name: "Mistral Small 4".to_string(),
            description: "Hybrid model unifying instruct, reasoning, and coding (119B params, 6.5B active)".to_string(),
            provider: Provider::Mistral,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep".to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(256_000),
        },
        ModelPreset {
            id: "mistral-medium-2508".to_string(),
            model: "mistral-medium-2508".to_string(),
            display_name: "Mistral Medium 3.1".to_string(),
            description: "Frontier-class multimodal model with 256k context".to_string(),
            provider: Provider::Mistral,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(256_000),
        },
        ModelPreset {
            id: "codestral-2508".to_string(),
            model: "codestral-2508".to_string(),
            display_name: "Codestral".to_string(),
            description: "Cutting-edge language model for code completion".to_string(),
            provider: Provider::Mistral,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: Vec::new(),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(256_000),
        },
    ]
}
