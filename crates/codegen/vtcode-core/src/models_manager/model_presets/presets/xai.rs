use super::super::{ModelPreset, ReasoningEffortPreset};
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;

pub(crate) fn xai_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "grok-4.5".to_string(),
            model: "grok-4.5".to_string(),
            display_name: "Grok 4.5".to_string(),
            description: "xAI's flagship reasoning model with reasoning_effort support (500k context)".to_string(),
            provider: Provider::XAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Low,
                    description: "Fast, less thinking".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced thinking".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep thinking (default)".to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(500_000),
        },
        ModelPreset {
            id: "grok-build-0.1".to_string(),
            model: "grok-build-0.1".to_string(),
            display_name: "Grok Build 0.1".to_string(),
            description: "xAI's fast coding model for agentic software engineering (256k context)".to_string(),
            provider: Provider::XAI,
            default_reasoning_effort: ReasoningEffortLevel::None,
            supported_reasoning_efforts: vec![],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(256_000),
        },
        ModelPreset {
            id: "grok-4.3".to_string(),
            model: "grok-4.3".to_string(),
            display_name: "Grok 4.3".to_string(),
            description: "xAI's balanced model with 1M context for complex tasks".to_string(),
            provider: Provider::XAI,
            default_reasoning_effort: ReasoningEffortLevel::None,
            supported_reasoning_efforts: vec![],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
    ]
}
