//! Model presets and built-in model configurations.
//!
//! This module provides pre-configured model presets for all supported providers,
//! following the pattern from OpenAI Codex's models_manager.

use serde::{Deserialize, Serialize};

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

    // DeepSeek presets
    presets.extend(deepseek_presets());

    // Z.AI presets
    presets.extend(zai_presets());

    // LM Studio presets
    presets.extend(lmstudio_presets());

    // MiniMax presets
    presets.extend(minimax_presets());

    presets
}

/// Get presets for a specific provider
pub fn presets_for_provider(provider: Provider) -> Vec<ModelPreset> {
    match provider {
        Provider::Gemini => gemini_presets(),
        Provider::OpenAI => openai_presets(),
        Provider::Anthropic => anthropic_presets(),
        Provider::DeepSeek => deepseek_presets(),
        Provider::ZAI => zai_presets(),
        Provider::Minimax => minimax_presets(),
        Provider::OpenRouter => openrouter_presets(),
        Provider::Ollama => ollama_presets(),
        Provider::LmStudio => lmstudio_presets(),
        Provider::Moonshot => moonshot_presets(),
        Provider::HuggingFace => huggingface_presets(),
    }
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

fn openai_presets() -> Vec<ModelPreset> {
    vec![
        ModelPreset {
            id: "gpt-5.2".to_string(),
            model: "gpt-5.2".to_string(),
            display_name: "GPT-5.2".to_string(),
            description: "Latest frontier model with improved reasoning and coding".to_string(),
            provider: Provider::OpenAI,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Low,
                    description: "Fast responses".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Maximum reasoning".to_string(),
                },
            ],
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
            id: "gpt-5.3-codex".to_string(),
            model: "gpt-5.3-codex".to_string(),
            display_name: "GPT-5.3 Codex".to_string(),
            description: "GPT-5.3 variant optimized for agentic coding with xhigh reasoning"
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
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::XHigh,
                    description: "Maximum reasoning".to_string(),
                },
            ],
            is_default: true,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(400_000),
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
            id: "claude-opus-4.6".to_string(),
            model: "claude-opus-4.6".to_string(),
            display_name: "Claude Opus 4.6".to_string(),
            description: "Next-gen flagship with adaptive thinking".to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
                },
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::High,
                    description: "Deep reasoning".to_string(),
                },
            ],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(200_000),
        },
        ModelPreset {
            id: "claude-opus-4.5".to_string(),
            model: "claude-opus-4.5".to_string(),
            display_name: "Claude Opus 4.5".to_string(),
            description: "Latest flagship with exceptional reasoning".to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![
                ReasoningEffortPreset {
                    effort: ReasoningEffortLevel::Medium,
                    description: "Balanced".to_string(),
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
            context_window: Some(200_000),
        },
        ModelPreset {
            id: "claude-sonnet-4.5".to_string(),
            model: "claude-sonnet-4.5".to_string(),
            display_name: "Claude Sonnet 4.5".to_string(),
            description: "Balanced Anthropic model".to_string(),
            provider: Provider::Anthropic,
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
            id: "claude-haiku-4.5".to_string(),
            model: "claude-haiku-4.5".to_string(),
            display_name: "Claude Haiku 4.5".to_string(),
            description: "Efficient Anthropic model".to_string(),
            provider: Provider::Anthropic,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Fast".to_string(),
            }],
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
            id: "deepseek-reasoner".to_string(),
            model: "deepseek-reasoner".to_string(),
            display_name: "DeepSeek V3.2 Reasoner".to_string(),
            description: "Thinking mode with structured reasoning".to_string(),
            provider: Provider::DeepSeek,
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
            context_window: Some(128_000),
        },
        ModelPreset {
            id: "deepseek-chat".to_string(),
            model: "deepseek-chat".to_string(),
            display_name: "DeepSeek V3.2 Chat".to_string(),
            description: "Fast non-thinking mode".to_string(),
            provider: Provider::DeepSeek,
            default_reasoning_effort: ReasoningEffortLevel::Medium,
            supported_reasoning_efforts: vec![ReasoningEffortPreset {
                effort: ReasoningEffortLevel::Medium,
                description: "Fast".to_string(),
            }],
            is_default: false,
            upgrade: None,
            show_in_picker: true,
            supported_in_api: true,
            context_window: Some(128_000),
        },
    ]
}

fn zai_presets() -> Vec<ModelPreset> {
    vec![ModelPreset {
        id: "glm-5".to_string(),
        model: "glm-5".to_string(),
        display_name: "GLM-5".to_string(),
        description: "Z.ai's flagship open-source foundation model for complex systems".to_string(),
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
    }]
}

fn minimax_presets() -> Vec<ModelPreset> {
    vec![ModelPreset {
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
        is_default: true,
        upgrade: None,
        show_in_picker: true,
        supported_in_api: true,
        context_window: Some(128_000),
    }]
}

fn openrouter_presets() -> Vec<ModelPreset> {
    vec![ModelPreset {
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
        is_default: true,
        upgrade: None,
        show_in_picker: true,
        supported_in_api: true,
        context_window: Some(128_000),
    }]
}

fn ollama_presets() -> Vec<ModelPreset> {
    vec![ModelPreset {
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
    }]
}

fn lmstudio_presets() -> Vec<ModelPreset> {
    vec![ModelPreset {
        id: "lmstudio/local-model".to_string(),
        model: "local-model".to_string(),
        display_name: "Local Model (LM Studio)".to_string(),
        description: "LM Studio local inference server".to_string(),
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
        context_window: Some(32_000),
    }]
}

fn moonshot_presets() -> Vec<ModelPreset> {
    vec![]
}

fn huggingface_presets() -> Vec<ModelPreset> {
    vec![ModelPreset {
        id: "huggingface/deepseek-v3.2".to_string(),
        model: "deepseek-ai/DeepSeek-V3-0324".to_string(),
        display_name: "DeepSeek V3.2 (Hugging Face)".to_string(),
        description: "DeepSeek via Hugging Face Inference".to_string(),
        provider: Provider::HuggingFace,
        default_reasoning_effort: ReasoningEffortLevel::Medium,
        supported_reasoning_efforts: vec![ReasoningEffortPreset {
            effort: ReasoningEffortLevel::Medium,
            description: "Balanced".to_string(),
        }],
        is_default: true,
        upgrade: None,
        show_in_picker: true,
        supported_in_api: true,
        context_window: Some(128_000),
    }]
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
}
