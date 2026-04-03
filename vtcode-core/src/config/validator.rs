//! Configuration validation utilities
//!
//! Validates VTCodeConfig against actual provider models and API keys.

use anyhow::{Context, Result, bail};
use hashbrown::HashMap;
use serde_json::Value as JsonValue;
use std::path::Path;

use crate::config::api_keys::ApiKeySources;
use crate::config::api_keys::get_api_key;
use crate::config::constants::models::openai::RESPONSES_API_MODELS;
use crate::config::loader::VTCodeConfig;
use crate::config::models::{Provider, model_catalog_entry, supported_models_for_provider};
use crate::utils::file_utils::read_file_with_context_sync;

/// Loaded models database from docs/models.json
#[derive(Debug, Clone)]
enum ModelsDatabase {
    Generated,
    File {
        providers: HashMap<String, ProviderModels>,
    },
}

#[derive(Debug, Clone)]
struct ProviderModels {
    models: HashMap<String, ModelInfo>,
}

#[derive(Debug, Clone)]
struct ModelInfo {
    context_window: usize,
}

impl ModelsDatabase {
    pub fn generated() -> Self {
        Self::Generated
    }

    /// Load models database from docs/models.json
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = read_file_with_context_sync(path, "models database")?;

        let json: JsonValue =
            serde_json::from_str(&content).context("Failed to parse models database JSON")?;

        let mut providers = HashMap::new();

        if let Some(obj) = json.as_object() {
            for (provider_id, provider_data) in obj {
                if let Some(provider_obj) = provider_data.as_object() {
                    let mut models = HashMap::new();

                    if let Some(models_obj) = provider_obj.get("models").and_then(|v| v.as_object())
                    {
                        for (model_id, model_data) in models_obj {
                            let context_window = model_data
                                .get("context")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0)
                                as usize;

                            models.insert(model_id.clone(), ModelInfo { context_window });
                        }
                    }

                    providers.insert(provider_id.clone(), ProviderModels { models });
                }
            }
        }

        Ok(Self::File { providers })
    }

    /// Check if model exists for provider
    pub fn model_exists(&self, provider: &str, model: &str) -> bool {
        match self {
            Self::Generated => supported_models_for_provider(provider)
                .map(|models| models.contains(&model))
                .unwrap_or(false),
            Self::File { providers } => providers
                .get(provider)
                .map(|p| p.models.contains_key(model))
                .unwrap_or(false),
        }
    }

    /// Get context window size for model
    pub fn get_context_window(&self, provider: &str, model: &str) -> Option<usize> {
        match self {
            Self::Generated => model_catalog_entry(provider, model)
                .map(|entry| entry.context_window)
                .filter(|context_window| *context_window > 0),
            Self::File { providers } => providers
                .get(provider)
                .and_then(|p| p.models.get(model))
                .map(|m| m.context_window),
        }
    }
}

/// Configuration validator
pub struct ConfigValidator {
    models_db: ModelsDatabase,
}

impl ConfigValidator {
    /// Create a validator backed by the build-generated model catalog.
    pub fn generated() -> Self {
        Self {
            models_db: ModelsDatabase::generated(),
        }
    }

    /// Create a new validator from models database file
    pub fn new(models_db_path: &Path) -> Result<Self> {
        Ok(Self {
            models_db: ModelsDatabase::from_file(models_db_path)?,
        })
    }

