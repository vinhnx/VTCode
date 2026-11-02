//! minimax_presets — provider preset definitions for minimax.

use super::super::ModelPreset;
use super::reasoning_ladder_medium_high;
use super::reasoning_preset;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn minimax_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "minimax-m3".to_string(),
            model: "MiniMax-M3".to_string(),
            display_name: "MiniMax M3".to_string(),
            description: "Frontier multimodal coding model with 1M context".to_string(),
            provider: Provider::Minimax,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: reasoning_ladder_medium_high(),
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "minimax-m3.1-flash".to_string(),
            model: "MiniMax-M3.1-Flash".to_string(),
            display_name: "MiniMax M3.1 Flash".to_string(),
            description: "Latest ultra-efficient multimodal model with 1M context and tunable thinking depth"
                .to_string(),
            provider: Provider::Minimax,
            default_reasoning_effort: ReasoningEffortLevel::Max,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep"),
                reasoning_preset(ReasoningEffortLevel::XHigh, "Extra-deep"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum depth (model default)"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
    ]
}
