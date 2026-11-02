//! stepfun_presets — provider preset definitions for stepfun.

use super::super::ModelPreset;
use super::reasoning_ladder_low_medium_high;
use crate::config::constants::models::stepfun as stepfun_models;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn stepfun_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: stepfun_models::STEP_3_7_FLASH.to_string(),
            model: stepfun_models::STEP_3_7_FLASH.to_string(),
            display_name: "Step 3.7 Flash".to_string(),
            description: "StepFun's flagship multimodal reasoning model with 256K context and tool calling."
                .to_string(),
            provider: Provider::StepFun,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: reasoning_ladder_low_medium_high(),
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(262_144),
        },
        ModelPreset {
            id: stepfun_models::STEP_5_PREVIEW.to_string(),
            model: stepfun_models::STEP_5_PREVIEW.to_string(),
            display_name: "Step 5 Preview".to_string(),
            description:
                "StepFun's frontier model for production-scale Agent applications with 1M context and tool calling."
                    .to_string(),
            provider: Provider::StepFun,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: reasoning_ladder_low_medium_high(),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
    ]
}
