//! anthropic_presets — provider preset definitions for anthropic.

use super::super::{ModelPreset, ReasoningEffortPreset};
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;
pub(crate) fn anthropic_presets() -> Vec<ModelPreset> {
    vec![
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
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Low,
                    description: "Fast adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Default adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::XHigh,
                    description: "Extended capability for the hardest coding and agentic tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Max,
                    description: "Maximum adaptive effort for intelligence-demanding tasks"
                        .to_string(),
                },
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
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Low,
                    description: "Fast adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Default adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::XHigh,
                    description: "Extended capability for the most capability-sensitive workloads".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Max,
                    description: "Maximum adaptive effort for intelligence-demanding tasks"
                        .to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-mythos-5".to_string(),
            model: "claude-mythos-5".to_string(),
            display_name: "Claude Mythos 5".to_string(),
            description:
                "Shares Claude Fable 5's capabilities without safety classifiers. Available through Project Glasswing (limited access)."
                    .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Low,
                    description: "Fast adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Default adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::XHigh,
                    description: "Extended capability for the most capability-sensitive workloads".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Max,
                    description: "Maximum adaptive effort for intelligence-demanding tasks"
                        .to_string(),
                },
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
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Low,
                    description: "Fast adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Default adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::XHigh,
                    description: "Extended capability for the hardest coding and agentic tasks".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Max,
                    description: "Maximum adaptive effort for intelligence-demanding tasks"
                        .to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-opus-4-8".to_string(),
            model: "claude-opus-4-8".to_string(),
            display_name: "Claude Opus 4.8".to_string(),
            description:
                "Anthropic's most capable model for complex reasoning, long-horizon agentic coding, and high-autonomy work"
                    .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::XHigh,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Low,
                    description: "Fast adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::XHigh,
                    description: "Recommended Opus 4.8 effort for coding and agentic work".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Max,
                    description: "Maximum adaptive effort for intelligence-demanding tasks"
                        .to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-sonnet-4-6".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            display_name: "Claude Sonnet 4.6".to_string(),
            description:
                "The best combination of speed and intelligence. Supports extended thinking and adaptive thinking with 1M context."
                    .to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Low,
                    description: "Fast adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Default adaptive effort".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Max,
                    description: "Maximum adaptive effort".to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-haiku-4-5".to_string(),
            model: "claude-haiku-4-5".to_string(),
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
