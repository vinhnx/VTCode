//! anthropic_presets — provider preset definitions for anthropic.

use super::super::ModelPreset;
use super::reasoning_preset;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn anthropic_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "claude-sonnet-5-5".to_string(),
            model: "claude-sonnet-5-5".to_string(),
            display_name: "Claude Sonnet 5.5".to_string(),
            description:
                "The best combination of speed and intelligence with adaptive thinking on by default, 1M context, 128K output, and between_tools as the lowest thinking setting"
                    .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::High, "Default adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::XHigh, "Extended capability for the hardest coding and agentic tasks"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum adaptive effort for intelligence-demanding tasks"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-sonnet-5".to_string(),
            model: "claude-sonnet-5".to_string(),
            display_name: "Claude Sonnet 5".to_string(),
            description:
                "The best combination of speed and intelligence with adaptive thinking on by default, 1M context, and new tokenizer"
                    .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::High, "Default adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::XHigh, "Extended capability for the hardest coding and agentic tasks"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum adaptive effort for intelligence-demanding tasks"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-fable-5".to_string(),
            model: "claude-fable-5".to_string(),
            display_name: "Claude Fable 5".to_string(),
            description:
                "Anthropic's most capable widely released model, for the most demanding reasoning and long-horizon agentic work"
                    .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::High, "Default adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::XHigh, "Extended capability for the most capability-sensitive workloads"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum adaptive effort for intelligence-demanding tasks"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-fable-5-1".to_string(),
            model: "claude-fable-5-1".to_string(),
            display_name: "Claude Fable 5.1".to_string(),
            description:
                "Successor to Claude Fable 5 for demanding reasoning and long-horizon agentic work, adaptive thinking always on, 1M context, 128K output, cache reads at 1/4 cost"
                    .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::High, "Default adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::XHigh, "Extended capability for the most capability-sensitive workloads"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum adaptive effort for intelligence-demanding tasks"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-opus-5".to_string(),
            model: "claude-opus-5".to_string(),
            display_name: "Claude Opus 5".to_string(),
            description:
                "Anthropic's newest Opus-tier model with 1M context, thinking on by default, and full effort ladder support"
                    .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::High, "Default adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::XHigh, "Extended capability for the hardest coding and agentic tasks"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum adaptive effort for intelligence-demanding tasks"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-opus-5-5".to_string(),
            model: "claude-opus-5-5".to_string(),
            display_name: "Claude Opus 5.5".to_string(),
            description:
                "Opus-tier successor for long-running agentic coding and knowledge work with adaptive thinking always on, 1M context, 128K output, and default medium effort"
                    .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Default adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::XHigh, "Extended capability for the hardest coding and agentic tasks"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum adaptive effort for intelligence-demanding tasks"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-haiku-5-5".to_string(),
            model: "claude-haiku-5-5".to_string(),
            display_name: "Claude Haiku 5.5".to_string(),
            description:
                "High-volume latency-sensitive work for classification, routing, extraction, and subagents with adaptive thinking, 1M context, 128K output, and default medium effort"
                    .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Default adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::XHigh, "Extended capability for the hardest coding and agentic tasks"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum adaptive effort for intelligence-demanding tasks"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-opus-5".to_string(),
            model: "claude-opus-5".to_string(),
            display_name: "Claude Opus 4.8".to_string(),
            description:
                "Anthropic's most capable model for complex reasoning, long-horizon agentic coding, and high-autonomy work"
                    .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::XHigh,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::XHigh, "Recommended Opus 4.8 effort for coding and agentic work"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum adaptive effort for intelligence-demanding tasks"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-sonnet-5".to_string(),
            model: "claude-sonnet-5".to_string(),
            display_name: "Claude Sonnet 4.6".to_string(),
            description:
                "The best combination of speed and intelligence. Supports extended thinking and adaptive thinking with 1M context."
                    .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::High, "Default adaptive effort"),
                reasoning_preset(ReasoningEffortLevel::Max, "Maximum adaptive effort"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-sonnet-5".to_string(),
            model: "claude-sonnet-5".to_string(),
            display_name: "Claude Haiku 4.5".to_string(),
            description: "The fastest model with near-frontier intelligence. Supports extended thinking with manual budget."
                .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: Vec::new(),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(200_000),
        },
    ]
}
