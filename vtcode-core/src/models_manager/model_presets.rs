//! Model presets and built-in model configurations.
//!
//! This module provides pre-configured model presets for all supported providers,
//! following the pattern from OpenAI Codex's models_manager.

use serde::{Deserialize, Serialize};

use crate::config::constants::models::copilot as copilot_models;
use crate::config::constants::models::evolink as evolink_models;
use crate::config::constants::models::llamacpp as llamacpp_models;
use crate::config::constants::models::mimo as mimo_models;
use crate::config::constants::models::poolside as poolside_models;
use crate::config::constants::models::qwen as qwen_models;
use crate::config::constants::models::stepfun as stepfun_models;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;

/// Reasoning effort preset with description
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningEffortPreset {
    /// The effort level
    pub effort: ReasoningEffortLevel,
    /// Human-readable description
    pub description: String,
}

/// Model upgrade information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelUpgrade {
    /// Target model ID to upgrade to
    pub id: String,
    /// Optional reasoning effort mapping
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort_mapping: Option<String>,
    /// Configuration key for migration
    pub migration_config_key: String,
    /// Link to model documentation
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_link: Option<String>,
    /// Upgrade notification copy
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade_copy: Option<String>,
}

/// Remote model information received from provider APIs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Unique model identifier/slug
    pub slug: String,
    /// Display name for UI
    pub display_name: String,
    /// Model description
    pub description: String,
    /// Provider this model belongs to
    pub provider: Provider,
    /// Default reasoning level
    #[serde(default)]
    pub default_reasoning_level: ReasoningEffortLevel,
    /// Supported reasoning levels
    #[serde(default)]
    pub supported_reasoning_levels: Vec<ReasoningEffortPreset>,
    /// Context window size
    #[serde(default)]
    pub context_window: Option<i64>,
    /// Whether this model supports tool use
    #[serde(default = "default_true")]
    pub supports_tool_use: bool,
    /// Whether this model supports streaming
    #[serde(default = "default_true")]
    pub supports_streaming: bool,
    /// Whether this model supports reasoning/thinking
    #[serde(default)]
    pub supports_reasoning: bool,
    /// Priority for sorting (lower = higher priority)
    #[serde(default)]
    pub priority: i32,
    /// Visibility in picker
    #[serde(default = "default_visibility")]
    pub visibility: String,
    /// Whether supported in API mode
    #[serde(default = "default_true")]
    pub supported_in_api: bool,
    /// Upgrade path if available
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade: Option<ModelUpgrade>,
}

fn default_true() -> bool {
    true
}

fn default_visibility() -> String {
    "list".to_string()
}

/// A preset configuration for a model shown in the picker
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelPreset {
    /// Unique identifier for the preset
    pub id: String,
    /// Actual model slug to use in API calls
    pub model: String,
    /// Display name for UI
    pub display_name: String,
    /// Model description
    pub description: String,
    /// Provider
    pub provider: Provider,
    /// Default reasoning effort
    pub default_reasoning_effort: ReasoningEffortLevel,
    /// Supported reasoning efforts
    pub supported_reasoning_efforts: Vec<ReasoningEffortPreset>,
    /// Whether this is the default model
    #[serde(default)]
    pub is_default: bool,
    /// Upgrade path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade: Option<ModelUpgrade>,
    /// Whether to show in picker
    #[serde(default = "default_true")]
    pub show_in_picker: bool,
    /// Whether supported in API mode
    #[serde(default = "default_true")]
    pub supported_in_api: bool,
    /// Context window size
    #[serde(default)]
    pub context_window: Option<i64>,
}

impl From<ModelInfo> for ModelPreset {
    fn from(info: ModelInfo) -> Self {
        Self {
            id: info.slug.clone(),
            model: info.slug,
            display_name: info.display_name,
            description: info.description,
            provider: info.provider,
            default_reasoning_effort: info.default_reasoning_level,
            supported_reasoning_efforts: info.supported_reasoning_levels,
            is_default: false,
            upgrade: info.upgrade,
            show_in_picker: info.visibility == "list",
            supported_in_api: info.supported_in_api,
            context_window: info.context_window,
        }
    }
}

/// Get built-in model presets for the given provider
pub fn builtin_model_presets() -> Vec<ModelPreset> {
    let mut presets = Vec::new();

    // Gemini presets
    presets.extend(gemini_presets());

    // OpenAI presets
    presets.extend(openai_presets());

    // Anthropic presets
    presets.extend(anthropic_presets());

    // Copilot presets
    presets.extend(copilot_presets());

    // DeepSeek presets
    presets.extend(deepseek_presets());

    // Z.AI presets
    presets.extend(zai_presets());

    // LM Studio presets
    presets.extend(lmstudio_presets());

    // llama.cpp presets
    presets.extend(llamacpp_presets());

    // MiniMax presets
    presets.extend(minimax_presets());

    // OpenCode Zen presets
    presets.extend(opencode_zen_presets());

    // OpenCode Go presets
    presets.extend(opencode_go_presets());

    // Poolside presets
    presets.extend(poolside_presets());

    // StepFun presets
    presets.extend(stepfun_presets());

    // Evolink presets
    presets.extend(evolink_presets());

    presets
}

