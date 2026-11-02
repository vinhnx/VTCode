//! zai_presets — provider preset definitions for zai.

use super::super::ModelPreset;
use super::reasoning_preset;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn zai_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "glm-5.3".to_string(),
            model: "glm-5.3".to_string(),
            display_name: "GLM-5.3".to_string(),
            description:
                "Z.ai flagship coding model with frontier long-horizon agentic performance and 1M-token context"
                    .to_string(),
            provider: Provider::ZAI,
            default_reasoning_effort: ReasoningEffortLevel::Max,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast, light reasoning"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep thinking"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum deep reasoning (default)"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "glm-5.3-flash".to_string(),
            model: "glm-5.3-flash".to_string(),
            display_name: "GLM-5.3 Flash".to_string(),
            description:
                "Z.ai efficient multimodal model with hybrid sparse+linear attention (320B total / 18B active), 1M context and native vision"
                    .to_string(),
            provider: Provider::ZAI,
            default_reasoning_effort: ReasoningEffortLevel::Max,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast, light reasoning"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep thinking"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum deep reasoning (default)"),
            ],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "glm-5.3-flashx".to_string(),
            model: "glm-5.3-flashx".to_string(),
            display_name: "GLM-5.3 FlashX".to_string(),
            description:
                "Z.ai high-speed Flash variant with faster inference (up to 200 tok/s), 320B total / 18B active, 1M context and native vision"
                    .to_string(),
            provider: Provider::ZAI,
            default_reasoning_effort: ReasoningEffortLevel::Max,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast, light reasoning"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep thinking"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum deep reasoning (default)"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "glm-5.2".to_string(),
            model: "glm-5.2".to_string(),
            display_name: "GLM-5.2".to_string(),
            description: "Z.ai flagship model for long-horizon tasks with truly usable 1M-token context".to_string(),
            provider: Provider::ZAI,
            default_reasoning_effort: ReasoningEffortLevel::Max,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast, light reasoning"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep thinking"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum deep reasoning (default)"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
    ]
}
