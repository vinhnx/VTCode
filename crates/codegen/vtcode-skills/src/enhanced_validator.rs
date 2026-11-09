//! Enhanced skill validator with comprehensive error collection
//!
//! Validates skills against Agent Skills specification and collects all issues
//! instead of failing on the first error.

use crate::file_references::FileReferenceValidator;
use crate::limits::{
    MAX_ALLOWED_TOOLS, MAX_COMPATIBILITY_CHARS, MAX_LICENSE_CHARS, MAX_SKILL_DESCRIPTION_CHARS, MAX_SKILL_NAME_CHARS,
    allowed_tool_count,
};
use crate::types::{SkillManifest, resolve_skill_dir};
use crate::validation_report::SkillValidationReport;
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
    pub fn validate_manifest(&self, manifest: &SkillManifest, skill_path: &Path) -> SkillValidationReport {
        let mut report = SkillValidationReport::new(manifest.name.clone(), skill_path.to_path_buf());

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
            report.add_error(Some("name".to_string()), "name is required and must not be empty".to_string(), None);
            return;
        }

        // Check length
        if manifest.name.len() > MAX_SKILL_NAME_CHARS {
            report.add_error(
                Some("name".to_string()),
                format!("name exceeds maximum length: {} characters (max {MAX_SKILL_NAME_CHARS})", manifest.name.len()),
                Some(format!("Use a shorter name (1-{MAX_SKILL_NAME_CHARS} characters)")),
            );
        }

        // Check for valid characters
        if !manifest.name.chars().all(|c| c.is_lowercase() || c.is_numeric() || c == '-') {
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
                format!("name contains reserved word: '{}'\nMust not contain 'anthropic' or 'claude'", manifest.name),
                Some("Choose a different name without these words".to_string()),
            );
        }
    }

    /// Validate description field
    fn validate_description_field(&self, manifest: &SkillManifest, report: &mut SkillValidationReport) {
        if manifest.description.is_empty() {
            report.add_error(
                Some("description".to_string()),
                "description is required and must not be empty".to_string(),
                Some("Add a description explaining what the skill does and when to use it".to_string()),
            );
            return;
        }

        if manifest.description.len() > MAX_SKILL_DESCRIPTION_CHARS {
            report.add_error(
                Some("description".to_string()),
                format!(
                    "description exceeds maximum length: {} characters (max {MAX_SKILL_DESCRIPTION_CHARS})",
                    manifest.description.len()
                ),
                Some(format!("Shorten the description to {MAX_SKILL_DESCRIPTION_CHARS} characters or less")),
            );
        }

        // Suggest longer description if too short
        if manifest.description.len() < 50 {
            report.add_suggestion(Some("description".to_string()), "Description is very short".to_string());
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
                Some("Rename the skill directory to match the name field, or rename the skill".to_string()),
            );
        }
    }

    /// Validate all optional fields
    fn validate_optional_fields(&self, manifest: &SkillManifest, report: &mut SkillValidationReport) {
        // Validate allowed-tools field
        if let Some(allowed_tools) = &manifest.allowed_tools {
            let tool_count = allowed_tool_count(allowed_tools);

            if tool_count > MAX_ALLOWED_TOOLS {
                report.add_error(
                    Some("allowed-tools".to_string()),
                    format!("allowed-tools exceeds maximum tool count: {tool_count} tools (max {MAX_ALLOWED_TOOLS})"),
                    Some(format!("Reduce the number of tools to {MAX_ALLOWED_TOOLS} or fewer")),
                );
            }

            if tool_count == 0 {
                report.add_error(
                    Some("allowed-tools".to_string()),
                    "allowed-tools must not be empty if specified".to_string(),
                    Some("Either remove the field or add valid tool names".to_string()),
                );
            }
        }

        // Validate license field
        if let Some(license) = &manifest.license
            && license.len() > MAX_LICENSE_CHARS
        {
            report.add_error(
                Some("license".to_string()),
                format!("license exceeds maximum length: {} characters (max {MAX_LICENSE_CHARS})", license.len()),
                Some("Shorten the license field".to_string()),
            );
        }

        // Validate compatibility field
        if let Some(compatibility) = &manifest.compatibility {
            if compatibility.is_empty() {
                report.add_error(
                    Some("compatibility".to_string()),
                    "compatibility must not be empty if specified".to_string(),
                    Some("Either remove the field or add meaningful compatibility info".to_string()),
                );
            } else if compatibility.len() > MAX_COMPATIBILITY_CHARS {
                report.add_error(
                    Some("compatibility".to_string()),
                    format!(
                        "compatibility exceeds maximum length: {} characters (max {MAX_COMPATIBILITY_CHARS})",
                        compatibility.len()
                    ),
                    Some("Shorten the compatibility field".to_string()),
                );
            }
        }

        // Suggest adding optional fields if missing
        if manifest.license.is_none() {
            report.add_suggestion(Some("license".to_string()), "Consider adding a license field".to_string());
        }

        if manifest.compatibility.is_none() {
            report.add_suggestion(
                Some("compatibility".to_string()),
                "Consider adding a compatibility field if the skill has specific requirements".to_string(),
            );
        }

        if !self.description_has_routing_signals(&manifest.description) {
            self.report_routing_quality_issue(
                report,
                "description",
                "Description lacks concrete routing signals (inputs/triggers/outputs)",
                "Rewrite description as routing logic: when to use, when not to use, expected result.",
            );
        }
    }

    fn report_routing_quality_issue(
        &self,
        report: &mut SkillValidationReport,
        field: &str,
        message: &str,
        remediation: &str,
    ) {
        if self.strict_mode {
            report.add_error(Some(field.to_string()), message.to_string(), Some(remediation.to_string()));
        } else {
            report.add_warning(Some(field.to_string()), message.to_string(), Some(remediation.to_string()));
        }
    }

    fn description_has_routing_signals(&self, description: &str) -> bool {
        let text = description.to_lowercase();
        let has_trigger_hint =
            text.contains("when") || text.contains("if ") || text.contains("for ") || text.contains("trigger");
        let has_output_hint = text.contains("output")
            || text.contains("returns")
            || text.contains("result")
            || text.contains("generat")
            || text.contains("produ");
        let has_action_hint = text.contains("analy")
            || text.contains("extract")
            || text.contains("transform")
            || text.contains("validate")
            || text.contains("summar")
            || text.contains("convert")
            || text.contains("clean");

        has_action_hint && (has_trigger_hint || has_output_hint)
    }

    /// Validate instructions length (suggest keeping under 500 lines)
    fn validate_instructions_length(&self, _manifest: &SkillManifest, report: &mut SkillValidationReport) {
        // This is a suggestion based on the spec recommendation
        report.add_suggestion(None, "Keep SKILL.md under 500 lines for optimal context usage".to_string());
    }

    /// Validate file references in instructions
    pub fn validate_file_references(
        &self,
        _manifest: &SkillManifest,
        skill_path: &Path,
        instructions: &str,
        report: &mut SkillValidationReport,
    ) {
        let skill_root = resolve_skill_dir(skill_path);
        let validator = FileReferenceValidator::new(skill_root.to_path_buf());
        let reference_errors = validator.validate_references(instructions);

        for error in reference_errors {
            // In strict mode, treat reference errors as errors, otherwise warnings
            if self.strict_mode {
                report.add_error(
                    None,
                    format!("File reference issue: {error}"),
                    Some("Fix the file reference or ensure the referenced file exists".to_string()),
                );
            } else {
                report.add_warning(
                    None,
                    format!("File reference issue: {error}"),
                    Some("Fix the file reference or ensure the referenced file exists".to_string()),
                );
            }
        }

        // List valid references as info
        let valid_refs = validator.list_valid_references();
        if !valid_refs.is_empty() {
            let ref_list: Vec<String> = valid_refs.iter().map(|p| p.to_string_lossy().to_string()).collect();
            report.add_suggestion(
                None,
                format!("Found {} valid file references: {}", ref_list.len(), ref_list.join(", ")),
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
    use tempfile::TempDir;

    #[test]
    fn test_comprehensive_validation() {
        let validator = ComprehensiveSkillValidator::new();
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "A test skill for validation".to_string(),
            version: Some("1.0.0".to_string()),
            author: Some("Test Author".to_string()),
            allowed_tools: Some("Read Write Bash".to_string()),
            compatibility: Some("Designed for VT Code".to_string()),
            ..Default::default()
        };

        // Note: We can't easily test directory validation without creating temp dirs
        // So we'll test with a non-existent path which should generate warnings
        let report = validator.validate_manifest(&manifest, PathBuf::from("/tmp/nonexistent").as_path());

        // Should have some suggestions for missing fields
        assert!(report.suggestions.iter().any(|s| s.field == Some("license".to_string())));
    }

    /// The comprehensive validator reports the same limit the strict one
    /// enforces; keep both ends of the boundary in one place so a future edit
    /// cannot make the two validators disagree. Asserted through `to_json`
    /// because issue fields are private to `validation_report`.
    #[test]
    fn reports_allowed_tools_limit_at_the_same_boundary() {
        let validator = ComprehensiveSkillValidator::new();
        let report_for = |count: usize| {
            let tools = (0..count).map(|i| format!("Tool{i}")).collect::<Vec<_>>().join(" ");
            let manifest = SkillManifest {
                name: "test-skill".to_string(),
                description: "A test skill for validation".to_string(),
                allowed_tools: Some(tools),
                ..Default::default()
            };
            validator
                .validate_manifest(&manifest, PathBuf::from("/tmp/nonexistent").as_path())
                .to_json()
                .expect("report serializes")
        };

        let over_limit = report_for(MAX_ALLOWED_TOOLS + 1);
        assert!(
            over_limit.contains("allowed-tools exceeds maximum tool count"),
            "expected an allowed-tools error, got {over_limit}"
        );

        let at_limit = report_for(MAX_ALLOWED_TOOLS);
        assert!(
            !at_limit.contains("allowed-tools"),
            "{} tools is the inclusive limit and must not error, got {at_limit}",
            MAX_ALLOWED_TOOLS
        );
    }

    #[test]
    fn directory_name_check_uses_skill_directory_not_its_parent() {
        // Regression: validating with the skill directory (as the CLI does) must
        // compare against the skill's own directory name, not its grandparent
        // (e.g. "skills"), which produced a false "does not match directory
        // name 'skills'" warning for every skill.
        let temp = TempDir::new().expect("temp dir");
        let skill_dir = temp.path().join("skills").join("my-skill");
        std::fs::create_dir_all(&skill_dir).expect("create skill dir");

        let manifest = SkillManifest {
            name: "my-skill".to_string(),
            description: "A test skill for validation".to_string(),
            ..Default::default()
        };

        let validator = ComprehensiveSkillValidator::new();
        let report = validator.validate_manifest(&manifest, &skill_dir);

        assert!(
            !report.generate_summary().contains("does not match directory name"),
            "matching skill directory must not warn: {}",
            report.generate_summary()
        );
    }

    #[test]
    fn directory_name_check_accepts_skill_md_path() {
        // The same resolver must also accept the `SKILL.md` path (the shape
        // `parse_skill_file` uses) and still compare against the skill directory.
        let temp = TempDir::new().expect("temp dir");
        let skill_dir = temp.path().join("skills").join("my-skill");
        std::fs::create_dir_all(&skill_dir).expect("create skill dir");
        std::fs::write(skill_dir.join("SKILL.md"), "# Body").expect("write SKILL.md");

        let manifest = SkillManifest {
            name: "my-skill".to_string(),
            description: "A test skill for validation".to_string(),
            ..Default::default()
        };

        let validator = ComprehensiveSkillValidator::new();
        let report = validator.validate_manifest(&manifest, &skill_dir.join("SKILL.md"));

        assert!(
            !report.generate_summary().contains("does not match directory name"),
            "SKILL.md path must resolve to the skill directory: {}",
            report.generate_summary()
        );
    }

    #[test]
    fn file_references_resolve_against_skill_directory() {
        // Regression: file references must be validated relative to the skill
        // directory, not its parent, so an existing bundled resource resolves.
        let temp = TempDir::new().expect("temp dir");
        let skill_dir = temp.path().join("skills").join("ref-skill");
        std::fs::create_dir_all(skill_dir.join("references")).expect("create references dir");
        std::fs::write(skill_dir.join("references").join("guide.md"), "# Guide").expect("write reference");

        let manifest = SkillManifest {
            name: "ref-skill".to_string(),
            description: "Reference skill".to_string(),
            ..Default::default()
        };
        let instructions = "See references/guide.md for details.";

        let validator = ComprehensiveSkillValidator::new();
        let mut report = validator.validate_manifest(&manifest, &skill_dir);
        validator.validate_file_references(&manifest, &skill_dir, instructions, &mut report);
        report.finalize();

        assert!(
            !report.generate_summary().contains("File reference issue"),
            "existing reference must resolve against the skill directory: {}",
            report.generate_summary()
        );
    }

    #[test]
    fn file_references_report_missing_resource() {
        // Asymmetric control: a genuinely missing resource is still flagged, so
        // the root fix does not simply suppress all reference diagnostics.
        let temp = TempDir::new().expect("temp dir");
        let skill_dir = temp.path().join("skills").join("ref-skill");
        std::fs::create_dir_all(&skill_dir).expect("create skill dir");

        let manifest = SkillManifest {
            name: "ref-skill".to_string(),
            description: "Reference skill".to_string(),
            ..Default::default()
        };
        let instructions = "See references/missing.md for details.";

        let validator = ComprehensiveSkillValidator::new();
        let mut report = validator.validate_manifest(&manifest, &skill_dir);
        validator.validate_file_references(&manifest, &skill_dir, instructions, &mut report);
        report.finalize();

        assert!(
            report.generate_summary().contains("File reference issue"),
            "missing reference must still be flagged: {}",
            report.generate_summary()
        );
    }
}
