use anyhow::{Context, Result};

use crate::config::api_keys::{ApiKeySources, get_api_key};
use crate::config::constants::model_helpers;
use crate::config::loader::VTCodeConfig;
use crate::config::models::{ModelId, Provider};
use crate::config::types::AgentConfig as RuntimeAgentConfig;
use crate::llm::factory::{ProviderConfig, create_provider_with_config, infer_provider_from_model};
use crate::llm::provider::LLMProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightweightFeature {
    Memory,
    PromptSuggestions,
    PromptRefinement,
    AutoModeReview,
    AutoModeProbe,
    LargeReadSummary,
    WebSummary,
    GitHistorySummary,
    Subagent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRoute {
    pub provider_name: String,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightweightRouteSource {
    FeatureOverride,
    SharedConfigured,
    SharedAutomatic,
    MainModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightweightRouteResolution {
    pub primary: ModelRoute,
    pub fallback: Option<ModelRoute>,
    pub source: LightweightRouteSource,
    pub warning: Option<String>,
}

impl LightweightRouteResolution {
    pub fn uses_lightweight_model(&self) -> bool {
        !matches!(self.source, LightweightRouteSource::MainModel)
    }

    pub fn fallback_to_main_model(&self) -> Option<&ModelRoute> {
        self.fallback.as_ref()
    }
}

pub fn resolve_lightweight_route(
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    feature: LightweightFeature,
    explicit_override_model: Option<&str>,
) -> LightweightRouteResolution {
    let main_route = main_model_route(runtime_config);
    let main_provider = main_route.provider_name.as_str();

    let mut warning = None;
    if let Some(configured_model) = explicit_override_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(route) = route_for_candidate(main_provider, configured_model) {
            return LightweightRouteResolution {
                fallback: (route != main_route).then_some(main_route),
                primary: route,
                source: LightweightRouteSource::FeatureOverride,
                warning: None,
            };
        }

        warning = Some(format!(
            "ignored lightweight override model '{}' because it does not match the active provider '{}'",
            configured_model, main_provider
        ));
    }

    let Some(vt_cfg) = vt_cfg else {
        return LightweightRouteResolution {
            primary: main_route,
            fallback: None,
            source: LightweightRouteSource::MainModel,
            warning,
        };
    };

    let shared_cfg = &vt_cfg.agent.small_model;
    if !shared_cfg.enabled || !feature_uses_shared_model(shared_cfg, feature) {
        return LightweightRouteResolution {
            primary: main_route,
            fallback: None,
            source: LightweightRouteSource::MainModel,
            warning,
        };
    }

    let configured_model = shared_cfg.model.trim();
    if !configured_model.is_empty() {
        if let Some(route) = route_for_candidate(main_provider, configured_model) {
            return LightweightRouteResolution {
                fallback: (route != main_route).then_some(main_route),
                primary: route,
                source: LightweightRouteSource::SharedConfigured,
                warning,
            };
        }

        warning = Some(format!(
            "ignored lightweight model '{}' because it does not match the active provider '{}'",
            configured_model, main_provider
        ));
    }

    let primary = ModelRoute {
        provider_name: main_route.provider_name.clone(),
        model: auto_lightweight_model(main_provider, &main_route.model),
    };
    LightweightRouteResolution {
        fallback: (primary != main_route).then_some(main_route),
        primary,
        source: LightweightRouteSource::SharedAutomatic,
        warning,
    }
}

pub fn main_model_route(runtime_config: &RuntimeAgentConfig) -> ModelRoute {
    let provider_name = if runtime_config.provider.trim().is_empty() {
        infer_provider_from_model(&runtime_config.model)
            .map(|provider| provider.to_string().to_lowercase())
            .unwrap_or_else(|| "gemini".to_string())
    } else {
        runtime_config.provider.to_lowercase()
    };

    ModelRoute {
        provider_name,
        model: runtime_config.model.clone(),
    }
}

pub fn auto_lightweight_model(provider_name: &str, active_model: &str) -> String {
    let trimmed_model = active_model.trim();
    let provider = resolve_provider_for_model(provider_name, trimmed_model);

    if let Ok(model_id) = trimmed_model.parse::<ModelId>() {
        if model_id.is_efficient_variant() {
            return model_id.as_str().to_string();
        }

        if let Some(lightweight_model) = preferred_lightweight_model_id(model_id) {
            return lightweight_model.as_str().to_string();
        }
    }

    if let Some(lightweight_model) = preferred_lightweight_model_slug(provider, trimmed_model) {
        return lightweight_model;
    }

    provider_default_lightweight_model(provider)
        .or_else(|| model_helpers::default_for(provider_name))
        .unwrap_or(trimmed_model)
        .to_string()
}

pub fn lightweight_model_choices(provider_name: &str, active_model: &str) -> Vec<String> {
    let provider = resolve_provider_for_model(provider_name, active_model);
    let auto_model = auto_lightweight_model(provider_name, active_model);
    let mut choices = Vec::new();

    if !auto_model.trim().is_empty() {
        choices.push(auto_model.clone());
    }
    if !active_model.trim().is_empty() {
        choices.push(active_model.trim().to_string());
    }

    if let Some(models) = model_helpers::supported_for(provider.as_ref()) {
        for model in models {
            let include = model
                .parse::<ModelId>()
                .map(|model_id| model_id.is_efficient_variant())
                .unwrap_or(false)
                || model.eq_ignore_ascii_case(active_model.trim());
            if include {
                choices.push((*model).to_string());
            }
        }
    }

    choices.sort();
    choices.dedup();
    if let Some(auto_index) = choices
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(auto_model.as_str()))
    {
        let auto = choices.remove(auto_index);
        choices.insert(0, auto);
    }
    choices
}

pub fn create_provider_for_model_route(
    route: &ModelRoute,
    runtime_config: &RuntimeAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<Box<dyn LLMProvider>> {
    let api_key = resolve_api_key_for_model_route(route, runtime_config);
    create_provider_with_config(
        &route.provider_name,
        ProviderConfig {
            api_key,
            openai_chatgpt_auth: runtime_config.openai_chatgpt_auth.clone(),
            copilot_auth: vt_cfg.map(|cfg| cfg.auth.copilot.clone()),
            base_url: None,
            model: Some(route.model.clone()),
            prompt_cache: Some(runtime_config.prompt_cache.clone()),
            timeouts: None,
            openai: vt_cfg.map(|cfg| cfg.provider.openai.clone()),
            anthropic: vt_cfg.map(|cfg| cfg.provider.anthropic.clone()),
            model_behavior: runtime_config.model_behavior.clone(),
            workspace_root: Some(runtime_config.workspace.clone()),
        },
    )
    .with_context(|| {
        format!(
            "Failed to initialize lightweight provider '{}' for model '{}'",
            route.provider_name, route.model
        )
    })
}

pub fn resolve_api_key_for_model_route(
    route: &ModelRoute,
    runtime_config: &RuntimeAgentConfig,
) -> Option<String> {
    if route
        .provider_name
        .eq_ignore_ascii_case(main_model_route(runtime_config).provider_name.as_str())
        && !runtime_config.api_key.trim().is_empty()
    {
        return Some(runtime_config.api_key.clone());
    }

    get_api_key(&route.provider_name, &ApiKeySources::default()).ok()
}

fn feature_uses_shared_model(
    shared_cfg: &vtcode_config::core::agent::AgentSmallModelConfig,
    feature: LightweightFeature,
) -> bool {
    match feature {
        LightweightFeature::Memory => shared_cfg.use_for_memory,
        LightweightFeature::LargeReadSummary => shared_cfg.use_for_large_reads,
        LightweightFeature::WebSummary => shared_cfg.use_for_web_summary,
        LightweightFeature::GitHistorySummary => shared_cfg.use_for_git_history,
        LightweightFeature::PromptSuggestions
        | LightweightFeature::PromptRefinement
        | LightweightFeature::AutoModeReview
        | LightweightFeature::AutoModeProbe
        | LightweightFeature::Subagent => true,
    }
}

fn route_for_candidate(main_provider: &str, candidate_model: &str) -> Option<ModelRoute> {
    if infer_provider_from_model(candidate_model)
        .map(|provider| !provider.as_ref().eq_ignore_ascii_case(main_provider))
        .unwrap_or(false)
    {
        return None;
    }

    Some(ModelRoute {
        provider_name: main_provider.to_string(),
        model: candidate_model.to_string(),
    })
}

fn provider_from_name(provider_name: &str) -> Provider {
    known_provider_from_name(provider_name).unwrap_or(Provider::Gemini)
}

fn resolve_provider_for_model(provider_name: &str, active_model: &str) -> Provider {
    known_provider_from_name(provider_name)
        .or_else(|| infer_provider_from_model(active_model))
        .unwrap_or_else(|| provider_from_name(provider_name))
}

fn known_provider_from_name(provider_name: &str) -> Option<Provider> {
    match provider_name.to_ascii_lowercase().as_str() {
        "openai" => Some(Provider::OpenAI),
        "anthropic" => Some(Provider::Anthropic),
        "copilot" => Some(Provider::Copilot),
        "deepseek" => Some(Provider::DeepSeek),
        "gemini" | "google" => Some(Provider::Gemini),
        "openrouter" => Some(Provider::OpenRouter),
        "ollama" => Some(Provider::Ollama),
        "lmstudio" => Some(Provider::LmStudio),
        "moonshot" => Some(Provider::Moonshot),
        "zai" => Some(Provider::ZAI),
        "minimax" => Some(Provider::Minimax),
        "huggingface" => Some(Provider::HuggingFace),
        _ => None,
    }
}

fn preferred_lightweight_model_id(active_model: ModelId) -> Option<ModelId> {
    match active_model {
        ModelId::Gemini31ProPreview | ModelId::Gemini31ProPreviewCustomTools => {
            Some(ModelId::Gemini31FlashLitePreview)
        }
        ModelId::GPT54 | ModelId::GPT54Pro => Some(ModelId::GPT54Mini),
        ModelId::GPT52
        | ModelId::GPT52Codex
        | ModelId::GPT53Codex
        | ModelId::GPT51Codex
        | ModelId::GPT51CodexMax
        | ModelId::GPT5
        | ModelId::GPT5Codex => Some(ModelId::GPT5Mini),
        ModelId::ClaudeOpus46 | ModelId::ClaudeSonnet46 => Some(ModelId::ClaudeHaiku45),
        ModelId::CopilotGPT54 => Some(ModelId::CopilotGPT54Mini),
        ModelId::CopilotGPT52Codex | ModelId::CopilotGPT51CodexMax => {
            Some(ModelId::CopilotGPT54Mini)
        }
        ModelId::DeepSeekReasoner => Some(ModelId::DeepSeekChat),
        ModelId::ZaiGlm51 => Some(ModelId::ZaiGlm5),
        ModelId::MinimaxM27 => Some(ModelId::MinimaxM25),
        _ => None,
    }
}

fn preferred_lightweight_model_slug(provider: Provider, active_model: &str) -> Option<String> {
    let trimmed_model = active_model.trim();
    let lower = trimmed_model.to_ascii_lowercase();

    match provider {
        Provider::Anthropic => {
            if lower.contains("haiku") {
                return Some(ModelId::ClaudeHaiku45.as_str().to_string());
            }
            if lower.contains("sonnet") || lower.contains("opus") {
                return Some(ModelId::ClaudeHaiku45.as_str().to_string());
            }
            None
        }
        Provider::OpenAI => {
            if lower.contains("gpt-5.4-mini") || lower.contains("gpt-5.4-nano") {
                return Some(trimmed_model.to_string());
            }
            if lower.contains("gpt-5.4") {
                return Some(ModelId::GPT54Mini.as_str().to_string());
            }
            if lower.contains("gpt-5-mini") || lower.contains("gpt-5-nano") {
                return Some(trimmed_model.to_string());
            }
            if lower.contains("gpt-5.") || lower == "gpt-5" || lower.contains("gpt-5-codex") {
                return Some(ModelId::GPT5Mini.as_str().to_string());
            }
            None
        }
        Provider::Copilot => {
            if lower.contains("gpt-5.4-mini") {
                return Some(trimmed_model.to_string());
            }
            if lower.contains("gpt-5") || lower.contains("claude") {
                return Some(ModelId::CopilotGPT54Mini.as_str().to_string());
            }
            None
        }
        Provider::DeepSeek => {
            if lower.contains("chat") {
                return Some(trimmed_model.to_string());
            }
            if lower.contains("reasoner") {
                return Some(ModelId::DeepSeekChat.as_str().to_string());
            }
            None
        }
        Provider::Gemini => {
            if lower.contains("flash-lite") || lower.contains("flash preview") {
                return Some(trimmed_model.to_string());
            }
            if lower.contains("3.1") {
                return Some(ModelId::Gemini31FlashLitePreview.as_str().to_string());
            }
            if lower.contains("gemini-3") || lower.contains("gemini 3") {
                return Some(ModelId::Gemini3FlashPreview.as_str().to_string());
            }
            None
        }
        Provider::ZAI => {
            if lower.contains("glm-5.1") {
                return Some(ModelId::ZaiGlm5.as_str().to_string());
            }
            if lower.contains("glm-5") {
                return Some(ModelId::ZaiGlm5.as_str().to_string());
            }
            None
        }
        Provider::Minimax => {
            if lower.contains("m2.5") {
                return Some(trimmed_model.to_string());
            }
            if lower.contains("m2.7") {
                return Some(ModelId::MinimaxM25.as_str().to_string());
            }
            None
        }
        _ => None,
    }
}

fn provider_default_lightweight_model(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::OpenAI => Some(ModelId::GPT5Mini.as_str()),
        Provider::Anthropic => Some(ModelId::ClaudeHaiku45.as_str()),
        Provider::Copilot => Some(ModelId::CopilotGPT54Mini.as_str()),
        Provider::DeepSeek => Some(ModelId::DeepSeekChat.as_str()),
        Provider::Gemini => Some(ModelId::Gemini3FlashPreview.as_str()),
        Provider::ZAI => Some(ModelId::ZaiGlm5.as_str()),
        Provider::Minimax => Some(ModelId::MinimaxM25.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime_config() -> RuntimeAgentConfig {
        RuntimeAgentConfig {
            model: ModelId::GPT54.as_str().to_string(),
            api_key: "test-key".to_string(),
            provider: "openai".to_string(),
            openai_chatgpt_auth: None,
            api_key_env: "OPENAI_API_KEY".to_string(),
            workspace: std::env::temp_dir().join("vtcode-lightweight-routing-tests"),
            verbose: false,
            quiet: false,
            theme: "default".to_string(),
            reasoning_effort: Default::default(),
            ui_surface: Default::default(),
            prompt_cache: Default::default(),
            model_source: Default::default(),
            custom_api_keys: Default::default(),
            checkpointing_enabled: false,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: 0,
            checkpointing_max_age_days: None,
            max_conversation_turns: 0,
            model_behavior: None,
        }
    }

    #[test]
    fn explicit_override_uses_active_provider() {
        let runtime = runtime_config();
        let route = resolve_lightweight_route(
            &runtime,
            Some(&VTCodeConfig::default()),
            LightweightFeature::Memory,
            Some("gpt-5-mini"),
        );

        assert_eq!(route.primary.provider_name, "openai");
        assert_eq!(route.primary.model, "gpt-5-mini");
        assert_eq!(route.source, LightweightRouteSource::FeatureOverride);
    }

    #[test]
    fn cross_provider_shared_model_falls_back_to_auto_same_provider() {
        let runtime = runtime_config();
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.small_model.model = "claude-4-5-haiku".to_string();

        let route = resolve_lightweight_route(
            &runtime,
            Some(&vt_cfg),
            LightweightFeature::PromptSuggestions,
            None,
        );

        assert_eq!(route.primary.provider_name, "openai");
        assert_eq!(route.primary.model, ModelId::GPT54Mini.as_str());
        assert_eq!(route.source, LightweightRouteSource::SharedAutomatic);
        assert!(route.warning.is_some());
    }

    #[test]
    fn auto_lightweight_model_prefers_same_generation_openai_sibling() {
        assert_eq!(
            auto_lightweight_model("openai", ModelId::GPT54.as_str()),
            ModelId::GPT54Mini.as_str()
        );
    }

    #[test]
    fn auto_lightweight_model_uses_closest_anthropic_haiku_pair() {
        assert_eq!(
            auto_lightweight_model("anthropic", ModelId::ClaudeSonnet46.as_str()),
            ModelId::ClaudeHaiku45.as_str()
        );
        assert_eq!(
            auto_lightweight_model("anthropic", "claude-sonnet-4.5"),
            ModelId::ClaudeHaiku45.as_str()
        );
    }

    #[test]
    fn auto_lightweight_model_uses_lower_generation_glm_pair() {
        assert_eq!(
            auto_lightweight_model("zai", ModelId::ZaiGlm51.as_str()),
            ModelId::ZaiGlm5.as_str()
        );
    }

    #[test]
    fn auto_lightweight_model_prefers_same_generation_gemini_flash_lite() {
        assert_eq!(
            auto_lightweight_model("gemini", ModelId::Gemini31ProPreview.as_str()),
            ModelId::Gemini31FlashLitePreview.as_str()
        );
    }

    #[test]
    fn auto_lightweight_model_infers_family_for_custom_provider() {
        assert_eq!(
            auto_lightweight_model("mycorp", ModelId::GPT54.as_str()),
            ModelId::GPT54Mini.as_str()
        );
    }

    #[test]
    fn disabled_feature_uses_main_model() {
        let runtime = runtime_config();
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.small_model.use_for_memory = false;

        let route =
            resolve_lightweight_route(&runtime, Some(&vt_cfg), LightweightFeature::Memory, None);

        assert_eq!(route.primary.model, ModelId::GPT54.as_str());
        assert_eq!(route.source, LightweightRouteSource::MainModel);
        assert!(route.fallback.is_none());
    }
}
