/// Configuration validation module
///
/// Provides comprehensive validation of VTCodeConfig at startup to catch
/// common configuration errors early and provide helpful error messages.
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::Path;

use crate::config::FullAutoConfig;
use crate::config::loader::VTCodeConfig;
use serde_json::Value as JsonValue;

/// Result of a configuration validation check
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn add_error(&mut self, error: String) {
        self.is_valid = false;
        self.errors.push(error);
    }

    pub fn add_warning(&mut self, warning: String) {
        self.warnings.push(warning);
    }

    pub fn to_result(self) -> Result<()> {
        if !self.is_valid {
            let error_msg = self
                .errors
                .iter()
                .enumerate()
                .map(|(i, e)| format!("  {}. {}", i + 1, e))
                .collect::<Vec<_>>()
                .join("\n");

            bail!("Configuration validation failed:\n{}", error_msg);
        }

        // Print warnings if any
        for warning in &self.warnings {
            eprintln!("⚠️  Configuration warning: {}", warning);
        }

        Ok(())
    }
}

/// Load and parse models.json
fn load_models_json() -> Result<JsonValue> {
    // Try to load from docs/models.json relative to current dir or workspace
    let paths = [
        std::path::PathBuf::from("docs/models.json"),
        std::path::PathBuf::from("../docs/models.json"),
        std::path::PathBuf::from("../../docs/models.json"),
    ];

    for path in &paths {
        if path.exists() {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            return serde_json::from_str(&content)
                .context("Failed to parse docs/models.json. Check JSON syntax.");
        }
    }

    anyhow::bail!("Could not find docs/models.json. Checked: {:?}", paths)
}

/// Get available models from models.json
fn get_available_models() -> Result<HashMap<String, Vec<String>>> {
    let models_json = load_models_json()?;

    let mut result = HashMap::new();

    if let Some(providers) = models_json.as_object() {
        for (provider_name, provider_data) in providers {
            if let Some(provider_obj) = provider_data.as_object() {
                if let Some(models_obj) = provider_obj.get("models").and_then(|m| m.as_object()) {
                    let model_ids: Vec<String> = models_obj.keys().cloned().collect();
                    result.insert(provider_name.clone(), model_ids);
                }
            }
        }
    }

    Ok(result)
}

/// Validate that the configured model exists in models.json
pub fn validate_model_exists(provider: &str, model: &str) -> Result<()> {
    let available_models =
        get_available_models().context("Failed to load available models from docs/models.json")?;

    match available_models.get(provider) {
        Some(models) => {
            if !models.contains(&model.to_string()) {
                bail!(
                    "Model '{}' not found for provider '{}'. Available models: {}",
                    model,
                    provider,
                    models.join(", ")
                );
            }
            Ok(())
        }
        None => {
            let providers: Vec<&str> = available_models.keys().map(|s| s.as_str()).collect();
            bail!(
                "Provider '{}' not recognized. Available providers: {}",
                provider,
                providers.join(", ")
            );
        }
    }
}

/// Get context window size for a model
fn get_model_context_window(provider: &str, model: &str) -> Result<Option<usize>> {
    let models_json = load_models_json()?;

    if let Some(provider_data) = models_json.get(provider).and_then(|p| p.as_object()) {
        if let Some(model_data) = provider_data
            .get("models")
            .and_then(|m| m.as_object())
            .and_then(|m| m.get(model))
            .and_then(|m| m.as_object())
        {
            if let Some(context_size) = model_data.get("context").and_then(|c| c.as_u64()) {
                return Ok(Some(context_size as usize));
            }
        }
    }

    Ok(None)
}

/// Validate full VTCodeConfig at startup
pub fn validate_config(config: &VTCodeConfig, workspace: &Path) -> Result<ValidationResult> {
    let mut result = ValidationResult::new();

    // Validate agent model exists
    validate_agent_model(
        &config.agent.provider,
        &config.agent.default_model,
        &mut result,
    );

    // Validate context window if specified
    validate_context_window(config, &mut result);

    // Validate checkpointing directory if enabled
    if config.agent.checkpointing.enabled {
        if let Some(storage_dir) = &config.agent.checkpointing.storage_dir {
            validate_checkpointing_dir(storage_dir, workspace, &mut result);
        }
    }

    // Validate automation configuration
    if config.automation.full_auto.enabled {
        validate_full_auto_config(&config.automation.full_auto, workspace, &mut result);
    }

    Ok(result)
}

