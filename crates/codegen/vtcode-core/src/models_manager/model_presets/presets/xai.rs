use super::super::ModelPreset;
use super::reasoning_preset;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;

pub(crate) fn xai_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "grok-4.7".to_string(),
            model: "grok-4.7".to_string(),
            display_name: "Grok 4.7".to_string(),
            description: "xAI's flagship reasoning model with reasoning_effort support (500k context)".to_string(),
            provider: Provider::XAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast, less thinking"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced thinking"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep thinking (default)"),
                reasoning_preset(ReasoningEffortLevel::XHigh, "Maximum reasoning depth (grok-4.6 and later)"),
            ],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(500_000),
        },
        ModelPreset {
            id: "grok-4.6".to_string(),
            model: "grok-4.6".to_string(),
            display_name: "Grok 4.6".to_string(),
            description: "xAI's flagship reasoning model with reasoning_effort support (500k context)".to_string(),
            provider: Provider::XAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast, less thinking"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced thinking"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep thinking (default)"),
                reasoning_preset(ReasoningEffortLevel::XHigh, "Maximum reasoning depth (grok-4.6 and later)"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(500_000),
        },
        ModelPreset {
            id: "grok-4.5".to_string(),
            model: "grok-4.5".to_string(),
            display_name: "Grok 4.5".to_string(),
            description: "xAI's flagship reasoning model with reasoning_effort support (500k context)".to_string(),
            provider: Provider::XAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast, less thinking"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced thinking"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep thinking (default)"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(500_000),
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
