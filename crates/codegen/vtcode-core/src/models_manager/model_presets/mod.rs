//! Model presets and built-in model configurations.
//!
//! This module provides pre-configured model presets for all supported providers,
//! following the pattern from OpenAI Codex's models_manager.
//!
//! Per-provider preset definitions live in the `presets` subdirectory; this
//! module owns the shared types and the two public aggregators
//! ([`builtin_model_presets`] and [`presets_for_provider`]).

use serde::{Deserialize, Serialize};

use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;

mod presets;

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
    /// Whether this model accepts image inputs
    #[serde(default)]
    pub supports_vision: bool,
    /// Whether this model supports schema-constrained structured output
    #[serde(default)]
    pub supports_structured_output: bool,
    /// Whether this model supports reasoning/thinking
    #[serde(default)]
    pub supports_reasoning: bool,
    /// Maximum generated output tokens, when published by the provider
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i64>,
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
    presets.extend(presets::gemini_presets());

    // OpenAI presets
    presets.extend(presets::openai_presets());

    // Anthropic presets
    presets.extend(presets::anthropic_presets());

    // Copilot presets
    presets.extend(presets::copilot_presets());

    // DeepSeek presets
    presets.extend(presets::deepseek_presets());

    // Official Meta AI presets
    presets.extend(presets::meta_presets());

    // Z.AI presets
    presets.extend(presets::zai_presets());

    // LM Studio presets
    presets.extend(presets::lmstudio_presets());

    // llama.cpp presets
    presets.extend(presets::llamacpp_presets());

    // MiniMax presets
    presets.extend(presets::minimax_presets());

    // OpenCode Zen presets
    presets.extend(presets::opencode_zen_presets());

    // OpenCode Go presets
    presets.extend(presets::opencode_go_presets());

    // Poolside presets
    presets.extend(presets::poolside_presets());

    // StepFun presets
    presets.extend(presets::stepfun_presets());

    // xAI presets
    presets.extend(presets::xai_presets());

    // NVIDIA NIM presets
    presets.extend(presets::nvidia_presets());

    // Merge Gateway presets
    presets.extend(presets::merge_gateway_presets());

    // Evolink presets
    presets.extend(presets::evolink_presets());

    // Vercel AI Gateway presets
    presets.extend(presets::vercel_presets());

    presets
}

