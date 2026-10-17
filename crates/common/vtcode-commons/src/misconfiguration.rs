//! Misconfiguration-first failure classification.
//!
//! For every runtime failure, callers must check settings/config before
//! retrying. This module is the shared detector: given an [`ErrorCategory`]
//! plus the raw error text, it returns actionable [`ConfigGuidance`] when the
//! failure is caused by user misconfiguration (bad credentials, unknown
//! model/provider, invalid `base_url`, malformed `vtcode.toml`, bad MCP or
//! sampling params).
//!
//! Transient failures (network, timeout, rate-limit, 5xx) return `None` so
//! existing retry policy applies unchanged. LLM argument mistakes (bad patch,
//! bad tool args) also return `None` — they lack config markers.

use std::borrow::Cow;

use crate::error_category::ErrorCategory;

/// Kind of user misconfiguration detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MisconfigurationKind {
    Authentication,
    Model,
    Provider,
    BaseUrl,
    ApiKeyEnv,
    ConfigFile,
    Mcp,
    SamplingParams,
}

/// Actionable guidance for fixing a misconfiguration.
///
/// All strings are static to avoid allocation on the failure path.
/// Callers format them with [`ConfigGuidance::user_message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigGuidance {
    /// What kind of misconfiguration was detected.
    pub kind: MisconfigurationKind,
    /// Settings key to check (e.g. `agent.model`).
    pub setting: &'static str,
    /// Where the setting lives (e.g. `vtcode.toml`).
    pub location: &'static str,
    /// Actionable fix steps.
    pub fix: Cow<'static, str>,
}

impl ConfigGuidance {
    /// Render a user-facing message that directs to settings/config first.
    #[must_use]
    pub fn user_message(&self) -> String {
        format!(
            "Check settings/config first ({} in {}): {} Correct the configuration before retrying.",
            self.setting, self.location, self.fix
        )
    }
}

