use super::super::{ModelPreset, ReasoningEffortPreset};
use crate::config::constants::models;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;

fn merge_reasoning_presets() -> Vec<ReasoningEffortPreset> {
    vec![
        ReasoningEffortPreset {
            effort: ReasoningEffortLevel::Minimal,
            description: "Minimal reasoning depth".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffortLevel::Low,
            description: "Fast, less thinking".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffortLevel::Medium,
            description: "Balanced thinking".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffortLevel::High,
            description: "Deep thinking".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffortLevel::XHigh,
            description: "Extra-high reasoning depth".to_string(),
        },
        ReasoningEffortPreset {
            effort: ReasoningEffortLevel::Max,
            description: "Maximum reasoning depth".to_string(),
        },
    ]
}

pub(crate) fn merge_gateway_presets() -> Vec<ModelPreset> {
    let reasoning_routes = models::merge_gateway::REASONING_MODELS;
    [
        (
            models::merge_gateway::DEFAULT_ROUTING,
            "Default Routing (Merge Gateway)",
            "Merge Gateway automatically selects a configured route for the request",
            128_000,
            true,
        ),
        (
            models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5,
            "Claude Opus 5 (Merge Gateway)",
            "Anthropic Claude Opus 5 through Merge Gateway",
            1_000_000,
            false,
        ),
        (
            models::merge_gateway::ANTHROPIC_CLAUDE_OPUS_5_5,
            "Claude Opus 5.5 (Merge Gateway)",
            "Anthropic Claude Opus 5.5 through Merge Gateway",
            1_000_000,
            false,
        ),
        (
            models::merge_gateway::ANTHROPIC_CLAUDE_SONNET_5,
            "Claude Sonnet 5 (Merge Gateway)",
            "Anthropic Claude Sonnet 5 through Merge Gateway",
            1_000_000,
            false,
        ),
        (
            models::merge_gateway::ANTHROPIC_CLAUDE_SONNET_5_5,
            "Claude Sonnet 5.5 (Merge Gateway)",
            "Anthropic Claude Sonnet 5.5 through Merge Gateway",
            1_000_000,
            false,
        ),
        (
            models::merge_gateway::ANTHROPIC_CLAUDE_HAIKU_5_5,
            "Claude Haiku 5.5 (Merge Gateway)",
            "Anthropic Claude Haiku 5.5 for high-volume latency-sensitive work through Merge Gateway",
            1_000_000,
            false,
        ),
        (
            models::merge_gateway::ANTHROPIC_CLAUDE_FABLE_5_1,
            "Claude Fable 5.1 (Merge Gateway)",
            "Anthropic Claude Fable 5.1 through Merge Gateway",
            1_000_000,
            false,
        ),
        (
            models::merge_gateway::GOOGLE_GEMINI_3_8_FLASH,
            "Gemini 3.8 Flash (Merge Gateway)",
            "Google Gemini 3.8 Flash through Merge Gateway",
            1_000_000,
            false,
        ),
        (
            models::merge_gateway::DEEPSEEK_FLASH,
            "DeepSeek V4.1 Flash (Merge Gateway)",
            "DeepSeek V4.1 Flash through Merge Gateway",
            1_000_000,
            false,
        ),
        (
            models::merge_gateway::XAI_GROK_4_6,
            "Grok 4.6 (Merge Gateway)",
            "xAI Grok 4.6 through Merge Gateway",
            500_000,
            false,
        ),
        (
            models::merge_gateway::XAI_GROK_4_7,
            "Grok 4.7 (Merge Gateway)",
            "xAI Grok 4.7 through Merge Gateway",
            500_000,
            false,
        ),
        (
            models::merge_gateway::MINIMAX_H3,
            "MiniMax H3 (Merge Gateway)",
            "MiniMax H3 through Merge Gateway",
            131_000,
            false,
        ),
        (
            models::merge_gateway::MOONSHOT_KIMI_K3,
            "Kimi K3 (Merge Gateway)",
            "Moonshot Kimi K3 through Merge Gateway",
            1_000_000,
            false,
        ),
        (
            models::merge_gateway::THINKINGMACHINES_INKLING,
            "Inkling (Merge Gateway)",
            "Thinking Machines Inkling through Merge Gateway",
            1_000_000,
            false,
        ),
        (
            models::merge_gateway::ZAI_GLM_5_3_FLASH,
            "GLM-5.3 Flash (Merge Gateway)",
            "Z.AI GLM-5.3 Flash through Merge Gateway",
            1_310_720,
            false,
        ),
        (
            models::merge_gateway::ZAI_GLM_5_3_FLASHX,
            "GLM-5.3 FlashX (Merge Gateway)",
            "Z.AI GLM-5.3 FlashX high-speed variant through Merge Gateway",
            1_048_576,
            false,
        ),
        (
            models::merge_gateway::OPENAI_GPT_5_6_LUNA,
            "GPT-5.6 Luna (Merge Gateway)",
            "OpenAI GPT-5.6 Luna through Merge Gateway",
            1_100_000,
            false,
        ),
        (
            models::merge_gateway::OPENAI_GPT_5_6_SOL,
            "GPT-5.6 Sol (Merge Gateway)",
            "OpenAI GPT-5.6 Sol through Merge Gateway",
            1_100_000,
            false,
        ),
        (
            models::merge_gateway::OPENAI_GPT_5_6_TERRA,
            "GPT-5.6 Terra (Merge Gateway)",
            "OpenAI GPT-5.6 Terra through Merge Gateway",
            1_100_000,
            false,
        ),
        (
            models::merge_gateway::OPENAI_GPT_6_ASTRA,
            "GPT-6 Astra (Merge Gateway)",
            "OpenAI GPT-6 Astra through Merge Gateway",
            1_050_000,
            false,
        ),
        (
            models::merge_gateway::OPENAI_GPT_6_SOL,
            "GPT-6 Sol (Merge Gateway)",
            "OpenAI GPT-6 Sol through Merge Gateway",
            1_050_000,
            false,
        ),
        (
            models::merge_gateway::OPENAI_GPT_6_1_SOL,
            "GPT-6.1 Sol (Merge Gateway)",
            "OpenAI GPT-6.1 Sol through Merge Gateway",
            1_050_000,
            false,
        ),
        (
            models::merge_gateway::OPENAI_GPT_6_LUNA,
            "GPT-6 Luna (Merge Gateway)",
            "OpenAI GPT-6 Luna through Merge Gateway",
            1_050_000,
            false,
        ),
        (
            models::merge_gateway::XIAOMIMIMO_MIMO_V2_6_PRO,
            "MiMo V2.6 Pro (Merge Gateway)",
            "Xiaomi MiMo V2.6 Pro through Merge Gateway",
            1_048_576,
            false,
        ),
        (
            models::merge_gateway::XIAOMIMIMO_MIMO_V2_6_FLASH,
            "MiMo V2.6 Flash (Merge Gateway)",
            "Xiaomi MiMo V2.6 Flash through Merge Gateway",
            1_048_576,
            false,
        ),
        (
            models::merge_gateway::MISTRAL_LARGE_4,
            "Mistral Large 4 (Merge Gateway)",
            "Mistral Large 4 open-weight MoE flagship with 1M context through Merge Gateway",
            1_000_000,
            false,
        ),
    ]
    .into_iter()
    .map(|(model, display_name, description, context_window, is_default)| {
        let supports_reasoning = reasoning_routes.contains(&model);
        ModelPreset {
            id: model.to_string(),
            model: model.to_string(),
            display_name: display_name.to_string(),
            description: description.to_string(),
            provider: Provider::MergeGateway,
            default_reasoning_effort: if supports_reasoning {
                ReasoningEffortLevel::Medium
            } else {
                ReasoningEffortLevel::None
            },
            supported_reasoning_efforts: if supports_reasoning {
                merge_reasoning_presets()
            } else {
                Vec::new()
            },
            is_default,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(context_window),
        }
    })
    .collect()
}
