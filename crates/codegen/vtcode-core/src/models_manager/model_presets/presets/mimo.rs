//! mimo_presets — provider preset definitions for mimo.

use super::super::ModelPreset;
use super::reasoning_preset;
use crate::config::constants::models::mimo as mimo_models;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn mimo_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: mimo_models::MIMO_V2_6_PRO.to_string(),
            model: mimo_models::MIMO_V2_6_PRO.to_string(),
            display_name: "MiMo V2.6 Pro".to_string(),
            description: "Xiaomi's flagship reasoning model with advanced capabilities (1M context)".to_string(),
            provider: Provider::MiMo,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(ReasoningEffortLevel::Medium, "Balanced")],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
        ModelPreset {
            id: mimo_models::MIMO_V2_6_FLASH.to_string(),
            model: mimo_models::MIMO_V2_6_FLASH.to_string(),
            display_name: "MiMo V2.6 Flash".to_string(),
            description: "Xiaomi's efficient high-volume model for professional office scenarios (1M context)"
                .to_string(),
            provider: Provider::MiMo,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(ReasoningEffortLevel::Medium, "Balanced")],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
        ModelPreset {
            id: mimo_models::MIMO_V2_6_PRO_ULTRASPEED.to_string(),
            model: mimo_models::MIMO_V2_6_PRO_ULTRASPEED.to_string(),
            display_name: "MiMo V2.6 Pro UltraSpeed".to_string(),
            description: "Xiaomi's fastest flagship variant for real-time production scenarios (1M context)"
                .to_string(),
            provider: Provider::MiMo,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(ReasoningEffortLevel::Medium, "Balanced")],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
    ]
}
