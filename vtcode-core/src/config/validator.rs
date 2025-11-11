//! Configuration validation utilities
//!
//! Validates VTCodeConfig against actual provider models and API keys.

use anyhow::{Context, Result, bail};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::Path;

use crate::config::api_keys::ApiKeySources;
use crate::config::api_keys::get_api_key;
use crate::config::loader::VTCodeConfig;

/// Loaded models database from docs/models.json
#[derive(Debug, Clone)]
pub struct ModelsDatabase {
    /// Provider ID -> models
    providers: HashMap<String, ProviderModels>,
}

#[derive(Debug, Clone)]
struct ProviderModels {
    #[allow(dead_code)]
    default_model: String,
    models: HashMap<String, ModelInfo>,
}

#[derive(Debug, Clone)]
struct ModelInfo {
    context_window: usize,
}

impl ModelsDatabase {
    /// Load models database from docs/models.json
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read models database from {}", path.display()))?;

        let json: JsonValue =
            serde_json::from_str(&content).context("Failed to parse models database JSON")?;

        let mut providers = HashMap::new();

        if let Some(obj) = json.as_object() {
            for (provider_id, provider_data) in obj {
                if let Some(provider_obj) = provider_data.as_object() {
                    let default_model = provider_obj
                        .get("default_model")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

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

                    providers.insert(
                        provider_id.clone(),
                        ProviderModels {
                            default_model,
                            models,
                        },
                    );
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
        let models_db = ModelsDatabase::from_file(models_db_path)?;
        Ok(ConfigValidator { models_db })
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
                    .push("Current working directory does not exist".to_string());
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

// Note: Source enum from original was for tracking validation sources,
// but since we're implementing simple validators, this is not needed yet.
#[allow(dead_code)]
enum ValidationSource {
    Config,
    Models,
}

impl ValidationResult {
    /// Check if validation has any errors
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Format results for display
    pub fn format_for_display(&self) -> String {
        let mut output = String::new();

        if !self.errors.is_empty() {
            output.push_str("Configuration Errors:\n");
            for error in &self.errors {
                output.push_str(&format!("  ❌ {}\n", error));
            }
            output.push('\n');
        }

        if !self.warnings.is_empty() {
            output.push_str("Configuration Warnings:\n");
            for warning in &self.warnings {
                output.push_str(&format!("  ⚠️  {}\n", warning));
            }
        }

        output
    }
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
    "default_model": "gemini-2.5-flash",
    "models": {
      "gemini-2.5-flash": {
        "context": 1000000
      },
      "gemini-2.5-pro": {
        "context": 2000000
      }
    }
  },
  "openai": {
    "id": "openai",
    "default_model": "gpt-4",
    "models": {
      "gpt-4": {
        "context": 128000
      },
      "gpt-4-turbo": {
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

        assert!(db.model_exists("google", "gemini-2.5-flash"));
        assert!(db.model_exists("openai", "gpt-4"));
        assert!(!db.model_exists("google", "nonexistent"));
    }

    #[test]
    fn gets_context_window() {
        let dir = create_test_models_db();
        let db = ModelsDatabase::from_file(&dir.path().join("models.json")).unwrap();

        assert_eq!(
            db.get_context_window("google", "gemini-2.5-flash"),
            Some(1000000)
        );
        assert_eq!(db.get_context_window("openai", "gpt-4"), Some(128000));
        assert_eq!(db.get_context_window("google", "nonexistent"), None);
    }

    #[test]
    fn validates_model_exists() {
        let dir = create_test_models_db();
        let validator = ConfigValidator::new(&dir.path().join("models.json")).unwrap();
        let mut config = VTCodeConfig::default();
        config.agent.provider = "google".to_string();
        config.agent.default_model = "gemini-2.5-flash".to_string();

        let result = validator.validate(&config).unwrap();
        // Model exists, so no error about model
        assert!(!result.errors.iter().any(|e| e.contains("not found")));
    }

    #[test]
    fn detects_missing_model() {
        let dir = create_test_models_db();
        let validator = ConfigValidator::new(&dir.path().join("models.json")).unwrap();
        let mut config = VTCodeConfig::default();
        config.agent.provider = "google".to_string();
        config.agent.default_model = "nonexistent-model".to_string();

        let result = validator.validate(&config).unwrap();
        assert!(result.errors.iter().any(|e| e.contains("not found")));
    }

    #[test]
    fn detects_context_window_exceeded() {
        let dir = create_test_models_db();
        let validator = ConfigValidator::new(&dir.path().join("models.json")).unwrap();
        let mut config = VTCodeConfig::default();
        config.agent.provider = "google".to_string();
        config.agent.default_model = "gemini-2.5-flash".to_string();
        config.context.max_context_tokens = 2000000; // Exceeds 1000000 limit

        let result = validator.validate(&config).unwrap();
        assert!(result.warnings.iter().any(|w| w.contains("exceeds")));
    }
}
