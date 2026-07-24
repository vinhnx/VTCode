//! Context-generic provider wiring for VT Code's LLM factory.
//!
//! This keeps the runtime string-keyed registry intact while moving provider
//! metadata and construction behind the same CGP substrate used by the tool
//! runtime. Zero-sized provider config types act as the context, and the
//! factory/builder layers consume blanket traits instead of hand-written
//! per-provider registration macros.

use std::marker::PhantomData;

use super::factory::LLMFactory;
use super::factory::ProviderConfig as FactoryProviderConfig;
use super::provider::LLMProvider;
use super::provider_config::{
    AnthropicProviderConfig, CopilotProviderConfig, DeepSeekProviderConfig, EvolinkProviderConfig,
    GeminiProviderConfig, HuggingFaceProviderConfig, LlamaCppProviderConfig, LmStudioProviderConfig,
    MiMoProviderConfig, MinimaxProviderConfig, MistralProviderConfig, MoonshotProviderConfig, OllamaProviderConfig,
    OpenAIProviderConfig, OpenCodeGoProviderConfig, OpenCodeZenProviderConfig, OpenResponsesProviderConfig,
    OpenRouterProviderConfig, PoolsideProviderConfig, QwenProviderConfig, StepFunProviderConfig, XAIProviderConfig,
    ZAIProviderConfig,
};
use super::providers::{
    AnthropicProvider, CopilotProvider, DeepSeekProvider, EvolinkProvider, GeminiProvider, HuggingFaceProvider,
    LlamaCppProvider, LmStudioProvider, MiMoProvider, MinimaxProvider, MistralProvider, MoonshotProvider,
    OllamaProvider, OpenCodeGoProvider, OpenCodeZenProvider, OpenResponsesProvider, OpenRouterProvider,
    PoolsideProvider, QwenProvider, StepFunProvider, XAIProvider, ZAIProvider,
};
use vtcode_commons::cgp::{ComponentProvider, HasComponent};
use vtcode_config::TimeoutsConfig;
use vtcode_config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};

/// Marker component for static provider metadata.
pub enum ProviderMetadataComponent {}

/// Marker component for provider construction.
pub enum ProviderBuildComponent {}

/// Provider trait for provider metadata.
pub trait ProviderMetadataProvider<Ctx> {
    /// Registry key used to identify this provider in the factory.
    const PROVIDER_KEY: &'static str;
    /// Human-readable display name.
    const DISPLAY_NAME: &'static str;
    /// Default model identifier when none is specified.
    const DEFAULT_MODEL: &'static str;
    /// Base URL for the provider's API endpoint.
    const API_BASE_URL: &'static str;
    /// Optional environment variable that overrides the base URL.
    const BASE_URL_ENV_VAR: Option<&'static str>;
}

/// Provider trait for constructing boxed providers from factory config.
pub trait ProviderBuildProvider<Ctx>: Send + Sync {
    /// Construct a boxed [`LLMProvider`] from the given factory configuration.
    fn build_provider(config: FactoryProviderConfig) -> Box<dyn LLMProvider>;
}

/// Ergonomic blanket consumer over the metadata component.
pub trait CanDescribeProvider {
    /// Registry key used to identify this provider in the factory.
    const PROVIDER_KEY: &'static str;
    /// Human-readable display name.
    const DISPLAY_NAME: &'static str;
    /// Default model identifier when none is specified.
    const DEFAULT_MODEL: &'static str;
    /// Base URL for the provider's API endpoint.
    const API_BASE_URL: &'static str;
    /// Optional environment variable that overrides the base URL.
    const BASE_URL_ENV_VAR: Option<&'static str>;
}

impl<Ctx> CanDescribeProvider for Ctx
where
    Ctx: HasComponent<ProviderMetadataComponent>,
    ComponentProvider<Ctx, ProviderMetadataComponent>: ProviderMetadataProvider<Ctx>,
{
    const PROVIDER_KEY: &'static str =
        <ComponentProvider<Ctx, ProviderMetadataComponent> as ProviderMetadataProvider<Ctx>>::PROVIDER_KEY;
    const DISPLAY_NAME: &'static str =
        <ComponentProvider<Ctx, ProviderMetadataComponent> as ProviderMetadataProvider<Ctx>>::DISPLAY_NAME;
    const DEFAULT_MODEL: &'static str =
        <ComponentProvider<Ctx, ProviderMetadataComponent> as ProviderMetadataProvider<Ctx>>::DEFAULT_MODEL;
    const API_BASE_URL: &'static str =
        <ComponentProvider<Ctx, ProviderMetadataComponent> as ProviderMetadataProvider<Ctx>>::API_BASE_URL;
    const BASE_URL_ENV_VAR: Option<&'static str> =
        <ComponentProvider<Ctx, ProviderMetadataComponent> as ProviderMetadataProvider<Ctx>>::BASE_URL_ENV_VAR;
}

