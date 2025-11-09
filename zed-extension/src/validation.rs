/// Configuration Validation
///
/// Validates VTCode configuration with detailed error reporting.
use crate::config::{AiConfig, Config, SecurityConfig, WorkspaceConfig};

/// Validation result with detailed errors
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether validation succeeded
    pub valid: bool,
    /// Validation errors (if any)
    pub errors: Vec<ValidationError>,
    /// Validation warnings (non-critical issues)
    pub warnings: Vec<String>,
}

impl ValidationResult {
    /// Create a successful validation result
    pub fn ok() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Create a failed validation result
    pub fn failed(errors: Vec<ValidationError>) -> Self {
        Self {
            valid: false,
            errors,
            warnings: Vec::new(),
        }
    }

    /// Add a warning
    pub fn with_warning(mut self, warning: String) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Add multiple warnings
    pub fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings.extend(warnings);
        self
    }

    /// Get error count
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    /// Get warning count
    pub fn warning_count(&self) -> usize {
        self.warnings.len()
    }

    /// Format validation results as a string
    pub fn format(&self) -> String {
        let mut output = String::new();

        if self.valid {
            output.push_str("✓ Configuration is valid\n");
        } else {
            output.push_str("✗ Configuration validation failed\n\n");
            output.push_str("Errors:\n");
            for error in &self.errors {
                output.push_str(&format!("  - {}: {}\n", error.field, error.message));
            }
        }

        if !self.warnings.is_empty() {
            output.push_str("\nWarnings:\n");
            for warning in &self.warnings {
                output.push_str(&format!("  - {}\n", warning));
            }
        }

        output
    }
}

/// A single validation error
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Field that failed validation
    pub field: String,
    /// Error message
    pub message: String,
    /// Suggested fix (if available)
    pub suggestion: Option<String>,
}

impl ValidationError {
    /// Create a new validation error
    pub fn new(field: String, message: String) -> Self {
        Self {
            field,
            message,
            suggestion: None,
        }
    }

    /// Add a suggestion
    pub fn with_suggestion(mut self, suggestion: String) -> Self {
        self.suggestion = Some(suggestion);
        self
    }

    /// Format as a readable string
    pub fn format(&self) -> String {
        let mut output = format!("{}: {}", self.field, self.message);
        if let Some(ref suggestion) = self.suggestion {
            output.push_str(&format!("\n  Suggestion: {}", suggestion));
        }
        output
    }
}

/// Validates the entire configuration
pub fn validate_config(config: &Config) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Validate AI config
    if let Err(ai_errors) = validate_ai_config(&config.ai) {
        errors.extend(ai_errors);
    }

    // Validate workspace config
    if let Err(ws_warnings) = validate_workspace_config(&config.workspace) {
        warnings.extend(ws_warnings);
    }

    // Validate security config
    if let Err(sec_errors) = validate_security_config(&config.security) {
        errors.extend(sec_errors);
    }

    if errors.is_empty() {
        if warnings.is_empty() {
            ValidationResult::ok()
        } else {
            ValidationResult::ok().with_warnings(warnings)
        }
    } else {
        ValidationResult::failed(errors).with_warnings(warnings)
    }
}

/// Validates AI configuration
fn validate_ai_config(config: &AiConfig) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    if config.provider.is_empty() {
        errors.push(
            ValidationError::new(
                "ai.provider".to_string(),
                "Provider cannot be empty".to_string(),
            )
            .with_suggestion("Set to 'anthropic', 'openai', or 'local'".to_string()),
        );
    } else if !["anthropic", "openai", "local"].contains(&config.provider.as_str()) {
        errors.push(
            ValidationError::new(
                "ai.provider".to_string(),
                format!("Unknown provider: {}", config.provider),
            )
            .with_suggestion("Use 'anthropic', 'openai', or 'local'".to_string()),
        );
    }

    if config.model.is_empty() {
        errors.push(
            ValidationError::new("ai.model".to_string(), "Model cannot be empty".to_string())
                .with_suggestion("Specify a valid model ID for the provider".to_string()),
        );
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validates workspace configuration
fn validate_workspace_config(config: &WorkspaceConfig) -> Result<(), Vec<String>> {
    let mut warnings = Vec::new();

    if config.max_context_tokens == 0 {
        warnings.push("max_context_tokens is 0, no context will be sent".to_string());
    }

    if config.max_context_tokens > 100000 {
        warnings.push(format!(
            "max_context_tokens is very high ({}), may cause performance issues",
            config.max_context_tokens
        ));
    }

    if warnings.is_empty() {
        Ok(())
    } else {
        Err(warnings)
    }
}

/// Validates security configuration
fn validate_security_config(config: &SecurityConfig) -> Result<(), Vec<ValidationError>> {
    // If human_in_the_loop is false but no allowed_tools are specified, warn
    if !config.human_in_the_loop && config.allowed_tools.is_empty() {
        // This is more of a warning, not an error
        // Can be handled by caller
    }

    // No errors in security config for now
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_result_ok() {
        let result = ValidationResult::ok();
        assert!(result.valid);
        assert_eq!(result.error_count(), 0);
        assert_eq!(result.warning_count(), 0);
    }

    #[test]
    fn test_validation_result_with_errors() {
        let error = ValidationError::new("field".to_string(), "error message".to_string());
        let result = ValidationResult::failed(vec![error]);
        assert!(!result.valid);
        assert_eq!(result.error_count(), 1);
    }

    #[test]
    fn test_validation_result_with_warnings() {
        let result = ValidationResult::ok().with_warning("warning 1".to_string());
        assert!(result.valid);
        assert_eq!(result.warning_count(), 1);
    }

    #[test]
    fn test_validation_error_format() {
        let error = ValidationError::new("model".to_string(), "Invalid model".to_string())
            .with_suggestion("Use a valid model ID".to_string());

        let formatted = error.format();
        assert!(formatted.contains("model"));
        assert!(formatted.contains("Invalid model"));
        assert!(formatted.contains("Use a valid model ID"));
    }

    #[test]
    fn test_validate_config_success() {
        let config = Config::default();
        let result = validate_config(&config);
        assert!(result.valid);
    }

    #[test]
    fn test_validate_ai_config_invalid_provider() {
        let config = AiConfig {
            provider: "unknown".to_string(),
            model: "some-model".to_string(),
        };
        let result = validate_ai_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_ai_config_empty_model() {
        let config = AiConfig {
            provider: "anthropic".to_string(),
            model: String::new(),
        };
        let result = validate_ai_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_workspace_config_zero_tokens() {
        let config = WorkspaceConfig {
            analyze_on_startup: false,
            max_context_tokens: 0,
            ignore_patterns: vec![],
        };
        let result = validate_workspace_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_workspace_config_high_tokens() {
        let config = WorkspaceConfig {
            analyze_on_startup: false,
            max_context_tokens: 200000,
            ignore_patterns: vec![],
        };
        let result = validate_workspace_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validation_result_format() {
        let result = ValidationResult::ok().with_warning("test warning".to_string());
        let formatted = result.format();
        assert!(formatted.contains("✓ Configuration is valid"));
        assert!(formatted.contains("Warnings"));
        assert!(formatted.contains("test warning"));
    }

    #[test]
    fn test_validation_result_failed_format() {
        let error = ValidationError::new("field".to_string(), "test error".to_string());
        let result = ValidationResult::failed(vec![error]);
        let formatted = result.format();
        assert!(formatted.contains("✗ Configuration validation failed"));
        assert!(formatted.contains("Errors"));
        assert!(formatted.contains("test error"));
    }
}
