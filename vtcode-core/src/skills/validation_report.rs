//! Comprehensive validation report for Agent Skills
//!
//! Provides detailed validation feedback with multiple error levels and suggestions.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Severity level for validation issues
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationLevel {
    /// Critical error that prevents skill from working
    Error,
    /// Warning that may cause issues but doesn't prevent usage
    Warning,
    /// Suggestion for improvement
    Info,
}

/// Single validation issue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub level: ValidationLevel,
    pub field: Option<String>,
    pub message: String,
    pub suggestion: Option<String>,
}

/// Comprehensive skill validation report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillValidationReport {
    pub skill_name: String,
    pub skill_path: PathBuf,
    pub is_valid: bool,
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
    pub suggestions: Vec<ValidationIssue>,
    pub stats: ValidationStats,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationStats {
    pub total_issues: usize,
    pub error_count: usize,
    pub warning_count: usize,
    pub suggestion_count: usize,
    pub token_estimate: usize,
}

impl SkillValidationReport {
    pub fn new(skill_name: String, skill_path: PathBuf) -> Self {
        Self {
            skill_name,
            skill_path,
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            suggestions: Vec::new(),
            stats: ValidationStats {
                total_issues: 0,
                error_count: 0,
                warning_count: 0,
                suggestion_count: 0,
                token_estimate: 0,
            },
        }
    }

    pub fn add_error(
        &mut self,
        field: Option<String>,
        message: String,
        suggestion: Option<String>,
    ) {
        self.errors.push(ValidationIssue {
            level: ValidationLevel::Error,
            field,
            message,
            suggestion,
        });
        self.is_valid = false;
    }

    pub fn add_warning(
        &mut self,
        field: Option<String>,
        message: String,
        suggestion: Option<String>,
    ) {
        self.warnings.push(ValidationIssue {
            level: ValidationLevel::Warning,
            field,
            message,
            suggestion,
        });
    }

    pub fn add_suggestion(&mut self, field: Option<String>, message: String) {
        self.suggestions.push(ValidationIssue {
            level: ValidationLevel::Info,
            field,
            message,
            suggestion: None,
        });
    }

    pub fn finalize(&mut self) {
        self.stats.error_count = self.errors.len();
        self.stats.warning_count = self.warnings.len();
        self.stats.suggestion_count = self.suggestions.len();
        self.stats.total_issues =
            self.stats.error_count + self.stats.warning_count + self.stats.suggestion_count;
    }

    pub fn generate_summary(&self) -> String {
        let mut summary = String::new();

        summary.push_str(&format!("Skill: {}\n", self.skill_name));
        summary.push_str(&format!("Path: {}\n", self.skill_path.display()));
        summary.push_str(&format!(
            "Status: {}\n",
            if self.is_valid {
                "✅ Valid"
            } else {
                "❌ Invalid"
            }
        ));
        summary.push_str("\nIssues found:\n");
        summary.push_str(&format!("  Errors: {}\n", self.stats.error_count));
        summary.push_str(&format!("  Warnings: {}\n", self.stats.warning_count));
        summary.push_str(&format!("  Suggestions: {}\n", self.stats.suggestion_count));

        if !self.errors.is_empty() {
            summary.push_str("\n❌ Errors:\n");
            for error in &self.errors {
                summary.push_str(&format!("  - {}", error.message));
                if let Some(field) = &error.field {
                    summary.push_str(&format!(" [{}]", field));
                }
                summary.push('\n');
                if let Some(suggestion) = &error.suggestion {
                    summary.push_str(&format!("    💡 Suggestion: {}\n", suggestion));
                }
            }
        }

        if !self.warnings.is_empty() {
            summary.push_str("\n⚠️  Warnings:\n");
            for warning in &self.warnings {
                summary.push_str(&format!("  - {}", warning.message));
                if let Some(field) = &warning.field {
                    summary.push_str(&format!(" [{}]", field));
                }
                summary.push('\n');
            }
        }

        if !self.suggestions.is_empty() {
            summary.push_str("\n💡 Suggestions:\n");
            for suggestion in &self.suggestions {
                summary.push_str(&format!("  - {}", suggestion.message));
                if let Some(field) = &suggestion.field {
                    summary.push_str(&format!(" [{}]", field));
                }
                summary.push('\n');
            }
        }

        summary
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_validation_report() {
        let mut report =
            SkillValidationReport::new("test-skill".to_string(), PathBuf::from("/tmp/test-skill"));

        report.add_error(
            Some("name".to_string()),
            "Invalid characters in name".to_string(),
            Some("Use only lowercase letters, numbers, and hyphens".to_string()),
        );

        report.add_warning(
            None,
            "Description is very short".to_string(),
            Some("Consider adding more detail about when to use this skill".to_string()),
        );

        report.finalize();

        assert!(!report.is_valid);
        assert_eq!(report.stats.error_count, 1);
        assert_eq!(report.stats.warning_count, 1);
        assert!(report.generate_summary().contains("Invalid characters"));
    }
}
