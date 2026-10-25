//! moonshot_presets — provider preset definitions for moonshot.

use super::super::ModelPreset;
use super::reasoning_preset;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn moonshot_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "kimi-k3.1-pro".to_string(),
            model: "kimi-k3.1-pro".to_string(),
            display_name: "Kimi K3.1 Pro (Moonshot)".to_string(),
            description: "Moonshot's 3T-class open-weight flagship with 1M context and always-on deep reasoning."
                .to_string(),
            provider: Provider::Moonshot,
            default_reasoning_effort: ReasoningEffortLevel::Max,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast, light reasoning"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep reasoning"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum reasoning (default)"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
        ModelPreset {
            id: "kimi-k3".to_string(),
            model: "kimi-k3".to_string(),
            display_name: "Kimi K3 (Moonshot)".to_string(),
            description:
                "Moonshot's 2.8T parameter flagship with 1M context, native vision, and always-on deep reasoning."
                    .to_string(),
            provider: Provider::Moonshot,
            default_reasoning_effort: ReasoningEffortLevel::Max,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast, light reasoning"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep reasoning"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum reasoning (default)"),
            ],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
    ]
}
