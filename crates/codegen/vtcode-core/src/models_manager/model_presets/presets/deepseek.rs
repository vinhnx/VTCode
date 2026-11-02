//! deepseek_presets — provider preset definitions for deepseek.

use super::super::ModelPreset;
use super::reasoning_preset;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn deepseek_presets() -> Vec<ModelPreset> {
    vec![ModelPreset {
        id: "deepseek-v4.1-flash".to_string(),
        model: "deepseek-v4.1-flash".to_string(),
        display_name: "DeepSeek V4.1 Flash".to_string(),
        description: "Latest DeepSeek flash model with improved reasoning, efficiency, and agent capabilities"
            .to_string(),
        provider: Provider::DeepSeek,
        default_reasoning_effort: ReasoningEffortLevel::High,
        supported_reasoning_efforts: vec![
            reasoning_preset(ReasoningEffortLevel::Low, "Fast, light reasoning"),
            reasoning_preset(ReasoningEffortLevel::High, "Balanced"),
            reasoning_preset(ReasoningEffortLevel::Max, "Maximum thinking"),
        ],
        is_default: true,
        upgrade: None,
        show_in_picker: true,
        supported_in_api: true,
        context_window: Some(1_000_000),
    }]
}
