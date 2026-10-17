//! Native Claude speed selection and route eligibility.

use serde::{Deserialize, Serialize};

use crate::VTCodeConfig;
use crate::constants::{env_vars, models::anthropic::normalize_model_id, urls};

/// Native Anthropic Messages API speed. Unset configuration uses standard speed.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AnthropicSpeed {
    Standard,
    Fast,
}

impl AnthropicSpeed {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Fast => "fast",
        }
    }

    /// Exact native IDs, including the existing dated snapshot normalization.
    pub fn supports_model(model: &str) -> bool {
        matches!(normalize_model_id(model), "claude-opus-5-5" | "claude-opus-5" | "claude-opus-4-8")
    }

    /// Preview models require the beta for either serialized speed value.
    pub fn requires_beta(model: &str) -> bool {
        matches!(normalize_model_id(model), "claude-opus-5" | "claude-opus-4-8")
    }

    /// Only the standard native Messages endpoint supports this integration.
    pub fn is_native_endpoint(base_url: &str) -> bool {
        let Ok(url) = url::Url::parse(base_url.trim()) else {
            return false;
        };
        url.scheme() == "https"
            && url.host_str() == Some("api.anthropic.com")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && matches!(url.path(), "/v1" | "/v1/")
            && url.query().is_none()
            && url.fragment().is_none()
    }

    /// Explicit config wins over ANTHROPIC_BASE_URL, then the native default.
    pub fn resolve_base_url(configured: Option<&str>) -> String {
        configured
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                std::env::var(env_vars::ANTHROPIC_BASE_URL)
                    .ok()
                    .map(|url| url.trim().to_owned())
                    .filter(|url| !url.is_empty())
            })
            .unwrap_or_else(|| urls::ANTHROPIC_API_BASE.to_owned())
    }
}

impl VTCodeConfig {
    /// Mirror native dispatch eligibility for commands, picker, and display.
    pub fn anthropic_speed_supported(&self, provider: &str, model: &str) -> bool {
        vtcode_commons::provider::Provider::Anthropic.name_matches(provider)
            && self.custom_provider(provider).is_none()
            && AnthropicSpeed::supports_model(model)
            && AnthropicSpeed::is_native_endpoint(&AnthropicSpeed::resolve_base_url(
                self.provider_overrides
                    .get("anthropic")
                    .and_then(|config| config.base_url.as_deref()),
            ))
    }
}

#[cfg(test)]
mod tests;
