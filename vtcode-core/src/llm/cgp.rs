//! Context-generic provider wiring for VT Code's LLM factory.
//!
//! This keeps the runtime string-keyed registry intact while moving provider
//! metadata and construction behind the same CGP substrate used by the tool
//! runtime. Zero-sized provider config types act as the context, and the
//! factory/builder layers consume blanket traits instead of hand-written
//! per-provider registration macros.

use std::marker::PhantomData;

use crate::components::{ComponentProvider, HasComponent};
use crate::config::TimeoutsConfig;
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::factory::{LLMFactory, ProviderConfig as FactoryProviderConfig};
use crate::llm::provider::LLMProvider;
use crate::llm::provider_config::{
    AnthropicProviderConfig, CopilotProviderConfig, DeepSeekProviderConfig, GeminiProviderConfig,
    HuggingFaceProviderConfig, LmStudioProviderConfig, MinimaxProviderConfig,
    MoonshotProviderConfig, OllamaProviderConfig, OpenAIProviderConfig,
    OpenResponsesProviderConfig, OpenRouterProviderConfig, ZAIProviderConfig,
};
use crate::llm::providers::{
    AnthropicProvider, CopilotProvider, DeepSeekProvider, GeminiProvider, HuggingFaceProvider,
    LmStudioProvider, MinimaxProvider, MoonshotProvider, OllamaProvider, OpenAIProvider,
    OpenResponsesProvider, OpenRouterProvider, ZAIProvider,
};

/// Marker component for static provider metadata.
pub enum ProviderMetadataComponent {}

/// Marker component for provider construction.
pub enum ProviderBuildComponent {}

/// Provider trait for provider metadata.
pub trait ProviderMetadataProvider<Ctx> {
    const PROVIDER_KEY: &'static str;
    const DISPLAY_NAME: &'static str;
    const DEFAULT_MODEL: &'static str;
    const API_BASE_URL: &'static str;
    const BASE_URL_ENV_VAR: Option<&'static str>;
}

/// Provider trait for constructing boxed providers from factory config.
pub trait ProviderBuildProvider<Ctx>: Send + Sync {
    fn build_provider(config: FactoryProviderConfig) -> Box<dyn LLMProvider>;
}

/// Ergonomic blanket consumer over the metadata component.
pub trait CanDescribeProvider {
    const PROVIDER_KEY: &'static str;
    const DISPLAY_NAME: &'static str;
    const DEFAULT_MODEL: &'static str;
    const API_BASE_URL: &'static str;
    const BASE_URL_ENV_VAR: Option<&'static str>;
}

impl<Ctx> CanDescribeProvider for Ctx
where
    Ctx: HasComponent<ProviderMetadataComponent>,
    ComponentProvider<Ctx, ProviderMetadataComponent>: ProviderMetadataProvider<Ctx>,
{
    const PROVIDER_KEY: &'static str =
        <ComponentProvider<Ctx, ProviderMetadataComponent> as ProviderMetadataProvider<
            Ctx,
        >>::PROVIDER_KEY;
    const DISPLAY_NAME: &'static str =
        <ComponentProvider<Ctx, ProviderMetadataComponent> as ProviderMetadataProvider<
            Ctx,
        >>::DISPLAY_NAME;
    const DEFAULT_MODEL: &'static str =
        <ComponentProvider<Ctx, ProviderMetadataComponent> as ProviderMetadataProvider<
            Ctx,
        >>::DEFAULT_MODEL;
    const API_BASE_URL: &'static str =
        <ComponentProvider<Ctx, ProviderMetadataComponent> as ProviderMetadataProvider<
            Ctx,
        >>::API_BASE_URL;
    const BASE_URL_ENV_VAR: Option<&'static str> = <ComponentProvider<
        Ctx,
        ProviderMetadataComponent,
    > as ProviderMetadataProvider<Ctx>>::BASE_URL_ENV_VAR;
}

/// Ergonomic blanket consumer over the provider build component.
pub trait CanBuildProvider {
    fn build_provider(config: FactoryProviderConfig) -> Box<dyn LLMProvider>;
}