/// Ergonomic blanket consumer over the provider build component.
pub trait CanBuildProvider {
    /// Construct a boxed [`LLMProvider`] from the given factory configuration.
    fn build_provider(config: FactoryProviderConfig) -> Box<dyn LLMProvider>;
}

impl<Ctx> CanBuildProvider for Ctx
where
    Ctx: HasComponent<ProviderBuildComponent>,
    ComponentProvider<Ctx, ProviderBuildComponent>: ProviderBuildProvider<Ctx>,
{
    fn build_provider(config: FactoryProviderConfig) -> Box<dyn LLMProvider> {
        <ComponentProvider<Ctx, ProviderBuildComponent> as ProviderBuildProvider<Ctx>>::build_provider(config)
    }
}

trait StandardProviderConstructor: LLMProvider + Send + Sync + 'static {
    fn from_standard_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self;
}

/// Blanket build implementation for providers that follow the standard
/// `from_config` constructor pattern.
pub struct StandardProviderBuild<P>(PhantomData<P>);

impl<Ctx, P> ProviderBuildProvider<Ctx> for StandardProviderBuild<P>
where
    P: StandardProviderConstructor,
{
    fn build_provider(config: FactoryProviderConfig) -> Box<dyn LLMProvider> {
        let FactoryProviderConfig {
            api_key,
            openai_chatgpt_auth: _,
            base_url,
            model,
            prompt_cache,
            timeouts,
            openai: _,
            anthropic,
            model_behavior,
            ..
        } = config;

        Box::new(P::from_standard_config(api_key, model, base_url, prompt_cache, timeouts, anthropic, model_behavior))
    }
}

/// Build implementation for the Anthropic provider with custom config handling.
pub struct AnthropicProviderBuild;

impl ProviderBuildProvider<AnthropicProviderConfig> for AnthropicProviderBuild {
    fn build_provider(config: FactoryProviderConfig) -> Box<dyn LLMProvider> {
        let FactoryProviderConfig {
            api_key,
            openai_chatgpt_auth: _,
            base_url,
            model,
            prompt_cache,
            timeouts,
            openai: _,
            anthropic,
            model_behavior,
            ..
        } = config;

        Box::new(AnthropicProvider::from_config(
            api_key,
            model,
            base_url,
            prompt_cache,
            timeouts,
            anthropic,
            model_behavior,
        ))
    }
}

/// Build implementation for the OpenAI provider with provider-specific config handling.
pub struct OpenAIProviderBuild;

impl ProviderBuildProvider<OpenAIProviderConfig> for OpenAIProviderBuild {
    fn build_provider(config: FactoryProviderConfig) -> Box<dyn LLMProvider> {
        let FactoryProviderConfig {
            api_key,
            openai_chatgpt_auth,
            base_url,
            model,
            prompt_cache,
            timeouts,
            openai,
            anthropic,
            model_behavior,
            ..
        } = config;

        Box::new(vtcode_llm::providers::OpenAIProvider::from_config(
            api_key,
            openai_chatgpt_auth,
            model,
            base_url,
            prompt_cache,
            timeouts,
            anthropic,
            openai,
            model_behavior,
        ))
    }
}

/// Build implementation for the GitHub Copilot provider.
pub struct CopilotProviderBuild;

impl ProviderBuildProvider<CopilotProviderConfig> for CopilotProviderBuild {
    fn build_provider(config: FactoryProviderConfig) -> Box<dyn LLMProvider> {
        let FactoryProviderConfig { model, copilot_auth, workspace_root, .. } = config;

        Box::new(CopilotProvider::from_config(model, copilot_auth, workspace_root))
    }
}

macro_rules! impl_standard_provider_constructor {
    ($($provider:ty),+ $(,)?) => {
        $(
            impl StandardProviderConstructor for $provider {
                fn from_standard_config(
                    api_key: Option<String>,
                    model: Option<String>,
                    base_url: Option<String>,
                    prompt_cache: Option<PromptCachingConfig>,
                    timeouts: Option<TimeoutsConfig>,
                    anthropic: Option<AnthropicConfig>,
                    model_behavior: Option<ModelConfig>,
                ) -> Self {
                    <$provider>::from_config(
                        api_key,
                        model,
                        base_url,
                        prompt_cache,
                        timeouts,
                        anthropic,
                        model_behavior,
                    )
                }
            }
        )+
    };
}

