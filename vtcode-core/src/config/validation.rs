/// Configuration validation module
///
/// Provides comprehensive validation of VTCodeConfig at startup to catch
/// common configuration errors early and provide helpful error messages.
use anyhow::{Result, bail};
use std::path::Path;

use crate::config::FullAutoConfig;
use crate::config::loader::VTCodeConfig;
use crate::config::models::{
    catalog_provider_keys, model_catalog_entry, supported_models_for_provider,
};

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
            eprintln!("Configuration warning: {}", warning);
        }

        Ok(())
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate that the configured model exists in the generated model catalog.
pub fn validate_model_exists(provider: &str, model: &str) -> Result<()> {
    if let Some(models) = supported_models_for_provider(provider) {
        if !models.contains(&model) {
            bail!(
                "Model '{}' not found for provider '{}'. Available models: {}",
                model,
                provider,
                models.join(", ")
            );
        }
        Ok(())
    } else {
        bail!(
            "Provider '{}' not recognized. Available providers: {}",
            provider,
            catalog_provider_keys().join(", ")
        );
    }
}

/// Get context window size for a model from the catalog.
fn catalog_model_context_window(provider: &str, model: &str) -> Result<Option<usize>> {
    Ok(model_catalog_entry(provider, model)
        .map(|entry| entry.context_window)
        .filter(|context_window| *context_window > 0))
}

/// Resolve the effective context window size for a model.
pub fn effective_model_context_window(provider: &str, model: &str) -> Result<Option<usize>> {
    if provider.eq_ignore_ascii_case("anthropic") {
        return Ok(Some(
            crate::llm::providers::anthropic::capabilities::effective_context_size(model),
        ));
    }

    catalog_model_context_window(provider, model)
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
    if config.agent.checkpointing.enabled
        && let Some(storage_dir) = &config.agent.checkpointing.storage_dir
    {
        validate_checkpointing_dir(storage_dir, workspace, &mut result);
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
            if let Ok(Some(context_size)) = effective_model_context_window(provider, model) {
                let display_size = if context_size >= 1_000_000 {
                    format!("{}M", context_size / 1_000_000)
                } else if context_size >= 1_000 {
                    format!("{}K", context_size / 1_000)
                } else {
                    context_size.to_string()
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
    if context_window > 0
        && let Ok(Some(model_context)) =
            effective_model_context_window(&config.agent.provider, &config.agent.default_model)
        && context_window > model_context
    {
        result.add_warning(format!(
            "Configured context window {} exceeds model capacity {}. \
             The model will use its maximum context size.",
            context_window, model_context
        ));
    }
}

fn validate_checkpointing_dir(storage_dir: &str, workspace: &Path, result: &mut ValidationResult) {
    let path = if Path::new(storage_dir).is_absolute() {
        std::path::PathBuf::from(storage_dir)
    } else {
        workspace.join(storage_dir)
    };

    // Check if parent directory exists
    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        result.add_warning(format!(
            "Checkpointing storage directory parent '{}' does not exist. \
             It will be created when checkpointing is first used.",
            parent.display()
        ));
    }
}

fn validate_full_auto_config(
    full_auto_cfg: &FullAutoConfig,
    workspace: &Path,
    result: &mut ValidationResult,
) {
    if full_auto_cfg.require_profile_ack {
        if let Some(profile_path) = &full_auto_cfg.profile_path {
            let resolved = if Path::new(profile_path).is_absolute() {
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
                "Full-auto profile_path is required when require_profile_ack = true".to_owned(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_catalog_contains_providers() {
        let providers = catalog_provider_keys();
        assert!(!providers.is_empty(), "Should expose generated providers");
        assert!(
            providers.contains(&"gemini") || providers.contains(&"openai"),
            "Should have at least one major provider"
        );
    }

    #[test]
    fn validates_known_model() {
        let result = validate_model_exists("google", "gemini-3-flash-preview");
        assert!(
            result.is_ok(),
            "Should validate gemini-3-flash-preview for google provider"
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
        let result = effective_model_context_window("google", "gemini-3-flash-preview");
        assert!(result.is_ok(), "Should get context window");

        let context = result.unwrap();
        assert!(
            context.is_some() && context.unwrap() > 0,
            "Should have positive context window"
        );
    }

    #[test]
    fn anthropic_46_uses_effective_context_window() {
        let result = effective_model_context_window("anthropic", "claude-sonnet-4-6");
        assert_eq!(result.unwrap(), Some(1_000_000));
    }

    #[test]
    fn validation_result_collects_errors() {
        let mut result = ValidationResult::new();
        assert!(result.is_valid);

        result.add_error("Error 1".to_owned());
        assert!(!result.is_valid);

        result.add_error("Error 2".to_owned());
        assert_eq!(result.errors.len(), 2);
    }

    #[test]
    fn validation_result_collects_warnings() {
        let mut result = ValidationResult::new();
        result.add_warning("Warning 1".to_owned());
        assert_eq!(result.warnings.len(), 1);
        assert!(result.is_valid); // Warnings don't invalidate
    }
}