/// Return guidance when `message` indicates user misconfiguration.
///
/// `category` is used to short-circuit obvious cases (e.g. `Authentication`
/// is always misconfiguration) and to avoid false positives on transient
/// categories without config markers.
#[must_use]
pub fn detect_misconfiguration(category: ErrorCategory, message: &str) -> Option<ConfigGuidance> {
    // Authentication failures are always user-fixable credentials issues.
    if matches!(category, ErrorCategory::Authentication) {
        return Some(auth_guidance());
    }

    let msg = message.to_ascii_lowercase();

    // Config-file markers take priority: explicit vtcode.toml problems.
    if contains_any(
        &msg,
        &[
            "vtcode.toml",
            "config invalid",
            "config missing",
            "config parse",
            "configparse",
            "failed to parse config",
            "malformed config",
            "invalid custom_providers",
            "invalid provider_overrides",
            "repository-controlled",
            "protected field",
        ],
    ) || (msg.contains("workspace_lifecycle_hooks")
        && contains_any(&msg, &["invalid", "missing", "not allowed", "config"]))
    {
        return Some(ConfigGuidance {
            kind: MisconfigurationKind::ConfigFile,
            setting: "vtcode.toml",
            location: "workspace / user / system config layers",
            fix: Cow::Borrowed(
                "Invalid config file. Validate vtcode.toml syntax and fields, keep `custom_providers` and `provider_overrides.*.base_url`/`api_key_env` out of repository layers, then retry.",
            ),
        });
    }

    // Endpoint / base_url problems. Checked before generic provider
    // markers because `custom_providers[x]: base_url ...` contains both.
    if contains_any(
        &msg,
        &[
            "invalid endpoint",
            "invalid base url",
            "unsupported protocol",
            "endpoint must",
            "endpoint is invalid",
        ],
    ) || (msg.contains("base_url")
        && contains_any(
            &msg,
            &[
                "invalid",
                "empty",
                "missing",
                "must",
                "cannot",
                "malformed",
                "parse",
                "scheme",
            ],
        ))
    {
        return Some(ConfigGuidance {
            kind: MisconfigurationKind::BaseUrl,
            setting: "provider_overrides.*.base_url / custom_providers.*.base_url",
            location: "system / user / explicit config file only",
            fix: Cow::Borrowed(
                "Invalid endpoint. Check `base_url` is non-empty, uses a supported URL scheme, and is reachable, then retry.",
            ),
        });
    }

    // API-key env problems (distinct from authentication rejections above).
    if contains_any(
        &msg,
        &[
            "invalid `api_key_env`",
            "invalid api_key_env",
            "api key env",
            "api_key_env must",
            "missing api key",
            "no api key",
            "api key not found",
            "api key is not set",
            "api key not configured",
            "api key is required",
            "api key environment variable must",
            "missing mcp api key",
            "api-key env",
        ],
    ) {
        return Some(ConfigGuidance {
            kind: MisconfigurationKind::ApiKeyEnv,
            setting: "api_key_env",
            location: "vtcode.toml + environment",
            fix: Cow::Borrowed(
                "Invalid API-key config. Check `api_key_env` names a valid exported env var (or use `/secret`), then retry.",
            ),
        });
    }

    // Provider identity problems.
    if contains_any(
        &msg,
        &[
            "unknown provider",
            "unknown_provider",
            "invalid provider",
            "invalid_provider",
            "unsupported provider",
            "unsupported_provider",
            "provider not found",
            "provider_not_found",
        ],
    ) || (contains_any(&msg, &["agent.provider", "custom_providers", "provider_overrides"])
        && contains_any(&msg, &["invalid", "unknown", "unsupported", "not found", "missing", "empty"]))
    {
        return Some(ConfigGuidance {
            kind: MisconfigurationKind::Provider,
            setting: "agent.provider",
            location: "vtcode.toml",
            fix: Cow::Borrowed(
                "Unknown/invalid provider. Check `agent.provider` and `custom_providers` in vtcode.toml, then retry.",
            ),
        });
    }

    // Model identity problems. Require model-specific markers so generic
    // "not found" file errors do not match.
    if contains_any(
        &msg,
        &[
            "unknown model",
            "unknown_model",
            "invalid model",
            "invalid_model",
            "model not found",
            "model_not_found",
            "model not available",
            "model_not_available",
            "unsupported model",
            "unsupported_model",
            "model does not exist",
            "model doesn't exist",
            "model is not supported",
            "model not supported",
            "catalog-missing",
            "catalog missing",
        ],
    ) || (msg.contains("model ")
        && contains_any(&msg, &[" is not supported", " not supported by", " not supported with"]))
        || (contains_any(&msg, &["model id", "model_id", "agent.model", "agent.default_model"])
            && contains_any(
                &msg,
                &[
                    "invalid",
                    "unknown",
                    "not found",
                    "not available",
                    "unsupported",
                    "missing",
                    "empty",
                ],
            ))
    {
        return Some(ConfigGuidance {
            kind: MisconfigurationKind::Model,
            setting: "agent.default_model",
            location: "vtcode.toml / `/model` picker",
            fix: Cow::Borrowed(
                "Unknown/invalid model. Check `agent.default_model` in vtcode.toml or run `/model` to pick a supported ModelId, then retry.",
            ),
        });
    }

    // MCP / WebMCP config problems.
    if contains_any(&msg, &["mcp config", "https when enabled"])
        || (contains_any(&msg, &["mcp server", "mcp url", "webmcp"])
            && contains_any(&msg, &["invalid", "missing", "not allowed", "must", "unsupported"]))
        || (msg.contains("mcp")
            && contains_any(&msg, &["origin", "roots", "https", "url", "config"])
            && contains_any(&msg, &["invalid", "missing", "not allowed", "must", "unsupported"]))
    {
        return Some(ConfigGuidance {
            kind: MisconfigurationKind::Mcp,
            setting: "mcp / webmcp",
            location: "vtcode.toml",
            fix: Cow::Borrowed(
                "MCP misconfiguration. Check `mcp`/`webmcp` URLs are HTTPS, origins and roots are allowed, then retry.",
            ),
        });
    }

    // Sampling param range problems. Require a param name plus a range hint
    // so generic "max_tokens" context-capacity messages do not match.
    if contains_any(
        &msg,
        &[
            "temperature",
            "top_p",
            "top_k",
            "reasoning_effort",
            "max_tokens",
            "penalt",
        ],
    ) && contains_any(&msg, &["range", "out of range", "invalid", "must be", "validation"])
    {
        return Some(ConfigGuidance {
            kind: MisconfigurationKind::SamplingParams,
            setting: "sampling params",
            location: "vtcode.toml custom_providers / profile",
            fix: Cow::Borrowed(
                "Invalid sampling param. Check temperature/top_p/top_k/penalties/max_tokens/reasoning_effort ranges in the provider profile, then retry.",
            ),
        });
    }

    // Generic credential-adjacent markers that did not classify as
    // Authentication (e.g. ExecutionError wrappers around 401 text).
    if contains_any(
        &msg,
        &[
            "invalid api key",
            "invalid_api_key",
            "authentication failed",
            "authentication_failed",
            "unauthorized",
            "invalid credentials",
            "key was rejected",
            "re-authenticate",
            "/secret add",
            "/login ",
        ],
    ) {
        return Some(auth_guidance());
    }

    None
}