/// Get presets for a specific provider
pub fn presets_for_provider(provider: Provider) -> Vec<ModelPreset> {
    match provider {
        Provider::Gemini => presets::gemini_presets(),
        Provider::OpenAI => presets::openai_presets(),
        Provider::Anthropic => presets::anthropic_presets(),
        Provider::Copilot => presets::copilot_presets(),
        Provider::DeepSeek => presets::deepseek_presets(),
        Provider::Meta => presets::meta_presets(),
        Provider::ZAI => presets::zai_presets(),
        Provider::Minimax => presets::minimax_presets(),
        Provider::OpenRouter => presets::openrouter_presets(),
        Provider::Ollama => presets::ollama_presets(),
        Provider::OllamaCloud => presets::ollama_presets(),
        Provider::LmStudio => presets::lmstudio_presets(),
        Provider::LlamaCpp => presets::llamacpp_presets(),
        Provider::Moonshot => presets::moonshot_presets(),
        Provider::Mistral => presets::mistral_presets(),
        Provider::HuggingFace => presets::huggingface_presets(),
        Provider::OpenCodeZen => presets::opencode_zen_presets(),
        Provider::OpenCodeGo => presets::opencode_go_presets(),
        Provider::MiMo => presets::mimo_presets(),
        Provider::Qwen => presets::qwen_presets(),
        Provider::StepFun => presets::stepfun_presets(),
        Provider::Evolink => presets::evolink_presets(),
        Provider::Poolside => presets::poolside_presets(),
        Provider::XAI => presets::xai_presets(),
        Provider::NVIDIA => presets::nvidia_presets(),
        Provider::MergeGateway => presets::merge_gateway_presets(),
        Provider::Vercel => presets::vercel_presets(),
    }
}

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
            let default_count = presets.iter().filter(|p| p.provider == provider && p.is_default).count();
            assert!(default_count <= 1, "Provider {provider:?} has {default_count} defaults, expected 0 or 1");
        }
    }

    #[test]
    fn gemini_presets_exist() {
        let presets = presets::gemini_presets();
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
            supports_vision: false,
            supports_structured_output: false,
            supports_reasoning: false,
            max_output_tokens: None,
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
    fn openai_codex_presets_default_to_high_reasoning() {
        let codex = presets::openai_presets()
            .into_iter()
            .find(|preset| preset.id == "gpt-5-codex")
            .expect("gpt-5.3-codex preset");

        assert_eq!(codex.default_reasoning_effort, ReasoningEffortLevel::High);
    }

    #[test]
    fn mistral_presets_include_large_4_without_changing_default() {
        let presets = presets::mistral_presets();
        let large_4 = presets
            .iter()
            .find(|preset| preset.model == "mistral-large-4")
            .expect("mistral large 4 preset");
        assert_eq!(large_4.provider, Provider::Mistral);
        assert!(!large_4.is_default);
        assert!(large_4.show_in_picker);
        assert_eq!(large_4.context_window, Some(1_000_000));

        let default = presets.iter().find(|preset| preset.is_default).expect("mistral default preset");
        assert_eq!(default.model, "mistral-large-2512");
    }

    #[test]
    fn moonshot_presets_exist_and_default_to_kimi_k3() {
        let presets = presets::moonshot_presets();
        assert_eq!(presets.len(), 1);

        let default = presets
            .iter()
            .find(|preset| preset.is_default)
            .expect("moonshot default preset");
        assert_eq!(default.id, "kimi-k3");
        assert_eq!(default.provider, Provider::Moonshot);
    }

    #[test]
    fn xai_presets_exist_and_default_to_grok46() {
        let presets = presets::xai_presets();
        assert!(presets.iter().any(|preset| preset.model == "grok-4.6"));
        assert!(presets.iter().any(|preset| preset.model == "grok-4.7"));

        let default = presets.iter().find(|preset| preset.is_default).expect("xAI default preset");
        assert_eq!(default.model, "grok-4.7");
        assert_eq!(default.context_window, Some(500_000));
        assert!(
            default
                .supported_reasoning_efforts
                .iter()
                .any(|effort| effort.effort == ReasoningEffortLevel::XHigh)
        );
        assert!(
            presets
                .iter()
                .all(|preset| preset.provider == Provider::XAI && preset.show_in_picker)
        );
    }

    #[test]
    fn nvidia_presets_expose_curated_models_and_default() {
        let presets = presets::nvidia_presets();
        assert_eq!(presets.len(), 3);
        let default = presets.iter().find(|preset| preset.is_default).expect("NVIDIA default preset");
        assert_eq!(default.model, "nvidia/nemotron-3-ultra-550b-a55b");
        assert_eq!(default.context_window, Some(1_000_000));
        assert!(
            presets
                .iter()
                .all(|preset| preset.provider == Provider::NVIDIA && preset.show_in_picker)
        );
    }

    #[test]
    fn merge_gateway_presets_expose_curated_routes_and_default() {
        let presets = presets::merge_gateway_presets();
        assert_eq!(presets.len(), 26);
        let default = presets
            .iter()
            .find(|preset| preset.is_default)
            .expect("Merge Gateway default preset");
        assert_eq!(default.model, "default_routing");
        assert_eq!(default.context_window, Some(128_000));
        assert!(
            presets
                .iter()
                .all(|preset| preset.provider == Provider::MergeGateway && preset.show_in_picker)
        );

        use crate::config::constants::models;
        let reasoning_models: Vec<&str> = presets
            .iter()
            .filter(|preset| !preset.supported_reasoning_efforts.is_empty())
            .map(|preset| preset.model.as_str())
            .collect();
        assert_eq!(reasoning_models, models::merge_gateway::REASONING_MODELS.to_vec());
        assert!(
            presets
                .iter()
                .filter(|preset| !preset.supported_reasoning_efforts.is_empty())
                .all(|preset| preset.default_reasoning_effort == ReasoningEffortLevel::Medium)
        );
    }

    #[test]
    fn meta_presets_keep_standard_default_and_contributor_opt_in() {
        let presets = presets::meta_presets();
        assert_eq!(presets.len(), 3);

        let default = presets.iter().find(|preset| preset.is_default).expect("Meta default preset");
        assert_eq!(default.model, "muse-spark-1.3");
        assert!(presets.iter().any(|preset| preset.model == "muse-spark-1.3-contributor"));
        assert!(
            presets
                .iter()
                .all(|preset| preset.provider == Provider::Meta && preset.show_in_picker)
        );
    }

    #[test]
    fn openrouter_presets_include_meta_muse_models() {
        let presets = presets::openrouter_presets();

        let glimmer = presets
            .iter()
            .find(|preset| preset.model == "meta/muse-glimmer-30b")
            .expect("OpenRouter Glimmer preset");
        assert_eq!(glimmer.context_window, Some(131_072));
    }
}