    /// Validate entire configuration
    pub fn validate(&self, config: &VTCodeConfig) -> Result<ValidationResult> {
        let mut result = ValidationResult::default();
        let managed_auth_provider = configured_managed_auth_provider(config);
        let custom_provider = config.custom_provider(&config.agent.provider);
        let is_custom_provider = custom_provider.is_some();
        let is_codex_provider = config.agent.provider.eq_ignore_ascii_case("codex");

        // Check if configured model exists
        if !is_custom_provider
            && !is_codex_provider
            && !is_managed_auth_model(managed_auth_provider, &config.agent.default_model)
            && !self
                .models_db
                .model_exists(&config.agent.provider, &config.agent.default_model)
        {
            result.errors.push(format!(
                "Model '{}' not found for provider '{}'. Check docs/models.json.",
                config.agent.default_model, config.agent.provider
            ));
        }

        // Check if API key is available
        if !is_custom_provider
            && !is_codex_provider
            && managed_auth_provider.is_none()
            && let Err(e) = get_api_key(&config.agent.provider, &ApiKeySources::default())
        {
            result.errors.push(format!(
                "API key not found for provider '{}': {}. Set {} environment variable.",
                config.agent.provider,
                e,
                config.agent.provider.to_uppercase()
            ));
        }

        // Check context window configuration
        if !is_custom_provider
            && let Some(max_tokens) = self
                .models_db
                .get_context_window(&config.agent.provider, &config.agent.default_model)
        {
            let configured_context = config.context.max_context_tokens;
            if configured_context > 0 && configured_context > max_tokens {
                result.warnings.push(format!(
                    "Configured context window ({} tokens) exceeds model limit ({} tokens) for {} on {}",
                    configured_context, max_tokens,
                    config.agent.default_model, config.agent.provider
                ));
            }
        }

        if let Some(message) = check_openai_hosted_shell_compat(
            config,
            &config.agent.default_model,
            &config.agent.provider,
        ) {
            result.warnings.push(message);
        }

        // Check if workspace exists (if specified)
        if let Ok(cwd) = std::env::current_dir() {
            // Basic check only, actual workspace validation happens in StartupContext
            if !cwd.exists() {
                result
                    .warnings
                    .push("Current working directory does not exist".to_owned());
            }
        }

        Ok(result)
    }

    /// Quick validation - only critical checks
    pub fn quick_validate(&self, config: &VTCodeConfig) -> Result<()> {
        let managed_auth_provider = configured_managed_auth_provider(config);
        let is_custom_provider = config.custom_provider(&config.agent.provider).is_some();
        let is_codex_provider = config.agent.provider.eq_ignore_ascii_case("codex");

        // Check model exists
        if !is_custom_provider
            && !is_codex_provider
            && !is_managed_auth_model(managed_auth_provider, &config.agent.default_model)
            && !self
                .models_db
                .model_exists(&config.agent.provider, &config.agent.default_model)
        {
            bail!(
                "Model '{}' not found for provider '{}'. Check docs/models.json.",
                config.agent.default_model,
                config.agent.provider
            );
        }

        // Check API key
        if !is_custom_provider && !is_codex_provider && managed_auth_provider.is_none() {
            get_api_key(&config.agent.provider, &ApiKeySources::default()).with_context(|| {
                format!(
                    "API key not found for provider '{}'. Set {} environment variable.",
                    config.agent.provider,
                    config.agent.provider.to_uppercase()
                )
            })?;
        }

        Ok(())
    }
}

fn configured_managed_auth_provider(config: &VTCodeConfig) -> Option<Provider> {
    config
        .agent
        .provider
        .parse::<Provider>()
        .ok()
        .filter(|provider| provider.uses_managed_auth())
}

fn is_managed_auth_model(provider: Option<Provider>, model: &str) -> bool {
    matches!(provider, Some(Provider::Copilot)) && !model.trim().is_empty()
}