/// Get presets for a specific provider
pub fn presets_for_provider(provider: Provider) -> Vec<ModelPreset> {
    match provider {
        Provider::Gemini => gemini_presets(),
        Provider::OpenAI => openai_presets(),
        Provider::Anthropic => anthropic_presets(),
        Provider::Copilot => copilot_presets(),
        Provider::DeepSeek => deepseek_presets(),
        Provider::ZAI => zai_presets(),
        Provider::Minimax => minimax_presets(),
        Provider::OpenRouter => openrouter_presets(),
        Provider::Ollama => ollama_presets(),
        Provider::LmStudio => lmstudio_presets(),
        Provider::LlamaCpp => llamacpp_presets(),
        Provider::Moonshot => moonshot_presets(),
        Provider::Mistral => mistral_presets(),
        Provider::HuggingFace => huggingface_presets(),
        Provider::OpenCodeZen => opencode_zen_presets(),
        Provider::OpenCodeGo => opencode_go_presets(),
        Provider::MiMo => mimo_presets(),
        Provider::Qwen => qwen_presets(),
        Provider::StepFun => stepfun_presets(),
        Provider::Evolink => evolink_presets(),
        Provider::Poolside => poolside_presets(),
    }
}

fn copilot_presets() -> Vec<ModelPreset> {
    vec![ModelPreset {
        id: copilot_models::AUTO.to_string(),
        model: copilot_models::AUTO.to_string(),
        display_name: "GitHub Copilot Auto".to_string(),
        description:
            "Official GitHub Copilot preview provider via the Copilot CLI with automatic model selection."
                .to_string(),
        provider: Provider::Copilot,
        default_reasoning_effort: ReasoningEffortLevel::Medium,
        supported_reasoning_efforts: Vec::new(),
        is_default: true,
        upgrade: None,
        show_in_picker: true,
        supported_in_api: true,
        context_window: Some(400_000),
    }]
}

fn gemini_presets() -> Vec<ModelPreset> {
    vec![ModelPreset {
        id: "gemini-3-flash-preview".to_string(),
        model: "gemini-3-flash-preview".to_string(),
        display_name: "Gemini 3 Flash Preview".to_string(),
        description: "Most intelligent model built for speed with superior search and grounding"
            .to_string(),
        provider: Provider::Gemini,
        default_reasoning_effort: ReasoningEffortLevel::Medium,
        supported_reasoning_efforts: vec![
            ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Low,
                description: "Fast responses".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced reasoning".to_string(),
            },
            ReasoningEffortPreset {
                effort: ReasoningEffortLevel::High,
                description: "Deep reasoning".to_string(),
            },
        ],
        is_default: true,
        upgrade: None,
        show_in_picker: true,
        supported_in_api: true,
        context_window: Some(1_048_576),
    }]
}

fn reasoning_preset(
    effort: ReasoningEffortLevel,
    description: &'static str,
) -> ReasoningEffortPreset {
    ReasoningEffortPreset {
        effort,
        description: description.to_string(),
    }
}

fn openai_reasoning_efforts(include_none: bool, include_xhigh: bool) -> Vec<ReasoningEffortPreset> {
    let mut efforts = Vec::new();
    if include_none {
        efforts.push(reasoning_preset(
            ReasoningEffortLevel::None,
            "Lowest latency",
        ));
    }
    efforts.push(reasoning_preset(ReasoningEffortLevel::Low, "Fast"));
    efforts.push(reasoning_preset(ReasoningEffortLevel::Medium, "Balanced"));
    efforts.push(reasoning_preset(ReasoningEffortLevel::High, "Deep"));
    if include_xhigh {
        efforts.push(reasoning_preset(
            ReasoningEffortLevel::XHigh,
            "Maximum reasoning",
        ));
    }
    efforts
}

