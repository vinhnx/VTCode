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
    presets.extend(presets::gemini_presets());

    // OpenAI presets
    presets.extend(presets::openai_presets());

    // Anthropic presets
    presets.extend(presets::anthropic_presets());

    // Copilot presets
    presets.extend(presets::copilot_presets());

    // DeepSeek presets
    presets.extend(presets::deepseek_presets());

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

    // Evolink presets
    presets.extend(presets::evolink_presets());

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
    fn openai_codex_presets_default_to_high_reasoning() {
        let codex = presets::openai_presets()
            .into_iter()
            .find(|preset| preset.id == "gpt-5.3-codex")
            .expect("gpt-5.3-codex preset");

        assert_eq!(codex.default_reasoning_effort, ReasoningEffortLevel::High);
    }

    #[test]
    fn moonshot_presets_exist_and_default_to_kimi_k3() {
        let presets = presets::moonshot_presets();
        assert_eq!(presets.len(), 3);

        let default = presets
            .iter()
            .find(|preset| preset.is_default)
            .expect("moonshot default preset");
        assert_eq!(default.id, "kimi-k3");
        assert_eq!(default.provider, Provider::Moonshot);
    }
}
