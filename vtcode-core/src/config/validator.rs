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
use crate::utils::file_utils::read_file_with_context_sync;

/// Loaded models database from docs/models.json
#[derive(Debug, Clone)]
struct ModelsDatabase {
    /// Provider ID -> models
    providers: HashMap<String, ProviderModels>,
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

        Ok(ModelsDatabase { providers })
    }

    /// Check if model exists for provider
    pub fn model_exists(&self, provider: &str, model: &str) -> bool {
        self.providers
            .get(provider)
            .map(|p| p.models.contains_key(model))
            .unwrap_or(false)
    }

    /// Get context window size for model
    pub fn get_context_window(&self, provider: &str, model: &str) -> Option<usize> {
        self.providers
            .get(provider)
            .and_then(|p| p.models.get(model))
            .map(|m| m.context_window)
    }
}

/// Configuration validator
pub struct ConfigValidator {
    models_db: ModelsDatabase,
}

impl ConfigValidator {
    /// Create a new validator from models database file
    pub fn new(models_db_path: &Path) -> Result<Self> {
        Ok(Self {
            models_db: ModelsDatabase::from_file(models_db_path)?,
        })
    }

    /// Validate entire configuration
    pub fn validate(&self, config: &VTCodeConfig) -> Result<ValidationResult> {
        let mut result = ValidationResult::default();

        // Check if configured model exists
        if !self
            .models_db
            .model_exists(&config.agent.provider, &config.agent.default_model)
        {
            result.errors.push(format!(
                "Model '{}' not found for provider '{}'. Check docs/models.json.",
                config.agent.default_model, config.agent.provider
            ));
        }

        // Check if API key is available
        if let Err(e) = get_api_key(&config.agent.provider, &ApiKeySources::default()) {
            result.errors.push(format!(
                "API key not found for provider '{}': {}. Set {} environment variable.",
                config.agent.provider,
                e,
                config.agent.provider.to_uppercase()
            ));
        }

        // Check context window configuration
        if let Some(max_tokens) = self
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
        // Check model exists
        if !self
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
        get_api_key(&config.agent.provider, &ApiKeySources::default()).with_context(|| {
            format!(
                "API key not found for provider '{}'. Set {} environment variable.",
                config.agent.provider,
                config.agent.provider.to_uppercase()
            )
        })?;

        Ok(())
    }
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
}