fn openai_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "gpt-5.4".to_string(),
            model: "gpt-5.4".to_string(),
            display_name: "GPT-5.4".to_string(),
            description: "Frontier model for complex professional work".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::None,
            supported_reasoning_efforts: openai_reasoning_efforts(true, true),
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_050_000),
        },
        ModelPreset {
            id: "gpt-5.4-pro".to_string(),
            model: "gpt-5.4-pro".to_string(),
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
            id: "gpt-5.3-codex".to_string(),
            model: "gpt-5.3-codex".to_string(),
            display_name: "GPT-5.3 Codex".to_string(),
            description: "GPT-5.3 variant optimized for agentic coding with xhigh reasoning"
                .to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: openai_reasoning_efforts(true, true),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(272_000),
        },
        ModelPreset {
            id: "gpt-5.2-codex".to_string(),
            model: "gpt-5.2-codex".to_string(),
            display_name: "GPT-5.2 Codex".to_string(),
            description: "GPT-5.2 variant optimized for agentic coding with xhigh reasoning"
                .to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: openai_reasoning_efforts(true, true),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(272_000),
        },
        ModelPreset {
            id: "gpt-5.1-codex".to_string(),
            model: "gpt-5.1-codex".to_string(),
            display_name: "GPT-5.1 Codex".to_string(),
            description: "GPT-5.1 variant optimized for agentic coding".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: openai_reasoning_efforts(false, false),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(272_000),
        },
        ModelPreset {
            id: "gpt-5.1-codex-max".to_string(),
            model: "gpt-5.1-codex-max".to_string(),
            display_name: "GPT-5.1 Codex Max".to_string(),
            description:
                "Higher-compute GPT-5.1 Codex variant for longer-running engineering tasks"
                    .to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: openai_reasoning_efforts(false, false),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(272_000),
        },
        ModelPreset {
            id: "gpt-5-codex".to_string(),
            model: "gpt-5-codex".to_string(),
            display_name: "GPT-5 Codex".to_string(),
            description: "GPT-5 variant optimized for agentic coding".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: openai_reasoning_efforts(false, false),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(272_000),
        },
        ModelPreset {
            id: "gpt-5.2".to_string(),
            model: "gpt-5.2".to_string(),
            display_name: "GPT-5.2".to_string(),
            description: "Latest frontier model with improved reasoning and coding".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::None,
            supported_reasoning_efforts: openai_reasoning_efforts(true, true),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(272_000),
        },
        ModelPreset {
            id: "gpt-5".to_string(),
            model: "gpt-5".to_string(),
            display_name: "GPT-5".to_string(),
            description: "Latest most capable OpenAI model".to_string(),
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
            context_window: Some(200_000),
        },
        ModelPreset {
            id: "gpt-5-mini".to_string(),
            model: "gpt-5-mini".to_string(),
            display_name: "GPT-5 Mini".to_string(),
            description: "Efficient GPT-5 variant".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(128_000),
        },
        ModelPreset {
            id: "gpt-5-nano".to_string(),
            model: "gpt-5-nano".to_string(),
            display_name: "GPT-5 Nano".to_string(),
            description: "Most cost-effective GPT-5 variant for high-volume tasks".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(200_000),
        },
        ModelPreset {
            id: "o3".to_string(),
            model: "o3".to_string(),
            display_name: "o3".to_string(),
            description: "OpenAI reasoning model for harder multi-step work".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: openai_reasoning_efforts(false, false),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: None,
        },
        ModelPreset {
            id: "o4-mini".to_string(),
            model: "o4-mini".to_string(),
            display_name: "o4-mini".to_string(),
            description: "Smaller OpenAI reasoning model with strong tool use".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: openai_reasoning_efforts(false, false),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: None,
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
            description: "OpenAI's open-source 120B parameter model with advanced reasoning"
                .to_string(),
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

fn anthropic_presets() -> Vec<ModelPreset> {
    vec![
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
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-opus-4-7".to_string(),
            model: "claude-opus-4-7".to_string(),
            display_name: "Claude Opus 4.7".to_string(),
            description:
                "Next-gen Anthropic flagship with adaptive thinking, optional task budgets, and configurable low-through-max effort"
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
                    description: "Default Opus 4.7 effort for coding and agentic work".to_string(),
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
            id: "claude-mythos-preview".to_string(),
            model: "claude-mythos-preview".to_string(),
            display_name: "Claude Mythos Preview".to_string(),
            description:
                "Invitation-only Anthropic preview with adaptive thinking and support for max effort"
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
            show_in_picker: false,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "claude-opus-4-6".to_string(),
            model: "claude-opus-4-6".to_string(),
            display_name: "Claude Opus 4.6".to_string(),
            description:
                "Previous Anthropic flagship now using adaptive thinking by default, with legacy manual-budget fallback"
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

fn deepseek_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "deepseek-v4-pro".to_string(),
            model: "deepseek-v4-pro".to_string(),
            display_name: "DeepSeek V4 Pro".to_string(),
            description: "High-performance reasoning model with advanced thinking capabilities"
                .to_string(),
            provider: Provider::DeepSeek,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Max,
                    description: "Maximum thinking".to_string(),
                },
            ],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "deepseek-v4-flash".to_string(),
            model: "deepseek-v4-flash".to_string(),
            display_name: "DeepSeek V4 Flash".to_string(),
            description: "Fast inference model for cost-effective reasoning tasks".to_string(),
            provider: Provider::DeepSeek,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Max,
                    description: "Maximum thinking".to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
    ]
}

fn zai_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "glm-5".to_string(),
            model: "glm-5".to_string(),
            display_name: "GLM-5".to_string(),
            description: "Z.ai's flagship open-source foundation model for complex systems"
                .to_string(),
            provider: Provider::ZAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep thinking".to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(200_000),
        },
        ModelPreset {
            id: "glm-5.1".to_string(),
            model: "glm-5.1".to_string(),
            display_name: "GLM-5.1".to_string(),
            description:
                "Z.ai's next-gen foundation model with improved reasoning and agent capabilities"
                    .to_string(),
            provider: Provider::ZAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep thinking".to_string(),
                },
            ],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(200_000),
        },
    ]
}

fn mistral_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "mistral-large-2512".to_string(),
            model: "mistral-large-2512".to_string(),
            display_name: "Mistral Large 3".to_string(),
            description:
                "State-of-the-art open-weight general-purpose multimodal model (41B active, 675B total)".to_string(),
            provider: Provider::Mistral,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep".to_string(),
                },
            ],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(256_000),
        },
        ModelPreset {
            id: "mistral-medium-3-5".to_string(),
            model: "mistral-medium-3-5".to_string(),
            display_name: "Mistral Medium 3.5".to_string(),
            description:
                "Frontier-class multimodal model optimized for agentic and coding use cases (256k context)"
                    .to_string(),
            provider: Provider::Mistral,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
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
            context_window: Some(256_000),
        },
        ModelPreset {
            id: "mistral-small-2603".to_string(),
            model: "mistral-small-2603".to_string(),
            display_name: "Mistral Small 4".to_string(),
            description:
                "Hybrid model unifying instruct, reasoning, and coding (119B params, 6.5B active)"
                    .to_string(),
            provider: Provider::Mistral,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
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
            context_window: Some(256_000),
        },
        ModelPreset {
            id: "mistral-medium-2508".to_string(),
            model: "mistral-medium-2508".to_string(),
            display_name: "Mistral Medium 3.1".to_string(),
            description: "Frontier-class multimodal model with 256k context".to_string(),
            provider: Provider::Mistral,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(256_000),
        },
        ModelPreset {
            id: "codestral-2508".to_string(),
            model: "codestral-2508".to_string(),
            display_name: "Codestral".to_string(),
            description: "Cutting-edge language model for code completion".to_string(),
            provider: Provider::Mistral,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: Vec::new(),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(256_000),
        },
    ]
}