impl_standard_provider_constructor!(
    GeminiProvider,
    HuggingFaceProvider,
    MiMoProvider,
    MinimaxProvider,
    DeepSeekProvider,
    MistralProvider,
    OpenRouterProvider,
    OpenResponsesProvider,
    MoonshotProvider,
    OllamaProvider,
    LlamaCppProvider,
    LmStudioProvider,
    ZAIProvider,
    OpenCodeZenProvider,
    OpenCodeGoProvider,
    QwenProvider,
    StepFunProvider,
    EvolinkProvider,
    PoolsideProvider,
    XAIProvider,
);

crate::delegate_components!(GeminiProviderConfig {
    ProviderMetadataComponent => GeminiProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<GeminiProvider>,
});
crate::delegate_components!(AnthropicProviderConfig {
    ProviderMetadataComponent => AnthropicProviderConfig,
    ProviderBuildComponent => AnthropicProviderBuild,
});
crate::delegate_components!(CopilotProviderConfig {
    ProviderMetadataComponent => CopilotProviderConfig,
    ProviderBuildComponent => CopilotProviderBuild,
});
crate::delegate_components!(OpenAIProviderConfig {
    ProviderMetadataComponent => OpenAIProviderConfig,
    ProviderBuildComponent => OpenAIProviderBuild,
});
crate::delegate_components!(HuggingFaceProviderConfig {
    ProviderMetadataComponent => HuggingFaceProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<HuggingFaceProvider>,
});
crate::delegate_components!(DeepSeekProviderConfig {
    ProviderMetadataComponent => DeepSeekProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<DeepSeekProvider>,
});
crate::delegate_components!(MiMoProviderConfig {
    ProviderMetadataComponent => MiMoProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<MiMoProvider>,
});
crate::delegate_components!(MinimaxProviderConfig {
    ProviderMetadataComponent => MinimaxProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<MinimaxProvider>,
});
crate::delegate_components!(OpenRouterProviderConfig {
    ProviderMetadataComponent => OpenRouterProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<OpenRouterProvider>,
});
crate::delegate_components!(OpenResponsesProviderConfig {
    ProviderMetadataComponent => OpenResponsesProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<OpenResponsesProvider>,
});
crate::delegate_components!(MoonshotProviderConfig {
    ProviderMetadataComponent => MoonshotProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<MoonshotProvider>,
});
crate::delegate_components!(OllamaProviderConfig {
    ProviderMetadataComponent => OllamaProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<OllamaProvider>,
});
crate::delegate_components!(LmStudioProviderConfig {
    ProviderMetadataComponent => LmStudioProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<LmStudioProvider>,
});
crate::delegate_components!(LlamaCppProviderConfig {
    ProviderMetadataComponent => LlamaCppProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<LlamaCppProvider>,
});
crate::delegate_components!(ZAIProviderConfig {
    ProviderMetadataComponent => ZAIProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<ZAIProvider>,
});
crate::delegate_components!(MistralProviderConfig {
    ProviderMetadataComponent => MistralProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<MistralProvider>,
});
crate::delegate_components!(OpenCodeZenProviderConfig {
    ProviderMetadataComponent => OpenCodeZenProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<OpenCodeZenProvider>,
});
crate::delegate_components!(OpenCodeGoProviderConfig {
    ProviderMetadataComponent => OpenCodeGoProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<OpenCodeGoProvider>,
});
crate::delegate_components!(QwenProviderConfig {
    ProviderMetadataComponent => QwenProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<QwenProvider>,
});
crate::delegate_components!(StepFunProviderConfig {
    ProviderMetadataComponent => StepFunProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<StepFunProvider>,
});
crate::delegate_components!(EvolinkProviderConfig {
    ProviderMetadataComponent => EvolinkProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<EvolinkProvider>,
});
crate::delegate_components!(PoolsideProviderConfig {
    ProviderMetadataComponent => PoolsideProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<PoolsideProvider>,
});
crate::delegate_components!(XAIProviderConfig {
    ProviderMetadataComponent => XAIProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<XAIProvider>,
});

