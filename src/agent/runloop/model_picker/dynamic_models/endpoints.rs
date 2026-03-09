use std::path::Path;

use vtcode_core::config::constants::{env_vars, urls};
use vtcode_core::config::models::Provider;
use vtcode_core::utils::dot_config::{DotConfig, load_user_config};

#[derive(Clone, Default)]
pub(super) struct ProviderEndpointConfig {
    ollama: Option<String>,
}

impl ProviderEndpointConfig {
    pub(super) async fn gather(_workspace: Option<&Path>) -> Self {
        let dot_config = load_user_config().await.ok();
        Self {
            ollama: Self::extract_base_url(Provider::Ollama, dot_config.as_ref()),
        }
    }

    pub(super) fn base_url(&self, provider: Provider) -> Option<String> {
        match provider {
            Provider::Ollama => self.ollama.clone(),
            _ => None,
        }
    }

    pub(super) fn resolved_base_url(&self, provider: Provider) -> String {
        self.base_url(provider)
            .unwrap_or_else(|| default_provider_base(provider).to_string())
    }

    fn extract_base_url(provider: Provider, dot_config: Option<&DotConfig>) -> Option<String> {
        let from_config = dot_config.and_then(|cfg| match provider {
            Provider::Ollama => cfg
                .providers
                .ollama
                .as_ref()
                .and_then(|c| c.base_url.clone()),
            _ => None,
        });

        from_config
            .and_then(Self::sanitize_owned)
            .or_else(|| Self::env_override(provider))
    }

    fn sanitize_owned(value: String) -> Option<String> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    fn env_override(provider: Provider) -> Option<String> {
        let key = match provider {
            Provider::Ollama => env_vars::OLLAMA_BASE_URL,
            _ => return None,
        };
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }
}

pub(super) fn default_provider_base(provider: Provider) -> &'static str {
    match provider {
        Provider::Ollama => urls::OLLAMA_API_BASE,
        _ => "",
    }
}