fn minimax_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "minimax-m3".to_string(),
            model: "MiniMax-M3".to_string(),
            display_name: "MiniMax M3".to_string(),
            description: "Frontier multimodal coding model with 1M context".to_string(),
            provider: Provider::Minimax,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep".to_string(),
                },
            ],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "minimax-m2.5".to_string(),
            model: "MiniMax-M2.5".to_string(),
            display_name: "MiniMax M2.5".to_string(),
            description: "Enhanced code understanding and reasoning".to_string(),
            provider: Provider::Minimax,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
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
            context_window: Some(128_000),
        },
    ]
}

fn openrouter_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "openrouter/deepseek/deepseek-chat".to_string(),
            model: "deepseek/deepseek-chat".to_string(),
            display_name: "DeepSeek V3.2 (OpenRouter)".to_string(),
            description: "DeepSeek via OpenRouter".to_string(),
            provider: Provider::OpenRouter,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(128_000),
        },
        ModelPreset {
            id: "openrouter/moonshotai/kimi-k2.6".to_string(),
            model: "moonshotai/kimi-k2.6".to_string(),
            display_name: "Kimi K2.6 (OpenRouter)".to_string(),
            description: "Kimi K2.6 multimodal agentic model via OpenRouter".to_string(),
            provider: Provider::OpenRouter,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(262_144),
        },
        ModelPreset {
            id: "openrouter/qwen/qwen3.7-max".to_string(),
            model: "qwen/qwen3.7-max".to_string(),
            display_name: "Qwen3.7 Max (OpenRouter)".to_string(),
            description: "Qwen3.7 Max flagship model for coding and agentic workloads via OpenRouter".to_string(),
            provider: Provider::OpenRouter,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "openrouter/tencent/hy3-preview".to_string(),
            model: "tencent/hy3-preview".to_string(),
            display_name: "Hy3 Preview (OpenRouter)".to_string(),
            description: "Tencent Hy3 high-efficiency MoE model with configurable reasoning via OpenRouter".to_string(),
            provider: Provider::OpenRouter,
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
            context_window: Some(262_144),
        },
        ModelPreset {
            id: "openrouter/x-ai/grok-build-0.1".to_string(),
            model: "x-ai/grok-build-0.1".to_string(),
            display_name: "Grok Build 0.1 (OpenRouter)".to_string(),
            description: "xAI Grok Build 0.1 coding model for agentic software engineering via OpenRouter".to_string(),
            provider: Provider::OpenRouter,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(256_000),
        },
        ModelPreset {
            id: "openrouter/xiaomi/mimo-v2.5".to_string(),
            model: "xiaomi/mimo-v2.5".to_string(),
            display_name: "MiMo-V2.5 (OpenRouter)".to_string(),
            description: "Xiaomi MiMo-V2.5 omnimodal agentic model for complex software engineering via OpenRouter".to_string(),
            provider: Provider::OpenRouter,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "openrouter/xiaomi/mimo-v2.5-pro".to_string(),
            model: "xiaomi/mimo-v2.5-pro".to_string(),
            display_name: "MiMo-V2.5-Pro (OpenRouter)".to_string(),
            description: "Xiaomi MiMo-V2.5-Pro flagship agentic model for complex software engineering via OpenRouter".to_string(),
            provider: Provider::OpenRouter,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "openrouter/poolside/laguna-m.1:free".to_string(),
            model: "poolside/laguna-m.1:free".to_string(),
            display_name: "Laguna M.1 free (OpenRouter)".to_string(),
            description: "Poolside Laguna M.1 flagship free coding agent model via OpenRouter".to_string(),
            provider: Provider::OpenRouter,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(262_144),
        },
    ]
}

fn ollama_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "ollama/gpt-oss:20b".to_string(),
            model: "gpt-oss:20b".to_string(),
            display_name: "GPT-OSS 20B (Ollama)".to_string(),
            description: "Open-weight GPT-OSS served locally".to_string(),
            provider: Provider::Ollama,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(96_000),
        },
        ModelPreset {
            id: "ollama/deepseek-v4-flash:cloud".to_string(),
            model: "deepseek-v4-flash:cloud".to_string(),
            display_name: "DeepSeek V4 Flash (Ollama)".to_string(),
            description: "Fast inference DeepSeek V4 Flash model via Ollama Cloud".to_string(),
            provider: Provider::Ollama,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(128_000),
        },
        ModelPreset {
            id: "ollama/deepseek-v4-pro:cloud".to_string(),
            model: "deepseek-v4-pro:cloud".to_string(),
            display_name: "DeepSeek V4 Pro (Ollama)".to_string(),
            description: "High-performance DeepSeek V4 Pro model via Ollama Cloud".to_string(),
            provider: Provider::Ollama,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(128_000),
        },
        ModelPreset {
            id: "ollama/minimax-m3:cloud".to_string(),
            model: "minimax-m3:cloud".to_string(),
            display_name: "MiniMax-M3 (Ollama)".to_string(),
            description: "Cloud-hosted MiniMax-M3 model via Ollama Cloud".to_string(),
            provider: Provider::Ollama,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
    ]
}