fn validate_agent_model(provider: &str, model: &str, result: &mut ValidationResult) {
    match validate_model_exists(provider, model) {
        Ok(_) => {
            // Also check context window
            if let Ok(Some(context_size)) = get_model_context_window(provider, model) {
                let display_size = if context_size >= 1_000_000 {
                    format!("{}M", context_size / 1_000_000)
                } else if context_size >= 1_000 {
                    format!("{}K", context_size / 1_000)
                } else {
                    format!("{}", context_size)
                };
                tracing::debug!("Agent model '{}' context window: {}", model, display_size);
            }
        }
        Err(e) => {
            result.add_error(format!("Agent model configuration invalid: {}", e));
        }
    }
}

fn validate_context_window(config: &VTCodeConfig, result: &mut ValidationResult) {
    let context_window = config.context.max_context_tokens;
    if context_window > 0 {
        if let Ok(Some(model_context)) =
            get_model_context_window(&config.agent.provider, &config.agent.default_model)
        {
            if context_window > model_context {
                result.add_warning(format!(
                    "Configured context window {} exceeds model capacity {}. \
                     The model will use its maximum context size.",
                    context_window, model_context
                ));
            }
        }
    }
}

fn validate_checkpointing_dir(storage_dir: &str, workspace: &Path, result: &mut ValidationResult) {
    let path = if std::path::Path::new(storage_dir).is_absolute() {
        std::path::PathBuf::from(storage_dir)
    } else {
        workspace.join(storage_dir)
    };

    // Check if parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            result.add_warning(format!(
                "Checkpointing storage directory parent '{}' does not exist. \
                 It will be created when checkpointing is first used.",
                parent.display()
            ));
        }
    }
}

fn validate_full_auto_config(
    full_auto_cfg: &FullAutoConfig,
    workspace: &Path,
    result: &mut ValidationResult,
) {
    if full_auto_cfg.require_profile_ack {
        if let Some(profile_path) = &full_auto_cfg.profile_path {
            let resolved = if std::path::Path::new(profile_path).is_absolute() {
                std::path::PathBuf::from(profile_path)
            } else {
                workspace.join(profile_path)
            };

            if !resolved.exists() {
                result.add_error(format!(
                    "Full-auto profile '{}' required but not found. \
                     Create the acknowledgement file before using --full-auto.",
                    resolved.display()
                ));
            }
        } else {
            result.add_error(
                "Full-auto profile_path is required when require_profile_ack = true".to_string(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_models_json() {
        let result = load_models_json();
        assert!(result.is_ok(), "Should load models.json successfully");
    }

    #[test]
    fn gets_available_models() {
        let result = get_available_models();
        assert!(result.is_ok(), "Should get available models");

        let models = result.unwrap();
        assert!(!models.is_empty(), "Should have at least one provider");

        // Check for common providers
        assert!(
            models.contains_key("google") || models.contains_key("openai"),
            "Should have at least one major provider"
        );
    }

    #[test]
    fn validates_known_model() {
        // This will succeed if google provider and gemini-2.5-flash exist
        let result = validate_model_exists("google", "gemini-2.5-flash");
        assert!(
            result.is_ok(),
            "Should validate gemini-2.5-flash for google provider"
        );
    }

    #[test]
    fn rejects_unknown_model() {
        let result = validate_model_exists("google", "model-does-not-exist");
        assert!(result.is_err(), "Should reject unknown model");
    }

    #[test]
    fn rejects_unknown_provider() {
        let result = validate_model_exists("provider-does-not-exist", "some-model");
        assert!(result.is_err(), "Should reject unknown provider");
    }

    #[test]
    fn gets_context_window() {
        let result = get_model_context_window("google", "gemini-2.5-flash");
        assert!(result.is_ok(), "Should get context window");

        let context = result.unwrap();
        assert!(
            context.is_some() && context.unwrap() > 0,
            "Should have positive context window"
        );
    }

    #[test]
    fn validation_result_collects_errors() {
        let mut result = ValidationResult::new();
        assert!(result.is_valid);

        result.add_error("Error 1".to_string());
        assert!(!result.is_valid);

        result.add_error("Error 2".to_string());
        assert_eq!(result.errors.len(), 2);
    }

    #[test]
    fn validation_result_collects_warnings() {
        let mut result = ValidationResult::new();
        result.add_warning("Warning 1".to_string());
        assert_eq!(result.warnings.len(), 1);
        assert!(result.is_valid); // Warnings don't invalidate
    }
}