/// Results from configuration validation
#[derive(Debug, Default, Clone)]
pub struct ValidationResult {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn check_prompt_cache_retention_compat(
    config: &VTCodeConfig,
    model: &str,
    provider: &str,
) -> Option<String> {
    if !provider.eq_ignore_ascii_case("openai") {
        return None;
    }

    if let Some(ref retention) = config.prompt_cache.providers.openai.prompt_cache_retention {
        if retention.trim().is_empty() {
            return None;
        }
        if !RESPONSES_API_MODELS.contains(&model) {
            return Some(format!(
                "`prompt_cache_retention` is set but the selected model '{}' does not use the OpenAI Responses API. The setting will be ignored for this model. Run `vtcode models list --provider openai` to see supported Responses API models.",
                model
            ));
        }
    }

    None
}

pub fn check_openai_hosted_shell_compat(
    config: &VTCodeConfig,
    model: &str,
    provider: &str,
) -> Option<String> {
    if !provider.eq_ignore_ascii_case("openai") {
        return None;
    }

    let hosted_shell = &config.provider.openai.hosted_shell;
    if !hosted_shell.enabled {
        return None;
    }

    if !RESPONSES_API_MODELS.contains(&model) {
        return Some(format!(
            "`provider.openai.hosted_shell.enabled` is set but the selected model '{}' does not use the OpenAI Responses API. VT Code will ignore hosted shell and keep the local shell tool for this model.",
            model
        ));
    }

    if !hosted_shell.has_valid_reference_target() {
        return Some(
            "`provider.openai.hosted_shell.environment = \"container_reference\"` requires a non-empty `provider.openai.hosted_shell.container_id`. VT Code will ignore hosted shell until a container ID is configured."
                .to_string(),
        );
    }

    if hosted_shell.uses_container_reference()
        && (!hosted_shell.file_ids.is_empty()
            || !hosted_shell.skills.is_empty()
            || hosted_shell.network_policy.is_allowlist())
    {
        return Some(
            "`provider.openai.hosted_shell.file_ids`, `provider.openai.hosted_shell.skills`, and allowlist `provider.openai.hosted_shell.network_policy` settings are only used with `container_auto`. VT Code will ignore those fields while `container_reference` is selected."
                .to_string(),
        );
    }

    if let Some(message) = hosted_shell.first_invalid_skill_message() {
        return Some(format!(
            "{} VT Code will ignore hosted shell until the mounted skills are corrected.",
            message
        ));
    }

    if let Some(message) = hosted_shell.first_invalid_network_policy_message() {
        return Some(format!(
            "{} VT Code will ignore hosted shell until the hosted shell network policy is corrected.",
            message
        ));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_models_db() -> TempDir {
        let dir = TempDir::new().unwrap();
        let models_json = r#"{
  "google": {
    "id": "google",
    "default_model": "gemini-3-flash-preview",
    "models": {
      "gemini-3-flash-preview": {
        "context": 1048576
      }
    }
  },
  "openai": {
    "id": "openai",
    "default_model": "gpt-5",
    "models": {
      "gpt-5": {
        "context": 128000
      }
    }
  }
}"#;
        fs::write(dir.path().join("models.json"), models_json).unwrap();
        dir
    }

    #[test]
    fn loads_models_database() {
        let dir = create_test_models_db();
        let db = ModelsDatabase::from_file(&dir.path().join("models.json")).unwrap();

        assert!(db.model_exists("google", "gemini-3-flash-preview"));
        assert!(db.model_exists("openai", "gpt-5"));
        assert!(!db.model_exists("google", "nonexistent"));
    }

    #[test]
    fn gets_context_window() {
        let dir = create_test_models_db();
        let db = ModelsDatabase::from_file(&dir.path().join("models.json")).unwrap();

        assert_eq!(
            db.get_context_window("google", "gemini-3-flash-preview"),
            Some(1048576)
        );
        assert_eq!(db.get_context_window("openai", "gpt-5"), Some(128000));
        assert_eq!(db.get_context_window("google", "nonexistent"), None);
    }

    #[test]
    fn validates_model_exists() {
        let dir = create_test_models_db();
        let validator = ConfigValidator::new(&dir.path().join("models.json")).unwrap();
        let mut config = VTCodeConfig::default();
        config.agent.provider = "google".to_owned();
        config.agent.default_model = "gemini-3-flash-preview".to_owned();

        let result = validator.validate(&config).unwrap();
        // Model exists, so there should be no model-specific lookup error.
        assert!(!result.errors.iter().any(|e| {
            e.contains("Model 'gemini-3-flash-preview' not found for provider 'google'")
        }));
    }

    #[test]
    fn custom_provider_skips_builtin_model_catalog_checks() {
        let dir = create_test_models_db();
        let validator = ConfigValidator::new(&dir.path().join("models.json")).unwrap();
        let mut config = VTCodeConfig::default();
        config.agent.provider = "mycorp".to_owned();
        config.agent.default_model = "totally-custom-model".to_owned();
        config
            .custom_providers
            .push(vtcode_config::core::CustomProviderConfig {
                name: "mycorp".to_string(),
                display_name: "MyCorporateName".to_string(),
                base_url: "https://llm.example/v1".to_string(),
                api_key_env: "MYCORP_API_KEY".to_string(),
                auth: None,
                model: "totally-custom-model".to_string(),
            });

        let result = validator.validate(&config).unwrap();

        assert!(result.errors.is_empty());
    }

    #[test]
    fn codex_provider_skips_builtin_model_and_api_key_checks() {
        let dir = create_test_models_db();
        let validator = ConfigValidator::new(&dir.path().join("models.json")).unwrap();
        let mut config = VTCodeConfig::default();
        config.agent.provider = "codex".to_owned();
        config.agent.default_model = "managed-by-codex".to_owned();

        let result = validator.validate(&config).unwrap();

        assert!(result.errors.is_empty());
    }

    #[test]
    fn copilot_managed_auth_model_accepts_live_raw_ids() {
        assert!(is_managed_auth_model(
            Some(Provider::Copilot),
            "gpt-5.3-codex"
        ));
        assert!(!is_managed_auth_model(Some(Provider::Copilot), "   "));
    }

    #[test]
    fn detects_missing_model() {
        let dir = create_test_models_db();
        let validator = ConfigValidator::new(&dir.path().join("models.json")).unwrap();
        let mut config = VTCodeConfig::default();
        config.agent.provider = "google".to_owned();
        config.agent.default_model = "nonexistent-model".to_owned();

        let result = validator.validate(&config).unwrap();
        assert!(result.errors.iter().any(|e| e.contains("not found")));
    }

    #[test]
    fn detects_context_window_exceeded() {
        let dir = create_test_models_db();
        let validator = ConfigValidator::new(&dir.path().join("models.json")).unwrap();
        let mut config = VTCodeConfig::default();
        config.agent.provider = "google".to_owned();
        config.agent.default_model = "gemini-3-flash-preview".to_owned();
        config.context.max_context_tokens = 2000000; // Exceeds 1048576 limit

        let result = validator.validate(&config).unwrap();
        assert!(result.warnings.iter().any(|w| w.contains("exceeds")));
    }

    #[test]
    fn retention_warning_for_non_responses_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.prompt_cache.providers.openai.prompt_cache_retention = Some("24h".to_owned());
        let msg = check_prompt_cache_retention_compat(&cfg, "gpt-oss-20b", "openai");
        assert!(msg.is_some());
    }