/// Detect configuration failures from an error that may still retain its
/// typed provider cause. Provider metadata often contains the actionable model
/// or credential code even when the `Display` message is only a generic HTTP
/// failure.
#[must_use]
pub fn detect_misconfiguration_in_anyhow(error: &anyhow::Error) -> Option<ConfigGuidance> {
    let message = format!("{error:#}");
    detect_misconfiguration(crate::error_category::classify_anyhow_error(error), &message).or_else(|| {
        error
            .downcast_ref::<crate::llm::LLMError>()
            .and_then(detect_misconfiguration_in_llm_error)
    })
}

/// Detect configuration failures using both the primary LLM error message and
/// provider metadata such as `model_not_found` or `invalid_api_key`.
#[must_use]
pub fn detect_misconfiguration_in_llm_error(error: &crate::llm::LLMError) -> Option<ConfigGuidance> {
    let mut message = error.to_string();
    if let Some(metadata) = error.metadata() {
        if let Some(code) = metadata.code.as_deref() {
            message.push(' ');
            message.push_str(code);
        }
        if let Some(provider_message) = metadata.message.as_deref() {
            message.push(' ');
            message.push_str(provider_message);
        }
    }

    detect_misconfiguration(ErrorCategory::from(error), &message)
}

/// Single source for authentication guidance (used by both the typed
/// `Authentication` branch and generic credential-text fallback).
fn auth_guidance() -> ConfigGuidance {
    ConfigGuidance {
        kind: MisconfigurationKind::Authentication,
        setting: "API key / credentials",
        location: "secure storage (`vtcode secret` or `/secret`) or environment",
        fix: Cow::Borrowed(
            "Authentication failed. Run `vtcode secret add <provider>` (or `/secret add <provider>` in TUI) for API-key providers or `vtcode login <provider>` (or `/login <provider>` in TUI) for managed auth, verify the env var is exported, then retry.",
        ),
    }
}

/// Convenience predicate for fail-fast checks.
#[inline]
#[must_use]
pub fn is_misconfiguration(category: ErrorCategory, message: &str) -> bool {
    detect_misconfiguration(category, message).is_some()
}