/// Register all built-in provider contexts into the runtime factory.
pub fn register_builtin_cgp_providers(factory: &mut LLMFactory) {
    factory.register_cgp_provider::<GeminiProviderConfig>();
    factory.register_cgp_provider::<OpenAIProviderConfig>();
    factory.register_cgp_provider::<HuggingFaceProviderConfig>();
    factory.register_cgp_provider::<AnthropicProviderConfig>();
    factory.register_cgp_provider::<CopilotProviderConfig>();
    factory.register_cgp_provider::<MinimaxProviderConfig>();
    factory.register_cgp_provider::<MiMoProviderConfig>();
    factory.register_cgp_provider::<DeepSeekProviderConfig>();
    factory.register_cgp_provider::<OpenRouterProviderConfig>();
    factory.register_cgp_provider::<OpenResponsesProviderConfig>();
    factory.register_cgp_provider::<MoonshotProviderConfig>();
    factory.register_cgp_provider::<OllamaProviderConfig>();
    factory.register_cgp_provider::<LmStudioProviderConfig>();
    factory.register_cgp_provider::<LlamaCppProviderConfig>();
    factory.register_cgp_provider::<ZAIProviderConfig>();
    factory.register_cgp_provider::<MistralProviderConfig>();
    factory.register_cgp_provider::<OpenCodeZenProviderConfig>();
    factory.register_cgp_provider::<OpenCodeGoProviderConfig>();
    factory.register_cgp_provider::<QwenProviderConfig>();
    factory.register_cgp_provider::<StepFunProviderConfig>();
    factory.register_cgp_provider::<EvolinkProviderConfig>();
    factory.register_cgp_provider::<PoolsideProviderConfig>();
    factory.register_cgp_provider::<XAIProviderConfig>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_config::core::{AnthropicConfig, OpenAIConfig};

    #[test]
    fn provider_context_metadata_is_available_through_consumer_traits() {
        assert_eq!(<GeminiProviderConfig as CanDescribeProvider>::PROVIDER_KEY, "gemini");
        assert_eq!(<OpenAIProviderConfig as CanDescribeProvider>::DISPLAY_NAME, "OpenAI");
        assert_eq!(
            <AnthropicProviderConfig as CanDescribeProvider>::BASE_URL_ENV_VAR,
            Some(vtcode_config::constants::env_vars::ANTHROPIC_BASE_URL)
        );
    }

    #[test]
    fn standard_build_consumer_builds_provider() {
        let provider = <GeminiProviderConfig as CanBuildProvider>::build_provider(FactoryProviderConfig {
            api_key: Some("test-key".to_string()),
            openai_chatgpt_auth: None,
            copilot_auth: None,
            base_url: None,
            model: Some(vtcode_config::constants::models::google::GEMINI_3_FLASH_PREVIEW.to_string()),
            prompt_cache: None,
            timeouts: None,
            openai: None,
            anthropic: None,
            model_behavior: None,
            workspace_root: None,
        });

        assert_eq!(provider.name(), "gemini");
    }

    #[test]
    fn openai_build_consumer_accepts_provider_specific_config() {
        let provider = <OpenAIProviderConfig as CanBuildProvider>::build_provider(FactoryProviderConfig {
            api_key: Some("test-key".to_string()),
            openai_chatgpt_auth: None,
            copilot_auth: None,
            base_url: None,
            model: Some(vtcode_config::constants::models::openai::DEFAULT_MODEL.to_string()),
            prompt_cache: None,
            timeouts: None,
            openai: Some(OpenAIConfig { websocket_mode: true, ..OpenAIConfig::default() }),
            anthropic: Some(AnthropicConfig::default()),
            model_behavior: None,
            workspace_root: None,
        });

        assert_eq!(provider.name(), "openai");
    }

    #[test]
    fn anthropic_build_consumer_accepts_provider_specific_config() {
        let provider = <AnthropicProviderConfig as CanBuildProvider>::build_provider(FactoryProviderConfig {
            api_key: Some("test-key".to_string()),
            openai_chatgpt_auth: None,
            copilot_auth: None,
            base_url: None,
            model: Some(vtcode_config::constants::models::anthropic::DEFAULT_MODEL.to_string()),
            prompt_cache: None,
            timeouts: None,
            openai: None,
            anthropic: Some(AnthropicConfig {
                count_tokens_enabled: true,
                ..AnthropicConfig::default()
            }),
            model_behavior: None,
            workspace_root: None,
        });

        assert_eq!(provider.name(), "anthropic");
    }

    #[test]
    fn builtin_registration_helper_registers_all_contexts() {
        let mut factory = LLMFactory::new();
        register_builtin_cgp_providers(&mut factory);

        let mut providers = factory.list_providers();
        providers.sort();

        assert_eq!(
            providers,
            vec![
                "anthropic",
                "copilot",
                "deepseek",
                "evolink",
                "gemini",
                "huggingface",
                "llamacpp",
                "lmstudio",
                "mimo",
                "minimax",
                "mistral",
                "moonshot",
                "ollama",
                "openai",
                "opencode-go",
                "opencode-zen",
                "openresponses",
                "openrouter",
                "poolside",
                "qwen",
                "stepfun",
                "xai",
                "zai",
            ]
        );
    }
}