fn lmstudio_presets() -> Vec<ModelPreset> {
    use crate::config::constants::models::lmstudio as lmstudio_models;
    vec![
        ModelPreset {
            id: format!("lmstudio/{}", lmstudio_models::DEEPSEEK_R1_0528_QWEN3_8B),
            model: lmstudio_models::DEEPSEEK_R1_0528_QWEN3_8B.to_string(),
            display_name: "DeepSeek R1 0528 Qwen3 8B (LM Studio)".to_string(),
            description: "DeepSeek R1 distill on Qwen3 8B, reasoning-capable local model"
                .to_string(),
            provider: Provider::LmStudio,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
        ModelPreset {
            id: format!("lmstudio/{}", lmstudio_models::QWEN3_8B),
            model: lmstudio_models::QWEN3_8B.to_string(),
            display_name: "Qwen 3 8B (LM Studio)".to_string(),
            description: "Qwen 3 8B with thinking mode support for local inference".to_string(),
            provider: Provider::LmStudio,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
        ModelPreset {
            id: format!("lmstudio/{}", lmstudio_models::OPENAI_GPT_OSS_20B),
            model: lmstudio_models::OPENAI_GPT_OSS_20B.to_string(),
            display_name: "GPT-OSS 20B (LM Studio)".to_string(),
            description: "OpenAI's open-weight GPT-OSS 20B model served locally via LM Studio"
                .to_string(),
            provider: Provider::LmStudio,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
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
            description: "Meta Llama 3.1 8B Instruct for general-purpose local inference"
                .to_string(),
            provider: Provider::LmStudio,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
        ModelPreset {
            id: format!("lmstudio/{}", lmstudio_models::QWEN25_7B_INSTRUCT),
            model: lmstudio_models::QWEN25_7B_INSTRUCT.to_string(),
            display_name: "Qwen 2.5 7B (LM Studio)".to_string(),
            description: "Qwen 2.5 7B Instruct with tool calling support".to_string(),
            provider: Provider::LmStudio,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(32_768),
        },
        ModelPreset {
            id: format!("lmstudio/{}", lmstudio_models::GEMMA_3_12B_IT),
            model: lmstudio_models::GEMMA_3_12B_IT.to_string(),
            display_name: "Gemma 3 12B (LM Studio)".to_string(),
            description: "Google Gemma 3 12B IT for local inference".to_string(),
            provider: Provider::LmStudio,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(32_768),
        },
    ]
}

fn llamacpp_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: format!("llamacpp/{}", llamacpp_models::GPT_OSS_20B),
            model: llamacpp_models::GPT_OSS_20B.to_string(),
            display_name: "GPT-OSS 20B (llama.cpp)".to_string(),
            description: "OpenAI's open-weight GPT-OSS 20B model served locally through llama.cpp"
                .to_string(),
            provider: Provider::LlamaCpp,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
        ModelPreset {
            id: format!("llamacpp/{}", llamacpp_models::QWEN36_27B),
            model: llamacpp_models::QWEN36_27B.to_string(),
            display_name: "Qwen 3.6 27B (llama.cpp)".to_string(),
            description: "Dense Qwen 3.6 local model served through llama.cpp".to_string(),
            provider: Provider::LlamaCpp,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(262_144),
        },
        ModelPreset {
            id: format!("llamacpp/{}", llamacpp_models::QWEN36_35B_A3B),
            model: llamacpp_models::QWEN36_35B_A3B.to_string(),
            display_name: "Qwen 3.6 35B A3B (llama.cpp)".to_string(),
            description: "Qwen 3.6 MoE local model served through llama.cpp".to_string(),
            provider: Provider::LlamaCpp,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(262_144),
        },
        ModelPreset {
            id: format!("llamacpp/{}", llamacpp_models::GEMMA_4_26B_A4B),
            model: llamacpp_models::GEMMA_4_26B_A4B.to_string(),
            display_name: "Gemma 4 26B A4B (llama.cpp)".to_string(),
            description: "Gemma 4 desktop MoE model served through llama.cpp".to_string(),
            provider: Provider::LlamaCpp,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
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
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Low,
                description: "Fast".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
        ModelPreset {
            id: format!("llamacpp/{}", llamacpp_models::STEP_3_5_FLASH),
            model: llamacpp_models::STEP_3_5_FLASH.to_string(),
            display_name: "Step 3.5 Flash (llama.cpp)".to_string(),
            description: "StepFun's efficient reasoning model served through llama.cpp".to_string(),
            provider: Provider::LlamaCpp,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(262_144),
        },
    ]
}

fn opencode_zen_presets() -> Vec<ModelPreset> {
    vec![ModelPreset {
        id: "opencode/gpt-5.4".to_string(),
        model: "gpt-5.4".to_string(),
        display_name: "GPT-5.4 (OpenCode Zen)".to_string(),
        description: "OpenCode Zen gateway — curated, benchmarked models at cost".to_string(),
        provider: Provider::OpenCodeZen,
        default_reasoning_effort: ReasoningEffortLevel::Medium,
        supported_reasoning_efforts: vec![ReasoningEffortPreset {
            effort: ReasoningEffortLevel::Medium,
            description: "Balanced".to_string(),
        }],
        is_default: true,
        upgrade: None,
        show_in_picker: true,
        supported_in_api: true,
        context_window: Some(1_050_000),
    }]
}

fn opencode_go_presets() -> Vec<ModelPreset> {
    vec![ModelPreset {
        id: "opencode-go/kimi-k2.5".to_string(),
        model: "kimi-k2.5".to_string(),
        display_name: "Kimi K2.5 (OpenCode Go)".to_string(),
        description: "OpenCode Go — affordable subscription for open coding models".to_string(),
        provider: Provider::OpenCodeGo,
        default_reasoning_effort: ReasoningEffortLevel::Medium,
        supported_reasoning_efforts: vec![ReasoningEffortPreset {
            effort: ReasoningEffortLevel::Medium,
            description: "Balanced".to_string(),
        }],
        is_default: true,
        upgrade: None,
        show_in_picker: true,
        supported_in_api: true,
        context_window: Some(256_000),
    }]
}

fn poolside_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: poolside_models::LAGUNA_M1.to_string(),
            model: poolside_models::LAGUNA_M1.to_string(),
            display_name: "Laguna M.1".to_string(),
            description:
                "Poolside's flagship MoE coding agent model optimized for multi-step agentic tasks, tool use, and validation (128K context)"
                    .to_string(),
            provider: Provider::Poolside,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: Vec::new(),
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
        ModelPreset {
            id: poolside_models::LAGUNA_XS2.to_string(),
            model: poolside_models::LAGUNA_XS2.to_string(),
            display_name: "Laguna XS.2".to_string(),
            description:
                "Poolside's efficient MoE coding agent model optimized for fast agentic coding (128K context)"
                    .to_string(),
            provider: Provider::Poolside,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: Vec::new(),
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
    ]
}

fn mimo_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: mimo_models::MIMO_V2_5_PRO.to_string(),
            model: mimo_models::MIMO_V2_5_PRO.to_string(),
            display_name: "MiMo V2.5 Pro".to_string(),
            description:
                "Xiaomi's flagship reasoning model with advanced capabilities (1M context)"
                    .to_string(),
            provider: Provider::MiMo,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
        ModelPreset {
            id: mimo_models::MIMO_V2_5.to_string(),
            model: mimo_models::MIMO_V2_5.to_string(),
            display_name: "MiMo V2.5".to_string(),
            description: "Xiaomi's general-purpose model with strong reasoning (1M context)"
                .to_string(),
            provider: Provider::MiMo,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
        ModelPreset {
            id: mimo_models::MIMO_V2_FLASH.to_string(),
            model: mimo_models::MIMO_V2_FLASH.to_string(),
            display_name: "MiMo V2 Flash".to_string(),
            description: "Xiaomi's lightweight fast model for high-throughput tasks (256K context)"
                .to_string(),
            provider: Provider::MiMo,
            default_reasoning_effort: ReasoningEffortLevel::Low,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Low,
                description: "Fast".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(262_144),
        },
    ]
}

