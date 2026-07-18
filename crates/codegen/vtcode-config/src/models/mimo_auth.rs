use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::constants::{env_vars, urls};

/// The env key used by Provider::MiMo for pay-as-you-go
const MIMO_API_KEY: &str = "MIMO_API_KEY";

/// Authentication method for Xiaomi MiMo provider
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MiMoAuthMethod {
    /// Pay-as-you-go: uses api-key header, sk- prefix, api.xiaomimimo.com/v1
    #[default]
    #[serde(alias = "payg", alias = "pay_as_you_go", alias = "apikey", alias = "api_key")]
    PayAsYouGo,
    /// Token Plan: uses Authorization Bearer header, tp- prefix, token-plan-cn.xiaomimimo.com/v1
    #[serde(alias = "token_plan", alias = "tokenplan", alias = "tp")]
    TokenPlan,
    /// Forward-compatible catch-all for unknown auth methods
    #[serde(other)]
    Unknown,
}

impl MiMoAuthMethod {
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::PayAsYouGo => "API Usage Billing",
            Self::TokenPlan => "Subscription Plan",
            Self::Unknown => "Unknown",
        }
    }

    #[must_use]
    pub fn description(&self) -> &'static str {
        match self {
            Self::PayAsYouGo => "Standard API access. Uses sk- key with api-key header.",
            Self::TokenPlan => {
                "Subscription-based access. Uses tp- key with Bearer token. Includes more models. Defaults to Europe cluster."
            }
            Self::Unknown => "Unrecognized authentication method.",
        }
    }

    #[must_use]
    pub fn env_key(&self) -> &'static str {
        match self {
            Self::PayAsYouGo => MIMO_API_KEY,
            Self::TokenPlan => env_vars::MIMO_TOKEN_PLAN_KEY,
            Self::Unknown => MIMO_API_KEY,
        }
    }

    #[must_use]
    pub fn api_base(&self) -> &'static str {
        match self {
            Self::PayAsYouGo => urls::MIMO_API_BASE,
            Self::TokenPlan => urls::MIMO_TOKEN_PLAN_API_BASE,
            Self::Unknown => urls::MIMO_API_BASE,
        }
    }
}

impl fmt::Display for MiMoAuthMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PayAsYouGo => write!(f, "api-usage-billing"),
            Self::TokenPlan => write!(f, "subscription-plan"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl FromStr for MiMoAuthMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "payg" | "pay-as-you-go" | "pay_as_you_go" | "apikey" | "api-key" | "api-usage-billing" => {
                Ok(Self::PayAsYouGo)
            }
            "token-plan" | "token_plan" | "tokenplan" | "tp" | "subscription-plan" => Ok(Self::TokenPlan),
            _ => Err(format!("Unknown MiMo auth method: {s}")),
        }
    }
}

/// Detect auth method from API key prefix or base URL.
///
/// Returns `PayAsYouGo` as the default if no hint is available.
#[must_use]
pub fn detect_mimo_auth_method(api_key: &str, base_url: Option<&str>) -> MiMoAuthMethod {
    if api_key.starts_with("tp-") {
        return MiMoAuthMethod::TokenPlan;
    }
    if api_key.starts_with("sk-") {
        return MiMoAuthMethod::PayAsYouGo;
    }
    // If no key prefix hint, check base URL
    if let Some(url) = base_url
        && url.contains("token-plan")
    {
        return MiMoAuthMethod::TokenPlan;
    }
    // Check env var for base URL override
    if let Ok(url) = std::env::var(env_vars::MIMO_TOKEN_PLAN_BASE_URL)
        && !url.trim().is_empty()
    {
        return MiMoAuthMethod::TokenPlan;
    }
    MiMoAuthMethod::PayAsYouGo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_from_key_prefix() {
        assert_eq!(detect_mimo_auth_method("sk-abc123", None), MiMoAuthMethod::PayAsYouGo);
        assert_eq!(detect_mimo_auth_method("tp-abc123", None), MiMoAuthMethod::TokenPlan);
    }

    #[test]
    fn detect_from_base_url() {
        assert_eq!(
            detect_mimo_auth_method("abc", Some("https://token-plan-cn.xiaomimimo.com/v1")),
            MiMoAuthMethod::TokenPlan
        );
        assert_eq!(detect_mimo_auth_method("abc", Some("https://api.xiaomimimo.com/v1")), MiMoAuthMethod::PayAsYouGo);
    }

    #[test]
    fn detect_defaults_to_payg() {
        assert_eq!(detect_mimo_auth_method("", None), MiMoAuthMethod::PayAsYouGo);
        assert_eq!(detect_mimo_auth_method("unknown", None), MiMoAuthMethod::PayAsYouGo);
    }

    #[test]
    fn parse_from_str() {
        assert_eq!("payg".parse::<MiMoAuthMethod>().unwrap(), MiMoAuthMethod::PayAsYouGo);
        assert_eq!("token-plan".parse::<MiMoAuthMethod>().unwrap(), MiMoAuthMethod::TokenPlan);
        assert_eq!("tp".parse::<MiMoAuthMethod>().unwrap(), MiMoAuthMethod::TokenPlan);
        assert!("invalid".parse::<MiMoAuthMethod>().is_err());
    }

    #[test]
    fn display_roundtrip() {
        let payg = MiMoAuthMethod::PayAsYouGo;
        assert_eq!(payg.to_string(), "api-usage-billing");
        let tp = MiMoAuthMethod::TokenPlan;
        assert_eq!(tp.to_string(), "subscription-plan");
    }
}