#[inline]
fn contains_any(message: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| message.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_category_is_always_misconfiguration() {
        let guidance = detect_misconfiguration(ErrorCategory::Authentication, "anything").expect("auth must match");
        assert_eq!(guidance.kind, MisconfigurationKind::Authentication);
        assert!(guidance.user_message().contains("before retrying"));
    }

    #[test]
    fn transient_without_markers_is_not_misconfiguration() {
        assert!(detect_misconfiguration(ErrorCategory::Network, "connection reset by peer").is_none());
        assert!(detect_misconfiguration(ErrorCategory::Timeout, "request timed out after 30s").is_none());
        assert!(detect_misconfiguration(ErrorCategory::RateLimit, "429 too many requests").is_none());
        assert!(detect_misconfiguration(ErrorCategory::ServiceUnavailable, "503 service unavailable").is_none());
        assert!(
            detect_misconfiguration(
                ErrorCategory::Network,
                "connection reset while requesting provider_overrides.openai.base_url",
            )
            .is_none()
        );
        assert!(detect_misconfiguration(ErrorCategory::Network, "mcp server connection reset by peer").is_none());
    }

    #[test]
    fn llm_mistakes_are_not_misconfiguration() {
        // Asymmetric pair: patch/arg mistakes share InvalidParameters with
        // real config errors but must not trigger config guidance.
        assert!(
            detect_misconfiguration(
                ErrorCategory::InvalidParameters,
                "invalid patch format: missing '*** Begin Patch' marker"
            )
            .is_none()
        );
        assert!(
            detect_misconfiguration(
                ErrorCategory::InvalidParameters,
                "failed to parse arguments for read_file handler: invalid type"
            )
            .is_none()
        );
        assert!(
            detect_misconfiguration(ErrorCategory::ResourceNotFound, "no such file or directory: /tmp/missing")
                .is_none()
        );
        // Positive side of the same boundary: model markers do trigger.
        assert!(
            detect_misconfiguration(ErrorCategory::InvalidParameters, "unknown model 'gpt-99' in agent.model")
                .is_some()
        );
    }

    #[test]
    fn model_vs_file_not_found_boundary() {
        let model =
            detect_misconfiguration(ErrorCategory::ResourceNotFound, "model not found: foo-bar").expect("model");
        assert_eq!(model.kind, MisconfigurationKind::Model);
        assert!(detect_misconfiguration(ErrorCategory::ResourceNotFound, "file not found: /tmp/x").is_none());

        for message in [
            "The 'gpt-5.4' model is not supported with this method.",
            "The requested model does not exist",
            "Model 'gpt-5.4' is not supported by OpenResponses provider.",
        ] {
            let guidance = detect_misconfiguration(ErrorCategory::InvalidParameters, message).expect("model");
            assert_eq!(guidance.kind, MisconfigurationKind::Model);
        }
    }

    #[test]
    fn provider_and_config_markers() {
        let provider = detect_misconfiguration(
            ErrorCategory::InvalidParameters,
            "unknown provider 'mycloud'; check agent.provider",
        )
        .expect("provider");
        assert_eq!(provider.kind, MisconfigurationKind::Provider);

        let config =
            detect_misconfiguration(ErrorCategory::ExecutionError, "config invalid: vtcode.toml has unknown field")
                .expect("config");
        assert_eq!(config.kind, MisconfigurationKind::ConfigFile);
        let invalid_provider = detect_misconfiguration(
            ErrorCategory::InvalidParameters,
            "Invalid provider_overrides configuration: provider key is empty",
        )
        .expect("invalid provider config");
        assert_eq!(invalid_provider.kind, MisconfigurationKind::ConfigFile);
    }

    #[test]
    fn base_url_and_key_env_markers() {
        let base =
            detect_misconfiguration(ErrorCategory::ExecutionError, "custom_providers[x]: `base_url` must not be empty")
                .expect("base_url");
        assert_eq!(base.kind, MisconfigurationKind::BaseUrl);

        let key = detect_misconfiguration(
            ErrorCategory::InvalidParameters,
            "providers[openai]: invalid `api_key_env`: bad name",
        )
        .expect("api_key_env");
        assert_eq!(key.kind, MisconfigurationKind::ApiKeyEnv);

        let missing = detect_misconfiguration(
            ErrorCategory::ExecutionError,
            "API key not found for provider 'openai'. Set OPENAI_API_KEY or run /secret add openai.",
        )
        .expect("missing API key");
        assert_eq!(missing.kind, MisconfigurationKind::ApiKeyEnv);

        for message in [
            "API key is required",
            "API key environment variable must be set when auth is enabled",
            "Missing MCP API key environment variable: MCP_TOKEN",
        ] {
            let guidance = detect_misconfiguration(ErrorCategory::ExecutionError, message).expect("missing API key");
            assert_eq!(guidance.kind, MisconfigurationKind::ApiKeyEnv);
        }
    }

    #[test]
    fn mcp_and_sampling_boundaries() {
        let mcp = detect_misconfiguration(
            ErrorCategory::InvalidParameters,
            "webmcp remote mcp url must be https when enabled",
        )
        .expect("mcp");
        assert_eq!(mcp.kind, MisconfigurationKind::Mcp);
        // Bare "mcp" without config context stays out.
        assert!(detect_misconfiguration(ErrorCategory::ExecutionError, "mcp tool finished").is_none());

        let sampling =
            detect_misconfiguration(ErrorCategory::InvalidParameters, "temperature out of range: must be 0..2")
                .expect("sampling");
        assert_eq!(sampling.kind, MisconfigurationKind::SamplingParams);
        let max_tokens =
            detect_misconfiguration(ErrorCategory::InvalidParameters, "max_tokens must be greater than zero")
                .expect("max_tokens sampling");
        assert_eq!(max_tokens.kind, MisconfigurationKind::SamplingParams);
        // Context-capacity max_tokens prose must not match sampling.
        assert!(
            detect_misconfiguration(
                ErrorCategory::ExecutionError,
                "input token count exceeds the maximum number of tokens"
            )
            .is_none()
        );
    }

    #[test]
    fn auth_text_inside_generic_error_is_misconfiguration() {
        let guidance = detect_misconfiguration(ErrorCategory::ExecutionError, "provider error: 401 unauthorized")
            .expect("auth text");
        assert_eq!(guidance.kind, MisconfigurationKind::Authentication);
    }

    #[test]
    fn typed_llm_metadata_overrides_generic_provider_text() {
        let error = anyhow::Error::new(crate::llm::LLMError::Provider {
            message: "HTTP 503 Service Unavailable".to_string(),
            metadata: Some(crate::llm::LLMErrorMetadata::new(
                "openai",
                Some(404),
                Some("model_not_found".to_string()),
                None,
                None,
                None,
                Some("The requested model does not exist".to_string()),
            )),
        });

        let guidance = detect_misconfiguration_in_anyhow(&error).expect("metadata model marker");
        assert_eq!(guidance.kind, MisconfigurationKind::Model);
    }

    #[test]
    fn guidance_never_echoes_secrets() {
        let secret = concat!("sk-", "test1234567890abcdef");
        let message = format!("Authentication failed: invalid api key {secret}");
        let guidance = detect_misconfiguration(ErrorCategory::ExecutionError, &message).expect("auth text must match");
        let rendered = guidance.user_message();
        assert!(rendered.contains("before retrying"));
        assert!(!rendered.contains(secret), "guidance must be static and never echo input secrets");
    }

    #[test]
    fn misconfiguration_check_is_fail_closed() {
        // Even with generous retry budgets, misconfiguration must not retry.
        // This is the fail-closed property: fix config first, never blind-retry.
        for category in [ErrorCategory::Authentication, ErrorCategory::InvalidParameters] {
            let guidance = detect_misconfiguration(category, "unknown model 'x' in agent.model");
            if category == ErrorCategory::Authentication {
                assert!(guidance.is_some());
            }
        }
        assert!(!ErrorCategory::Authentication.is_retryable());
    }
}