fn qwen_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: qwen_models::QWEN3_7_MAX.to_string(),
            model: qwen_models::QWEN3_7_MAX.to_string(),
            display_name: "Qwen 3.7 Max".to_string(),
            description:
                "Alibaba Cloud's flagship reasoning model with 131K context and advanced thinking"
                    .to_string(),
            provider: Provider::Qwen,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
        ModelPreset {
            id: qwen_models::QWEN3_6_FLASH.to_string(),
            model: qwen_models::QWEN3_6_FLASH.to_string(),
            display_name: "Qwen 3.6 Flash".to_string(),
            description:
                "Alibaba Cloud's fast inference model with 1M context, optimized for speed and cost-efficiency"
                    .to_string(),
            provider: Provider::Qwen,
            default_reasoning_effort: ReasoningEffortLevel::Low,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Low,
                description: "Fast".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
        ModelPreset {
            id: qwen_models::QWEN3_6_PLUS.to_string(),
            model: qwen_models::QWEN3_6_PLUS.to_string(),
            display_name: "Qwen 3.6 Plus".to_string(),
            description:
                "Alibaba Cloud's balanced model with 131K context, strong reasoning and coding performance"
                    .to_string(),
            provider: Provider::Qwen,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
        ModelPreset {
            id: qwen_models::DEEPSEEK_V4_FLASH.to_string(),
            model: qwen_models::DEEPSEEK_V4_FLASH.to_string(),
            display_name: "DeepSeek V4 Flash (Qwen)".to_string(),
            description:
                "DeepSeek V4 Flash fast inference model served through Qwen Cloud API (1M context)"
                    .to_string(),
            provider: Provider::Qwen,
            default_reasoning_effort: ReasoningEffortLevel::Low,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Low,
                description: "Fast".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
        ModelPreset {
            id: qwen_models::DEEPSEEK_V4_PRO.to_string(),
            model: qwen_models::DEEPSEEK_V4_PRO.to_string(),
            display_name: "DeepSeek V4 Pro (Qwen)".to_string(),
            description:
                "DeepSeek V4 Pro high-performance reasoning model served through Qwen Cloud API (1M context)"
                    .to_string(),
            provider: Provider::Qwen,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_048_576),
        },
        ModelPreset {
            id: qwen_models::GLM_5_1.to_string(),
            model: qwen_models::GLM_5_1.to_string(),
            display_name: "GLM-5.1 (Qwen)".to_string(),
            description:
                "Z.AI GLM-5.1 next-gen foundation model served through Qwen Cloud API"
                    .to_string(),
            provider: Provider::Qwen,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Balanced".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(131_072),
        },
    ]
}

