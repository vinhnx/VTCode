//! openai_presets — provider preset definitions for openai.

use super::super::{ModelPreset, ReasoningEffortPreset};
use super::reasoning_preset;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;

fn openai_reasoning_efforts(include_none: bool, include_xhigh: bool, include_max: bool) -> Vec<ReasoningEffortPreset> {
    let mut efforts = Vec::new();
    if include_none {
        efforts.push(reasoning_preset(ReasoningEffortLevel::None, "Lowest latency"));
    }
    efforts.push(reasoning_preset(ReasoningEffortLevel::Low, "Fast"));
    efforts.push(reasoning_preset(ReasoningEffortLevel::Medium, "Balanced"));
    efforts.push(reasoning_preset(ReasoningEffortLevel::High, "Deep"));
    if include_xhigh {
        efforts.push(reasoning_preset(ReasoningEffortLevel::XHigh, "Maximum reasoning"));
    }
    if include_max {
        efforts.push(reasoning_preset(ReasoningEffortLevel::Max, "Maximum adaptive reasoning"));
    }
    efforts
}
pub(crate) fn openai_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "gpt-6-astra".to_string(),
            model: "gpt-6-astra".to_string(),
            display_name: "GPT-6 Astra".to_string(),
            description: "Most capable model for hardest end-to-end work with complex reasoning, coding, computer use, and research".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: openai_reasoning_efforts(false, true, true),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_050_000),
        },
        ModelPreset {
            id: "gpt-6-sol".to_string(),
            model: "gpt-6-sol".to_string(),
            display_name: "GPT-6 Sol".to_string(),
            description: "Cost-efficient high-end GPT-6 model for demanding professional work".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: openai_reasoning_efforts(true, true, true),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_050_000),
        },
        ModelPreset {
            id: "gpt-6.1-sol".to_string(),
            model: "gpt-6.1-sol".to_string(),
            display_name: "GPT-6.1 Sol".to_string(),
            description: "Near-Astra performance for complex coding, computer use, and professional work at a lower cost".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: openai_reasoning_efforts(false, true, true),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_050_000),
        },
        ModelPreset {
            id: "gpt-6-luna".to_string(),
            model: "gpt-6-luna".to_string(),
            display_name: "GPT-6 Luna".to_string(),
            description: "Fast cost-efficient GPT-6 model for high-volume latency-sensitive workloads".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: openai_reasoning_efforts(true, true, true),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_050_000),
        },
        ModelPreset {
            id: "gpt-5.6-sol".to_string(),
            model: "gpt-5.6-sol".to_string(),
            display_name: "GPT-5.6 Sol".to_string(),
            description: "Frontier model for complex professional work in the GPT-5.6 family".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: openai_reasoning_efforts(true, true, true),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_050_000),
        },
        ModelPreset {
            id: "gpt-5.6-terra".to_string(),
            model: "gpt-5.6-terra".to_string(),
            display_name: "GPT-5.6 Terra".to_string(),
            description: "GPT-5.6 model that balances intelligence and cost".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: openai_reasoning_efforts(true, true, true),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_050_000),
        },
        ModelPreset {
            id: "gpt-5.6-luna".to_string(),
            model: "gpt-5.6-luna".to_string(),
            display_name: "GPT-5.6 Luna".to_string(),
            description: "GPT-5.6 model optimized for cost-sensitive workloads".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: openai_reasoning_efforts(true, true, true),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_050_000),
        },
        ModelPreset {
            id: "gpt-5.6-sol".to_string(),
            model: "gpt-5.6-sol".to_string(),
            display_name: "GPT-5.4".to_string(),
            description: "Frontier model for complex professional work".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::None,
            supported_reasoning_efforts: openai_reasoning_efforts(true, true, false),
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_050_000),
        },
        ModelPreset {
            id: "gpt-5.6-sol".to_string(),
            model: "gpt-5.6-sol".to_string(),
            display_name: "GPT-5.4 Pro".to_string(),
            description: "Higher-compute GPT-5.4 variant for tougher problems".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep"),
                reasoning_preset(ReasoningEffortLevel::XHigh, "Maximum reasoning"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_050_000),
        },
        ModelPreset {
            id: "gpt-5-codex".to_string(),
            model: "gpt-5-codex".to_string(),
            display_name: "GPT-5.3 Codex".to_string(),
            description: "GPT-5.3 variant optimized for agentic coding with xhigh reasoning".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: openai_reasoning_efforts(true, true, false),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(272_000),
        },
        ModelPreset {
            id: "gpt-5.1-mini".to_string(),
            model: "gpt-5.1-mini".to_string(),
            display_name: "GPT-5.1 Mini".to_string(),
            description: "Compact GPT-5.1 variant for cost-effective tasks".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: openai_reasoning_efforts(true, false, false),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(272_000),
        },
        ModelPreset {
            id: "gpt-5.6".to_string(),
            model: "gpt-5.6".to_string(),
            display_name: "GPT-5.2".to_string(),
            description: "Previous frontier model with improved reasoning and coding".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::None,
            supported_reasoning_efforts: openai_reasoning_efforts(true, true, false),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(272_000),
        },
        ModelPreset {
            id: "gpt-oss-20b".to_string(),
            model: "gpt-oss-20b".to_string(),
            display_name: "GPT-OSS 20B".to_string(),
            description: "OpenAI's open-source 20B parameter model".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Low,
                    description: "Fast".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep".to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
        ModelPreset {
            id: "gpt-oss-120b".to_string(),
            model: "gpt-oss-120b".to_string(),
            display_name: "GPT-OSS 120B".to_string(),
            description: "OpenAI's open-source 120B parameter model with advanced reasoning".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Low,
                    description: "Fast".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep".to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
    ]
}