    #[test]
    fn retention_ok_for_responses_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.prompt_cache.providers.openai.prompt_cache_retention = Some("24h".to_owned());
        let msg = check_prompt_cache_retention_compat(
            &cfg,
            crate::config::constants::models::openai::GPT_5,
            "openai",
        );
        assert!(msg.is_none());
    }

    #[test]
    fn retention_ok_for_gpt_alias() {
        let mut cfg = VTCodeConfig::default();
        cfg.prompt_cache.providers.openai.prompt_cache_retention = Some("24h".to_owned());
        let msg = check_prompt_cache_retention_compat(
            &cfg,
            crate::config::constants::models::openai::GPT,
            "openai",
        );
        assert!(msg.is_none());
    }

    #[test]
    fn hosted_shell_warning_for_non_responses_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.provider.openai.hosted_shell.enabled = true;

        let msg = check_openai_hosted_shell_compat(&cfg, "gpt-oss-20b", "openai");
        assert!(msg.is_some());
    }

    #[test]
    fn hosted_shell_warning_for_missing_container_reference_id() {
        let mut cfg = VTCodeConfig::default();
        cfg.provider.openai.hosted_shell.enabled = true;
        cfg.provider.openai.hosted_shell.environment =
            crate::config::core::OpenAIHostedShellEnvironment::ContainerReference;

        let msg = check_openai_hosted_shell_compat(
            &cfg,
            crate::config::constants::models::openai::GPT_5,
            "openai",
        );
        assert!(msg.is_some());
    }

    #[test]
    fn hosted_shell_warning_for_auto_only_fields_on_container_reference() {
        let mut cfg = VTCodeConfig::default();
        cfg.provider.openai.hosted_shell.enabled = true;
        cfg.provider.openai.hosted_shell.environment =
            crate::config::core::OpenAIHostedShellEnvironment::ContainerReference;
        cfg.provider.openai.hosted_shell.container_id = Some("cntr_123".to_string());
        cfg.provider.openai.hosted_shell.file_ids = vec!["file_123".to_string()];
        cfg.provider.openai.hosted_shell.network_policy.policy_type =
            vtcode_config::core::OpenAIHostedShellNetworkPolicyType::Allowlist;
        cfg.provider
            .openai
            .hosted_shell
            .network_policy
            .allowed_domains = vec!["httpbin.org".to_string()];

        let msg = check_openai_hosted_shell_compat(
            &cfg,
            crate::config::constants::models::openai::GPT_5,
            "openai",
        );
        assert!(msg.is_some());
    }

    #[test]
    fn hosted_shell_ok_for_valid_responses_config() {
        let mut cfg = VTCodeConfig::default();
        cfg.provider.openai.hosted_shell.enabled = true;

        let msg = check_openai_hosted_shell_compat(
            &cfg,
            crate::config::constants::models::openai::GPT_5,
            "openai",
        );
        assert!(msg.is_none());
    }

    #[test]
    fn hosted_shell_ok_for_gpt_alias() {
        let mut cfg = VTCodeConfig::default();
        cfg.provider.openai.hosted_shell.enabled = true;

        let msg = check_openai_hosted_shell_compat(
            &cfg,
            crate::config::constants::models::openai::GPT,
            "openai",
        );
        assert!(msg.is_none());
    }

    #[test]
    fn hosted_shell_warning_for_empty_skill_reference_id() {
        let mut cfg = VTCodeConfig::default();
        cfg.provider.openai.hosted_shell.enabled = true;
        cfg.provider.openai.hosted_shell.skills =
            vec![vtcode_config::core::OpenAIHostedSkill::SkillReference {
                skill_id: "   ".to_string(),
                version: vtcode_config::core::OpenAIHostedSkillVersion::default(),
            }];

        let msg = check_openai_hosted_shell_compat(&cfg, "gpt-5", "openai");

        assert!(
            msg.as_deref()
                .unwrap_or_default()
                .contains("provider.openai.hosted_shell.skills[0].skill_id")
        );
    }

    #[test]
    fn hosted_shell_warning_for_empty_inline_bundle() {
        let mut cfg = VTCodeConfig::default();
        cfg.provider.openai.hosted_shell.enabled = true;
        cfg.provider.openai.hosted_shell.skills =
            vec![vtcode_config::core::OpenAIHostedSkill::Inline {
                bundle_b64: " ".to_string(),
                sha256: None,
            }];

        let msg = check_openai_hosted_shell_compat(&cfg, "gpt-5", "openai");

        assert!(
            msg.as_deref()
                .unwrap_or_default()
                .contains("provider.openai.hosted_shell.skills[0].bundle_b64")
        );
    }

    #[test]
    fn hosted_shell_warning_for_empty_allowlist_domains() {
        let mut cfg = VTCodeConfig::default();
        cfg.provider.openai.hosted_shell.enabled = true;
        cfg.provider.openai.hosted_shell.network_policy.policy_type =
            vtcode_config::core::OpenAIHostedShellNetworkPolicyType::Allowlist;

        let msg = check_openai_hosted_shell_compat(&cfg, "gpt-5", "openai");

        assert!(
            msg.as_deref()
                .unwrap_or_default()
                .contains("network_policy.allowed_domains")
        );
    }

    #[test]
    fn hosted_shell_warning_for_secret_domain_outside_allowlist() {
        let mut cfg = VTCodeConfig::default();
        cfg.provider.openai.hosted_shell.enabled = true;
        cfg.provider.openai.hosted_shell.network_policy.policy_type =
            vtcode_config::core::OpenAIHostedShellNetworkPolicyType::Allowlist;
        cfg.provider
            .openai
            .hosted_shell
            .network_policy
            .allowed_domains = vec!["pypi.org".to_string()];
        cfg.provider
            .openai
            .hosted_shell
            .network_policy
            .domain_secrets = vec![vtcode_config::core::OpenAIHostedShellDomainSecret {
            domain: "httpbin.org".to_string(),
            name: "API_KEY".to_string(),
            value: "secret".to_string(),
        }];

        let msg = check_openai_hosted_shell_compat(&cfg, "gpt-5", "openai");

        assert!(
            msg.as_deref()
                .unwrap_or_default()
                .contains("domain_secrets[0].domain")
        );
    }

    #[test]
    fn validate_surfaces_hosted_shell_warning() {
        let dir = create_test_models_db();
        let validator = ConfigValidator::new(&dir.path().join("models.json")).unwrap();
        let mut config = VTCodeConfig::default();
        config.agent.provider = "openai".to_owned();
        config.agent.default_model = "gpt-5".to_owned();
        config.provider.openai.hosted_shell.enabled = true;
        config.provider.openai.hosted_shell.skills =
            vec![vtcode_config::core::OpenAIHostedSkill::SkillReference {
                skill_id: "   ".to_string(),
                version: vtcode_config::core::OpenAIHostedSkillVersion::default(),
            }];

        let result = validator.validate(&config).unwrap();

        assert!(result.warnings.iter().any(|warning| {
            warning.contains("provider.openai.hosted_shell.skills[0].skill_id")
        }));
    }
}