fn stepfun_presets() -> Vec<ModelPreset> {
    vec![ModelPreset {
        id: stepfun_models::STEP_3_7_FLASH.to_string(),
        model: stepfun_models::STEP_3_7_FLASH.to_string(),
        display_name: "Step 3.7 Flash".to_string(),
        description:
            "StepFun's flagship multimodal reasoning model with 256K context and tool calling."
                .to_string(),
        provider: Provider::StepFun,
        default_reasoning_effort: ReasoningEffortLevel::Medium,
        supported_reasoning_efforts: vec![
            reasoning_preset(ReasoningEffortLevel::Low, "Fast"),
            reasoning_preset(ReasoningEffortLevel::Medium, "Balanced"),
            reasoning_preset(ReasoningEffortLevel::High, "Deep"),
        ],
        is_default: true,
        upgrade: None,
        show_in_picker: true,
        supported_in_api: true,
        context_window: Some(262_144),
    }]
}

fn evolink_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "evolink/gpt-5.2".to_string(),
            model: evolink_models::GPT_5_2.to_string(),
            display_name: "GPT-5.2 (Evolink)".to_string(),
            description: "GPT-5.2 served through the Evolink OpenAI-compatible gateway."
                .to_string(),
            provider: Provider::Evolink,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(
                ReasoningEffortLevel::Medium,
                "Balanced",
            )],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(400_000),
        },
        ModelPreset {
            id: "evolink/gpt-5.5".to_string(),
            model: evolink_models::GPT_5_5.to_string(),
            display_name: "GPT-5.5 (Evolink)".to_string(),
            description: "GPT-5.5 flagship model served through the Evolink gateway.".to_string(),
            provider: Provider::Evolink,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(
                ReasoningEffortLevel::Medium,
                "Balanced",
            )],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(400_000),
        },
        ModelPreset {
            id: "evolink/deepseek-v4-pro".to_string(),
            model: evolink_models::DEEPSEEK_V4_PRO.to_string(),
            display_name: "DeepSeek V4 Pro (Evolink)".to_string(),
            description: "DeepSeek V4 Pro reasoning model served through the Evolink gateway."
                .to_string(),
            provider: Provider::Evolink,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(163_840),
        },
        ModelPreset {
            id: "evolink/deepseek-v4-flash".to_string(),
            model: evolink_models::DEEPSEEK_V4_FLASH.to_string(),
            display_name: "DeepSeek V4 Flash (Evolink)".to_string(),
            description: "DeepSeek V4 Flash fast inference model served through the Evolink gateway."
                .to_string(),
            provider: Provider::Evolink,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "evolink/doubao-seed-2.0-pro".to_string(),
            model: evolink_models::DOUBAO_SEED_2_0_PRO.to_string(),
            display_name: "Doubao Seed 2.0 Pro (Evolink)".to_string(),
            description: "Doubao Seed 2.0 Pro served through the Evolink gateway.".to_string(),
            provider: Provider::Evolink,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(262_144),
        },
        ModelPreset {
            id: "evolink/gemini-3.1-pro-preview".to_string(),
            model: evolink_models::GEMINI_3_1_PRO.to_string(),
            display_name: "Gemini 3.1 Pro (Evolink)".to_string(),
            description:
                "Gemini 3.1 Pro served through the Evolink gateway via OpenAI SDK format."
                    .to_string(),
            provider: Provider::Evolink,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(
                ReasoningEffortLevel::Medium,
                "Balanced",
            )],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "evolink/gemini-3.5-flash".to_string(),
            model: evolink_models::GEMINI_3_5_FLASH.to_string(),
            display_name: "Gemini 3.5 Flash (Evolink)".to_string(),
            description:
                "Gemini 3.5 Flash served through the Evolink gateway via OpenAI SDK format."
                    .to_string(),
            provider: Provider::Evolink,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(
                ReasoningEffortLevel::Medium,
                "Balanced",
            )],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "evolink/MiniMax-M3".to_string(),
            model: evolink_models::MINIMAX_M3.to_string(),
            display_name: "MiniMax-M3 (Evolink)".to_string(),
            description:
                "MiniMax-M3 frontier multimodal model served through the Evolink gateway."
                    .to_string(),
            provider: Provider::Evolink,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(
                ReasoningEffortLevel::Medium,
                "Balanced",
            )],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "evolink/claude-sonnet-4-6".to_string(),
            model: evolink_models::CLAUDE_SONNET_4_6.to_string(),
            display_name: "Claude Sonnet 4.6 (Evolink)".to_string(),
            description: "Claude Sonnet 4.6 served through Evolink via Anthropic Messages API."
                .to_string(),
            provider: Provider::Evolink,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(
                ReasoningEffortLevel::Medium,
                "Balanced",
            )],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(200_000),
        },
        ModelPreset {
            id: "evolink/claude-opus-4-8".to_string(),
            model: evolink_models::CLAUDE_OPUS_4_8.to_string(),
            display_name: "Claude Opus 4.8 (Evolink)".to_string(),
            description: "Claude Opus 4.8 served through Evolink via Anthropic Messages API."
                .to_string(),
            provider: Provider::Evolink,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced"),
                reasoning_preset(ReasoningEffortLevel::High, "Deep"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(200_000),
        },
        ModelPreset {
            id: "evolink/claude-haiku-4-5-20251001".to_string(),
            model: evolink_models::CLAUDE_HAIKU_4_5.to_string(),
            display_name: "Claude Haiku 4.5 (Evolink)".to_string(),
            description: "Claude Haiku 4.5 fast model served through Evolink via Anthropic Messages API."
                .to_string(),
            provider: Provider::Evolink,
            default_reasoning_effort: ReasoningEffortLevel::Low,
            supported_reasoning_efforts: vec![
                reasoning_preset(ReasoningEffortLevel::Low, "Fast"),
                reasoning_preset(ReasoningEffortLevel::Medium, "Balanced"),
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(200_000),
        },
    ]
}

