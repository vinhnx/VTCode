//! llamacpp_presets — provider preset definitions for llamacpp.

use super::super::ModelPreset;
use super::reasoning_preset;
use crate::config::constants::models::llamacpp as llamacpp_models;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn llamacpp_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: format!("llamacpp/{}", llamacpp_models::GPT_OSS_20B),
            model: llamacpp_models::GPT_OSS_20B.to_string(),
            display_name: "GPT-OSS 20B (llama.cpp)".to_string(),
            description: "OpenAI's open-weight GPT-OSS 20B model served locally through llama.cpp".to_string(),
            provider: Provider::LlamaCpp,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(ReasoningEffortLevel::Medium, "Balanced")],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
        ModelPreset {
            id: format!("llamacpp/{}", llamacpp_models::GEMMA_4_26B_A4B),
            model: llamacpp_models::GEMMA_4_26B_A4B.to_string(),
            display_name: "Gemma 4 26B A4B (llama.cpp)".to_string(),
            description: "Gemma 4 desktop MoE model served through llama.cpp".to_string(),
            provider: Provider::LlamaCpp,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(ReasoningEffortLevel::Medium, "Balanced")],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(262_144),
        },
        ModelPreset {
            id: format!("llamacpp/{}", llamacpp_models::GEMMA_4_E4B),
            model: llamacpp_models::GEMMA_4_E4B.to_string(),
            display_name: "Gemma 4 E4B (llama.cpp)".to_string(),
            description: "Tiny-footprint Gemma 4 model served through llama.cpp".to_string(),
            provider: Provider::LlamaCpp,
            default_reasoning_effort: ReasoningEffortLevel::Low,
            supported_reasoning_efforts: vec![reasoning_preset(ReasoningEffortLevel::Low, "Fast")],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
    ]
}