impl<Ctx> CanBuildProvider for Ctx
where
    Ctx: HasComponent<ProviderBuildComponent>,
    ComponentProvider<Ctx, ProviderBuildComponent>: ProviderBuildProvider<Ctx>,
{
    fn build_provider(config: FactoryProviderConfig) -> Box<dyn LLMProvider> {
        <ComponentProvider<Ctx, ProviderBuildComponent> as ProviderBuildProvider<Ctx>>::build_provider(
            config,
        )
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

        Box::new(P::from_standard_config(
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

        Box::new(OpenAIProvider::from_config(
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

pub struct CopilotProviderBuild;

impl ProviderBuildProvider<CopilotProviderConfig> for CopilotProviderBuild {
    fn build_provider(config: FactoryProviderConfig) -> Box<dyn LLMProvider> {
        let FactoryProviderConfig {
            model,
            copilot_auth,
            workspace_root,
            ..
        } = config;

        Box::new(CopilotProvider::from_config(
            model,
            copilot_auth,
            workspace_root,
        ))
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
    MinimaxProvider,
    DeepSeekProvider,
    OpenRouterProvider,
    OpenResponsesProvider,
    MoonshotProvider,
    OllamaProvider,
    LmStudioProvider,
    ZAIProvider,
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
crate::delegate_components!(ZAIProviderConfig {
    ProviderMetadataComponent => ZAIProviderConfig,
    ProviderBuildComponent => StandardProviderBuild<ZAIProvider>,
});

/// Register all built-in provider contexts into the runtime factory.
pub fn register_builtin_cgp_providers(factory: &mut LLMFactory) {
    factory.register_cgp_provider::<GeminiProviderConfig>();
    factory.register_cgp_provider::<OpenAIProviderConfig>();
    factory.register_cgp_provider::<HuggingFaceProviderConfig>();
    factory.register_cgp_provider::<AnthropicProviderConfig>();
    factory.register_cgp_provider::<CopilotProviderConfig>();
    factory.register_cgp_provider::<MinimaxProviderConfig>();
    factory.register_cgp_provider::<DeepSeekProviderConfig>();
    factory.register_cgp_provider::<OpenRouterProviderConfig>();
    factory.register_cgp_provider::<OpenResponsesProviderConfig>();
    factory.register_cgp_provider::<MoonshotProviderConfig>();
    factory.register_cgp_provider::<OllamaProviderConfig>();
    factory.register_cgp_provider::<LmStudioProviderConfig>();
    factory.register_cgp_provider::<ZAIProviderConfig>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::core::{AnthropicConfig, OpenAIConfig};

    #[test]
    fn provider_context_metadata_is_available_through_consumer_traits() {
        assert_eq!(
            <GeminiProviderConfig as CanDescribeProvider>::PROVIDER_KEY,
            "gemini"
        );
        assert_eq!(
            <OpenAIProviderConfig as CanDescribeProvider>::DISPLAY_NAME,
            "OpenAI"
        );
        assert_eq!(
            <AnthropicProviderConfig as CanDescribeProvider>::BASE_URL_ENV_VAR,
            Some(crate::config::constants::env_vars::ANTHROPIC_BASE_URL)
        );
    }

    #[test]
    fn standard_build_consumer_builds_provider() {
        let provider =
            <GeminiProviderConfig as CanBuildProvider>::build_provider(FactoryProviderConfig {
                api_key: Some("test-key".to_string()),
                openai_chatgpt_auth: None,
                copilot_auth: None,
                base_url: None,
                model: Some(
                    crate::config::constants::models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
                ),
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
        let provider =
            <OpenAIProviderConfig as CanBuildProvider>::build_provider(FactoryProviderConfig {
                api_key: Some("test-key".to_string()),
                openai_chatgpt_auth: None,
                copilot_auth: None,
                base_url: None,
                model: Some(crate::config::constants::models::openai::DEFAULT_MODEL.to_string()),
                prompt_cache: None,
                timeouts: None,
                openai: Some(OpenAIConfig {
                    websocket_mode: true,
                    ..OpenAIConfig::default()
                }),
                anthropic: Some(AnthropicConfig::default()),
                model_behavior: None,
                workspace_root: None,
            });

        assert_eq!(provider.name(), "openai");
    }

    #[test]
    fn anthropic_build_consumer_accepts_provider_specific_config() {
        let provider =
            <AnthropicProviderConfig as CanBuildProvider>::build_provider(FactoryProviderConfig {
                api_key: Some("test-key".to_string()),
                openai_chatgpt_auth: None,
                copilot_auth: None,
                base_url: None,
                model: Some(crate::config::constants::models::anthropic::DEFAULT_MODEL.to_string()),
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
                "gemini",
                "huggingface",
                "lmstudio",
                "minimax",
                "moonshot",
                "ollama",
                "openai",
                "openresponses",
                "openrouter",
                "zai",
            ]
        );
    }
}
