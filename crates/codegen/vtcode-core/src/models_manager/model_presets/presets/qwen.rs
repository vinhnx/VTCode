//! qwen_presets — provider preset definitions for qwen.

use super::super::ModelPreset;
use super::reasoning_preset;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn qwen_presets() -> Vec<ModelPreset> {
    // Qwen's Rust catalog is a single alias route (`models::qwen::DEFAULT_MODEL`
    // == `models::deepseek::DEEPSEEK_FLASH`); display strings come from the
    // `DeepSeekFlash` model table entry and the effort ladder from the catalog
    // `reasoning_efforts` for `deepseek-flash`.
    vec![ModelPreset {
        id: "deepseek-flash".to_string(),
        model: "deepseek-flash".to_string(),
        display_name: "DeepSeek V4.1 Flash".to_string(),
        description: "Latest flash model with improved reasoning, efficiency, and agent capabilities".to_string(),
        provider: Provider::Qwen,
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
