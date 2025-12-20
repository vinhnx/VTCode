//! Enhanced skill validator with comprehensive error collection
//!
//! Validates skills against Agent Skills specification and collects all issues
//! instead of failing on the first error.

use crate::skills::file_references::FileReferenceValidator;
use crate::skills::types::SkillManifest;
use crate::skills::validation_report::SkillValidationReport;
use std::path::Path;

/// Enhanced validator that collects all validation issues
pub struct ComprehensiveSkillValidator {
    strict_mode: bool,
}

impl ComprehensiveSkillValidator {
    pub fn new() -> Self {
        Self { strict_mode: false }
    }

    pub fn strict() -> Self {
        Self { strict_mode: true }
    }

    /// Validate a skill manifest comprehensively
    pub fn validate_manifest(
        &self,
        manifest: &SkillManifest,
        skill_path: &Path,
    ) -> SkillValidationReport {
        let mut report =
            SkillValidationReport::new(manifest.name.clone(), skill_path.to_path_buf());

        // Validate name field
        self.validate_name_field(manifest, &mut report);

        // Validate description field
        self.validate_description_field(manifest, &mut report);

        // Validate directory name match
        self.validate_directory_match(manifest, skill_path, &mut report);

        // Validate optional fields
        self.validate_optional_fields(manifest, &mut report);

        // Validate instructions length
        self.validate_instructions_length(manifest, &mut report);

        report.finalize();
        report
    }

    /// Validate name field with all checks
    fn validate_name_field(&self, manifest: &SkillManifest, report: &mut SkillValidationReport) {
        // Check empty
        if manifest.name.is_empty() {
            report.add_error(
                Some("name".to_string()),
                "name is required and must not be empty".to_string(),
                None,
            );
            return;
        }

        // Check length
        if manifest.name.len() > 64 {
            report.add_error(
                Some("name".to_string()),
                format!(
                    "name exceeds maximum length: {} characters (max 64)",
                    manifest.name.len()
                ),
                Some("Use a shorter name (1-64 characters)".to_string()),
            );
        }

        // Check for valid characters
        if !manifest
            .name
            .chars()
            .all(|c| c.is_lowercase() || c.is_numeric() || c == '-')
        {
            report.add_error(
                Some("name".to_string()),
                format!(
                    "name contains invalid characters: '{}'\nMust contain only lowercase letters, numbers, and hyphens",
                    manifest.name
                ),
                Some("Use only a-z, 0-9, and hyphens".to_string()),
            );
        }

        // Check consecutive hyphens
        if manifest.name.contains("--") {
            report.add_error(
                Some("name".to_string()),
                format!("name contains consecutive hyphens: '{}'", manifest.name),
                Some("Remove consecutive hyphens (--)".to_string()),
            );
        }

        // Check leading hyphen
        if manifest.name.starts_with('-') {
            report.add_error(
                Some("name".to_string()),
                format!("name starts with hyphen: '{}'", manifest.name),
                Some("Remove the leading hyphen".to_string()),
            );
        }

        // Check trailing hyphen
        if manifest.name.ends_with('-') {
            report.add_error(
                Some("name".to_string()),
                format!("name ends with hyphen: '{}'", manifest.name),
                Some("Remove the trailing hyphen".to_string()),
            );
        }

        // Check reserved words
        if manifest.name.contains("anthropic") || manifest.name.contains("claude") {
            report.add_error(
                Some("name".to_string()),
                format!(
                    "name contains reserved word: '{}'\nMust not contain 'anthropic' or 'claude'",
                    manifest.name
                ),
                Some("Choose a different name without these words".to_string()),
            );
        }
    }

    /// Validate description field
    fn validate_description_field(
        &self,
        manifest: &SkillManifest,
        report: &mut SkillValidationReport,
    ) {
        if manifest.description.is_empty() {
            report.add_error(
                Some("description".to_string()),
                "description is required and must not be empty".to_string(),
                Some(
                    "Add a description explaining what the skill does and when to use it"
                        .to_string(),
                ),
            );
            return;
        }

        if manifest.description.len() > 1024 {
            report.add_error(
                Some("description".to_string()),
                format!(
                    "description exceeds maximum length: {} characters (max 1024)",
                    manifest.description.len()
                ),
                Some("Shorten the description to 1024 characters or less".to_string()),
            );
        }

        // Suggest longer description if too short
        if manifest.description.len() < 50 {
            report.add_suggestion(
                Some("description".to_string()),
                "Description is very short".to_string(),
            );
        }
    }

    /// Validate directory name matches skill name
    fn validate_directory_match(
        &self,
        manifest: &SkillManifest,
        skill_path: &Path,
        report: &mut SkillValidationReport,
    ) {
        if let Err(e) = manifest.validate_directory_name_match(skill_path) {
            report.add_warning(
                Some("name".to_string()),
                e.to_string(),
                Some(
                    "Rename the skill directory to match the name field, or rename the skill"
                        .to_string(),
                ),
            );
        }
    }