fn moonshot_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "kimi-k2.6".to_string(),
            model: "kimi-k2.6".to_string(),
            display_name: "Kimi K2.6 (Moonshot)".to_string(),
            description: "Moonshot's latest flagship coding and agent model.".to_string(),
            provider: Provider::Moonshot,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(
                ReasoningEffortLevel::Medium,
                "Balanced",
            )],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(256_000),
        },
        ModelPreset {
            id: "kimi-k2.5".to_string(),
            model: "kimi-k2.5".to_string(),
            display_name: "Kimi K2.5 (Moonshot)".to_string(),
            description: "Moonshot's previous flagship model for long-context coding.".to_string(),
            provider: Provider::Moonshot,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![reasoning_preset(
                ReasoningEffortLevel::Medium,
                "Balanced",
            )],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(256_000),
        },
    ]
}

fn huggingface_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "huggingface/deepseek-v4-flash".to_string(),
            model: "deepseek-ai/DeepSeek-V4-Flash:novita".to_string(),
            display_name: "DeepSeek V4 Flash (HF/Novita)".to_string(),
            description:
                "Fast inference model for cost-effective reasoning (1M context, 158B params)"
                    .to_string(),
            provider: Provider::HuggingFace,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Max,
                    description: "Maximum thinking".to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
        ModelPreset {
            id: "huggingface/deepseek-v4-pro".to_string(),
            model: "deepseek-ai/DeepSeek-V4-Pro:together".to_string(),
            display_name: "DeepSeek V4 Pro (HF/Together)".to_string(),
            description:
                "High-performance reasoning model with advanced thinking capabilities (1M context, 1.6T params)"
                    .to_string(),
            provider: Provider::HuggingFace,
            default_reasoning_effort: ReasoningEffortLevel::High,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Max,
                    description: "Maximum thinking".to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(1_000_000),
        },
    ]
}

/// Get all model presets (for testing)
#[cfg(test)]
pub fn all_model_presets() -> Vec<ModelPreset> {
    builtin_model_presets()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_default_per_provider() {
        let presets = builtin_model_presets();
        let providers: Vec<Provider> = Provider::all_providers();

        for provider in providers {
            let default_count = presets
                .iter()
                .filter(|p| p.provider == provider && p.is_default)
                .count();
            assert!(
                default_count <= 1,
                "Provider {:?} has {} defaults, expected 0 or 1",
                provider,
                default_count
            );
        }
    }

    #[test]
    fn gemini_presets_exist() {
        let presets = gemini_presets();
        assert!(!presets.is_empty());
        assert!(presets.iter().any(|p| p.id.contains("gemini")));
    }

    #[test]
    fn model_info_converts_to_preset() {
        let info = ModelInfo {
            slug: "test-model".to_string(),
            display_name: "Test Model".to_string(),
            description: "A test model".to_string(),
            provider: Provider::Gemini,
            default_reasoning_level: ReasoningEffortLevel::Medium,
            supported_reasoning_levels: vec![],
            context_window: Some(128_000),
            supports_tool_use: true,
            supports_streaming: true,
            supports_reasoning: false,
            priority: 0,
            visibility: "list".to_string(),
            supported_in_api: true,
            upgrade: None,
        };

        let preset: ModelPreset = info.into();
        assert_eq!(preset.id, "test-model");
        assert_eq!(preset.model, "test-model");
        assert!(preset.show_in_picker);
    }

    #[test]
    fn anthropic_opus_47_defaults_to_xhigh_and_offers_max() {
        let opus = anthropic_presets()
            .into_iter()
            .find(|preset| preset.id == "claude-opus-4-7")
            .expect("claude-opus-4-7 preset");

        assert_eq!(opus.default_reasoning_effort, ReasoningEffortLevel::XHigh);
        assert!(
            opus.supported_reasoning_efforts
                .iter()
                .any(|preset| preset.effort == ReasoningEffortLevel::Max)
        );
    }

    #[test]
    fn openai_codex_presets_default_to_high_reasoning() {
        let codex = openai_presets()
            .into_iter()
            .find(|preset| preset.id == "gpt-5.3-codex")
            .expect("gpt-5.3-codex preset");

        assert_eq!(codex.default_reasoning_effort, ReasoningEffortLevel::High);
    }

    #[test]
    fn moonshot_presets_exist_and_default_to_kimi_k26() {
        let presets = moonshot_presets();
        assert_eq!(presets.len(), 2);
        assert!(presets.iter().any(|preset| preset.id == "kimi-k2.5"));

        let default = presets
            .iter()
            .find(|preset| preset.is_default)
            .expect("moonshot default preset");
        assert_eq!(default.id, "kimi-k2.6");
        assert_eq!(default.provider, Provider::Moonshot);
    }
}
