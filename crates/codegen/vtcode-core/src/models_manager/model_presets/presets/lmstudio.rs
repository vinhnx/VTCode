//! lmstudio_presets — provider preset definitions for lmstudio.

use super::super::ModelPreset;
use super::reasoning_preset;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn lmstudio_presets() -> Vec<ModelPreset> {
    use crate::config::constants::models::lmstudio as lmstudio_models;
    vec![
        ModelPreset {
            id: format!("lmstudio/{}", lmstudio_models::OPENAI_GPT_OSS_20B),
            model: lmstudio_models::OPENAI_GPT_OSS_20B.to_string(),
            display_name: "GPT-OSS 20B (LM Studio)".to_string(),
            description: "OpenAI's open-weight GPT-OSS 20B model served locally via LM Studio".to_string(),
            provider: Provider::LmStudio,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(ReasoningEffortLevel::Medium, "Balanced")],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
        ModelPreset {
            id: format!("lmstudio/{}", lmstudio_models::META_LLAMA_31_8B_INSTRUCT),
            model: lmstudio_models::META_LLAMA_31_8B_INSTRUCT.to_string(),
            display_name: "Llama 3.1 8B (LM Studio)".to_string(),
            description: "Meta Llama 3.1 8B Instruct for general-purpose local inference".to_string(),
            provider: Provider::LmStudio,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(ReasoningEffortLevel::Medium, "Balanced")],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
        ModelPreset {
            id: format!("lmstudio/{}", lmstudio_models::GEMMA_3_12B_IT),
            model: lmstudio_models::GEMMA_3_12B_IT.to_string(),
            display_name: "Gemma 3 12B (LM Studio)".to_string(),
            description: "Google Gemma 3 12B IT for local inference".to_string(),
            provider: Provider::LmStudio,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(ReasoningEffortLevel::Medium, "Balanced")],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(32_768),
        },
    ]
}