    /// Validate all optional fields
    fn validate_optional_fields(
        &self,
        manifest: &SkillManifest,
        report: &mut SkillValidationReport,
    ) {
        // Check for conflicting container flags
        if let (Some(true), Some(true)) = (manifest.requires_container, manifest.disallow_container)
        {
            report.add_error(
                None,
                "Skill manifest cannot set both requires-container and disallow-container"
                    .to_string(),
                Some(
                    "Choose either requires-container or disallow-container, not both".to_string(),
                ),
            );
        }

        // Validate when-to-use field
        if let Some(when_to_use) = &manifest.when_to_use
            && when_to_use.len() > 512
        {
            report.add_error(
                Some("when-to-use".to_string()),
                format!(
                    "when-to-use exceeds maximum length: {} characters (max 512)",
                    when_to_use.len()
                ),
                Some("Shorten the when-to-use field to 512 characters or less".to_string()),
            );
        }

        // Validate allowed-tools field
        if let Some(allowed_tools) = &manifest.allowed_tools {
            let tools: Vec<&str> = allowed_tools.split_whitespace().collect();

            if tools.len() > 16 {
                report.add_error(
                    Some("allowed-tools".to_string()),
                    format!(
                        "allowed-tools exceeds maximum tool count: {} tools (max 16)",
                        tools.len()
                    ),
                    Some("Reduce the number of tools to 16 or fewer".to_string()),
                );
            }

            if tools.is_empty() {
                report.add_error(
                    Some("allowed-tools".to_string()),
                    "allowed-tools must not be empty if specified".to_string(),
                    Some("Either remove the field or add valid tool names".to_string()),
                );
            }
        }

        // Validate license field
        if let Some(license) = &manifest.license
            && license.len() > 512
        {
            report.add_error(
                Some("license".to_string()),
                format!(
                    "license exceeds maximum length: {} characters (max 512)",
                    license.len()
                ),
                Some("Shorten the license field".to_string()),
            );
        }

        // Validate model field
        if let Some(model) = &manifest.model
            && model.len() > 128
        {
            report.add_error(
                Some("model".to_string()),
                format!(
                    "model exceeds maximum length: {} characters (max 128)",
                    model.len()
                ),
                Some("Shorten the model name".to_string()),
            );
        }

        // Validate compatibility field
        if let Some(compatibility) = &manifest.compatibility {
            if compatibility.is_empty() {
                report.add_error(
                    Some("compatibility".to_string()),
                    "compatibility must not be empty if specified".to_string(),
                    Some(
                        "Either remove the field or add meaningful compatibility info".to_string(),
                    ),
                );
            } else if compatibility.len() > 500 {
                report.add_error(
                    Some("compatibility".to_string()),
                    format!(
                        "compatibility exceeds maximum length: {} characters (max 500)",
                        compatibility.len()
                    ),
                    Some("Shorten the compatibility field".to_string()),
                );
            }
        }

        // Suggest adding optional fields if missing
        if manifest.license.is_none() {
            report.add_suggestion(
                Some("license".to_string()),
                "Consider adding a license field".to_string(),
            );
        }

        if manifest.compatibility.is_none() {
            report.add_suggestion(
                Some("compatibility".to_string()),
                "Consider adding a compatibility field if the skill has specific requirements"
                    .to_string(),
            );
        }
    }

    /// Validate instructions length (suggest keeping under 500 lines)
    fn validate_instructions_length(
        &self,
        _manifest: &SkillManifest,
        report: &mut SkillValidationReport,
    ) {
        // This is a suggestion based on the spec recommendation
        report.add_suggestion(
            None,
            "Keep SKILL.md under 500 lines for optimal context usage".to_string(),
        );
    }

    /// Validate file references in instructions
    pub fn validate_file_references(
        &self,
        _manifest: &SkillManifest,
        skill_path: &Path,
        instructions: &str,
        report: &mut SkillValidationReport,
    ) {
        let skill_root = skill_path.parent().unwrap_or(skill_path);
        let validator = FileReferenceValidator::new(skill_root.to_path_buf());
        let reference_errors = validator.validate_references(instructions);

        for error in reference_errors {
            // In strict mode, treat reference errors as errors, otherwise warnings
            if self.strict_mode {
                report.add_error(
                    None,
                    format!("File reference issue: {}", error),
                    Some("Fix the file reference or ensure the referenced file exists".to_string()),
                );
            } else {
                report.add_warning(
                    None,
                    format!("File reference issue: {}", error),
                    Some("Fix the file reference or ensure the referenced file exists".to_string()),
                );
            }
        }

        // List valid references as info
        let valid_refs = validator.list_valid_references();
        if !valid_refs.is_empty() {
            let ref_list: Vec<String> = valid_refs
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            report.add_suggestion(
                None,
                format!(
                    "Found {} valid file references: {}",
                    ref_list.len(),
                    ref_list.join(", ")
                ),
            );
        }
    }
}

impl Default for ComprehensiveSkillValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_comprehensive_validation() {
        let validator = ComprehensiveSkillValidator::new();
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "A test skill for validation".to_string(),
            version: Some("1.0.0".to_string()),
            author: Some("Test Author".to_string()),
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: Some("Read Write Bash".to_string()),
            disable_model_invocation: None,
            when_to_use: None,
            requires_container: None,
            disallow_container: None,
            compatibility: Some("Designed for VTCode".to_string()),
            metadata: None,
        };

        // Note: We can't easily test directory validation without creating temp dirs
        // So we'll test with a non-existent path which should generate warnings
        let report =
            validator.validate_manifest(&manifest, PathBuf::from("/tmp/nonexistent").as_path());

        // Should have some suggestions for missing fields
        assert!(
            report
                .suggestions
                .iter()
                .any(|s| s.field == Some("license".to_string()))
        );
    }
}
