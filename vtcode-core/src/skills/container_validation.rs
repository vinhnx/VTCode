//! Container Skills Validation
//!
//! Detects and validates skills that require Anthropic's container skills feature,
//! which is not supported in VTCode. Provides early warnings and filtering
//! to prevent false positives where skills load but cannot execute properly.

use crate::skills::types::Skill;
use serde::{Deserialize, Serialize};

/// Container skills requirement detection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ContainerSkillsRequirement {
    /// Skill requires container skills (not supported in VTCode)
    Required,
    /// Skill provides fallback alternatives
    RequiredWithFallback,
    /// Skill does not require container skills
    NotRequired,
    /// Cannot determine requirement (default to safe)
    Unknown,
}

/// Container skills validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerValidationResult {
    /// Whether container skills are required
    pub requirement: ContainerSkillsRequirement,
    /// Detailed analysis
    pub analysis: String,
    /// Specific patterns found
    pub patterns_found: Vec<String>,
    /// Recommendations for users
    pub recommendations: Vec<String>,
    /// Whether skill should be filtered out
    pub should_filter: bool,
}

/// Detects container skills requirements in skill instructions
pub struct ContainerSkillsValidator {
    /// Patterns that indicate container skills usage
    container_patterns: Vec<String>,
    /// Patterns that indicate fallback alternatives
    fallback_patterns: Vec<String>,
    /// Patterns that indicate VTCode incompatibility
    incompatibility_patterns: Vec<String>,
}

impl Default for ContainerSkillsValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerSkillsValidator {
    /// Create a new container skills validator
    pub fn new() -> Self {
        Self {
            container_patterns: vec![
                "container={".to_string(),
                "container.skills".to_string(),
                "betas=[\"skills-".to_string(),
                "anthropic".to_string(),
                "xlsx".to_string(),
                "pdf".to_string(),
                "docx".to_string(),
                "pptx".to_string(),
            ],
            fallback_patterns: vec![
                "vtcode does not currently support".to_string(),
                "use execute_code".to_string(),
                "openpyxl".to_string(),
                "reportlab".to_string(),
                "python-docx".to_string(),
            ],
            incompatibility_patterns: vec![
                "vtcode does not currently support".to_string(),
                "requires Anthropic's container skills".to_string(),
            ],
        }
    }

    /// Analyze a skill for container skills requirements
    pub fn analyze_skill(&self, skill: &Skill) -> ContainerValidationResult {
        // Check if skill uses VT Code native features (not container skills)
        if let Some(true) = skill.manifest.vtcode_native {
            return ContainerValidationResult {
                requirement: ContainerSkillsRequirement::NotRequired,
                analysis: "Skill uses VT Code native features (not container skills)".to_string(),
                patterns_found: vec![],
                recommendations: vec![],
                should_filter: false,
            };
        }

        let instructions = &skill.instructions;
        let mut patterns_found = Vec::new();
        let mut recommendations = Vec::new();

        // Check for container skills patterns
        let mut has_container_usage = false;
        for pattern in &self.container_patterns {
            if instructions.contains(pattern) {
                patterns_found.push(pattern.clone());
                has_container_usage = true;
            }
        }

        // Check for explicit incompatibility statements
        let mut has_incompatibility = false;
        for pattern in &self.incompatibility_patterns {
            if instructions.contains(pattern) {
                patterns_found.push(pattern.clone());
                has_incompatibility = true;
            }
        }

        // Check for fallback alternatives
        let mut has_fallback = false;
        for pattern in &self.fallback_patterns {
            if instructions.contains(pattern) {
                patterns_found.push(pattern.clone());
                has_fallback = true;
            }
        }

        // Determine requirement level and recommendations
        let (requirement, analysis, should_filter) = if has_incompatibility {
            (
                ContainerSkillsRequirement::RequiredWithFallback,
                format!(
                    "Skill '{}' explicitly states it requires Anthropic container skills which VTCode does not support. However, it provides fallback alternatives.",
                    skill.name()
                ),
                false, // Don't filter - provide fallback guidance
            )
        } else if has_container_usage && has_fallback {
            (
                ContainerSkillsRequirement::RequiredWithFallback,
                format!(
                    "Skill '{}' uses container skills but provides VTCode-compatible alternatives.",
                    skill.name()
                ),
                false,
            )
        } else if has_container_usage {
            (
                ContainerSkillsRequirement::Required,
                format!(
                    "Skill '{}' requires Anthropic container skills which are not supported in VTCode.",
                    skill.name()
                ),
                true, // Filter out - no fallback available
            )
        } else {
            (
                ContainerSkillsRequirement::NotRequired,
                format!("Skill '{}' does not require container skills.", skill.name()),
                false,
            )
        };

        // Generate recommendations with enhanced user guidance
        if requirement == ContainerSkillsRequirement::Required {
            recommendations.push("This skill requires Anthropic's container skills feature which is not available in VTCode.".to_string());
            recommendations.push("".to_string());
            recommendations.push("Consider these VTCode-compatible alternatives:".to_string());

            // Provide specific alternatives based on skill type
            if skill.name().contains("pdf") || skill.name().contains("report") {
                recommendations.push("  1. Use execute_code with Python libraries: reportlab, fpdf2, or weasyprint".to_string());
                recommendations.push("  2. Install: pip install reportlab".to_string());
                recommendations.push("  3. Use Python code execution to generate PDFs".to_string());
            } else if skill.name().contains("spreadsheet") || skill.name().contains("excel") {
                recommendations.push("  1. Use execute_code with Python libraries: openpyxl, xlsxwriter, or pandas".to_string());
                recommendations.push("  2. Install: pip install openpyxl xlsxwriter".to_string());
                recommendations.push("  3. Use Python code execution to create spreadsheets".to_string());
            } else if skill.name().contains("doc") || skill.name().contains("word") {
                recommendations.push("  1. Use execute_code with Python libraries: python-docx or docxtpl".to_string());
                recommendations.push("  2. Install: pip install python-docx".to_string());
                recommendations.push("  3. Use Python code execution to generate documents".to_string());
            } else if skill.name().contains("presentation") || skill.name().contains("ppt") {
                recommendations.push("  1. Use execute_code with Python libraries: python-pptx".to_string());
                recommendations.push("  2. Install: pip install python-pptx".to_string());
                recommendations.push("  3. Use Python code execution to create presentations".to_string());
            } else {
                recommendations.push("  1. Use execute_code with appropriate Python libraries".to_string());
                recommendations.push("  2. Search for VTCode-compatible skills in the documentation".to_string());
            }

            recommendations.push("".to_string());
            recommendations.push("Learn more about VTCode's code execution in the documentation.".to_string());
            recommendations.push("Official Anthropic container skills documentation: https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview".to_string());
        } else if requirement == ContainerSkillsRequirement::RequiredWithFallback {
            recommendations.push("This skill uses container skills but provides VTCode-compatible alternatives.".to_string());
            recommendations.push("Use the fallback instructions in the skill documentation.".to_string());
            recommendations.push("Look for sections marked 'Option 2' or 'VTCode Alternative'.".to_string());
            recommendations.push("The skill instructions contain working examples using `execute_code`.".to_string());
        }

        ContainerValidationResult {
            requirement,
            analysis,
            patterns_found,
            recommendations,
            should_filter,
        }
    }

    /// Batch analyze multiple skills
    pub fn analyze_skills(&self, skills: &[Skill]) -> Vec<ContainerValidationResult> {
        skills.iter().map(|skill| self.analyze_skill(skill)).collect()
    }

    /// Filter skills that require container skills without fallback
    pub fn filter_incompatible_skills(&self, skills: Vec<Skill>) -> (Vec<Skill>, Vec<IncompatibleSkillInfo>) {
        let mut compatible_skills = Vec::new();
        let mut incompatible_skills = Vec::new();

        for skill in skills {
            let analysis = self.analyze_skill(&skill);

            if analysis.should_filter {
                incompatible_skills.push(IncompatibleSkillInfo {
                    name: skill.name().to_string(),
                    description: skill.description().to_string(),
                    reason: analysis.analysis,
                    recommendations: analysis.recommendations,
                });
            } else {
                compatible_skills.push(skill);
            }
        }

        (compatible_skills, incompatible_skills)
    }
}

/// Information about incompatible skills
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncompatibleSkillInfo {
    pub name: String,
    pub description: String,
    pub reason: String,
    pub recommendations: Vec<String>,
}

/// Comprehensive validation report for all skills
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerValidationReport {
    pub total_skills_analyzed: usize,
    pub compatible_skills: Vec<String>,
    pub incompatible_skills: Vec<IncompatibleSkillInfo>,
    pub skills_with_fallbacks: Vec<SkillWithFallback>,
    pub summary: ValidationSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillWithFallback {
    pub name: String,
    pub description: String,
    pub fallback_description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSummary {
    pub total_compatible: usize,
    pub total_incompatible: usize,
    pub total_with_fallbacks: usize,
    pub recommendation: String,
}

impl ContainerValidationReport {
    pub fn new() -> Self {
        Self {
            total_skills_analyzed: 0,
            compatible_skills: Vec::new(),
            incompatible_skills: Vec::new(),
            skills_with_fallbacks: Vec::new(),
            summary: ValidationSummary {
                total_compatible: 0,
                total_incompatible: 0,
                total_with_fallbacks: 0,
                recommendation: String::new(),
            },
        }
    }

    pub fn add_skill_analysis(&mut self, skill_name: String, analysis: ContainerValidationResult) {
        self.total_skills_analyzed += 1;

        match analysis.requirement {
            ContainerSkillsRequirement::NotRequired => {
                self.compatible_skills.push(skill_name);
                self.summary.total_compatible += 1;
            }
            ContainerSkillsRequirement::Required => {
                self.incompatible_skills.push(IncompatibleSkillInfo {
                    name: skill_name.clone(),
                    description: "Container skills required".to_string(),
                    reason: analysis.analysis,
                    recommendations: analysis.recommendations,
                });
                self.summary.total_incompatible += 1;
            }
            ContainerSkillsRequirement::RequiredWithFallback => {
                self.skills_with_fallbacks.push(SkillWithFallback {
                    name: skill_name.clone(),
                    description: "Container skills with fallback".to_string(),
                    fallback_description: analysis.recommendations.join(" "),
                });
                self.summary.total_with_fallbacks += 1;
            }
            ContainerSkillsRequirement::Unknown => {
                // Treat unknown as compatible for safety
                self.compatible_skills.push(skill_name);
                self.summary.total_compatible += 1;
            }
        }
    }

    pub fn add_incompatible_skill(&mut self, name: String, description: String, reason: String) {
        self.incompatible_skills.push(IncompatibleSkillInfo {
            name,
            description,
            reason,
            recommendations: vec![
                "This skill requires Anthropic container skills which are not supported in VTCode.".to_string(),
                "Consider using alternative approaches with VTCode's code execution tools.".to_string(),
            ],
        });
        self.summary.total_incompatible += 1;
        self.total_skills_analyzed += 1;
    }

    pub fn finalize(&mut self) {
        self.summary.recommendation = match (self.summary.total_incompatible, self.summary.total_with_fallbacks) {
            (0, 0) => "All skills are fully compatible with VTCode.".to_string(),
            (0, _) => format!("{} skills have container skills dependencies but provide VTCode-compatible fallbacks.", self.summary.total_with_fallbacks),
            (_, 0) => format!("{} skills require container skills and cannot be used. Consider the suggested alternatives.", self.summary.total_incompatible),
            (_, _) => format!("{} skills require container skills. {} skills have fallbacks. Use alternatives or fallback instructions.",
                self.summary.total_incompatible, self.summary.total_with_fallbacks),
        };
    }

    pub fn format_report(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!("📊 Container Skills Validation Report\n"));
        output.push_str(&format!("=====================================\n\n"));
        output.push_str(&format!("Total Skills Analyzed: {}\n", self.total_skills_analyzed));
        output.push_str(&format!("Compatible: {}\n", self.summary.total_compatible));
        output.push_str(&format!("With Fallbacks: {}\n", self.summary.total_with_fallbacks));
        output.push_str(&format!("Incompatible: {}\n\n", self.summary.total_incompatible));
        output.push_str(&format!("{}", self.summary.recommendation));

        if !self.incompatible_skills.is_empty() {
            output.push_str("\n\nIncompatible Skills:");
            for skill in &self.incompatible_skills {
                output.push_str(&format!("\n  • {} - {}", skill.name, skill.description));
                for rec in &skill.recommendations {
                    output.push_str(&format!("\n    {}", rec));
                }
            }
        }

        if !self.skills_with_fallbacks.is_empty() {
            output.push_str("\n\nSkills with Fallbacks:");
            for skill in &self.skills_with_fallbacks {
                output.push_str(&format!("\n  • {} - {}", skill.name, skill.description));
            }
        }

        output
    }
}

impl Default for ContainerValidationReport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::types::{SkillManifest, Skill};
    use std::path::PathBuf;

    #[test]
    fn test_container_skills_detection() {
        let validator = ContainerSkillsValidator::new();

        // Test skill with container usage
        let manifest = SkillManifest {
            name: "pdf-report-generator".to_string(),
            description: "Generate PDFs".to_string(),
            version: Some("1.0.0".to_string()),
            author: Some("Test".to_string()),
            vtcode_native: None,
        };

        let instructions = r#"
        Generate PDF documents using Anthropic's pdf skill.

        ```python
        response = client.messages.create(
            model="claude-3-5-sonnet-20241022",
            container={
                "type": "skills",
                "skills": [{"type": "anthropic", "skill_id": "pdf", "version": "latest"}]
            },
            betas=["skills-2025-10-02"]
        )
        ```
        "#;

        let skill = Skill::new(manifest, PathBuf::from("/tmp"), instructions.to_string()).unwrap();
        let result = validator.analyze_skill(&skill);

        assert_eq!(result.requirement, ContainerSkillsRequirement::Required);
        assert!(result.should_filter);
        assert!(!result.patterns_found.is_empty());
    }

    #[test]
    fn test_enhanced_validation_with_fallback() {
        let validator = ContainerSkillsValidator::new();

        let manifest = SkillManifest {
            name: "spreadsheet-generator".to_string(),
            description: "Generate spreadsheets".to_string(),
            version: Some("1.0.0".to_string()),
            author: Some("Test".to_string()),
            vtcode_native: Some(true),
        };

        let instructions = r#"
        **vtcode does not currently support Anthropic container skills.** Instead, use:

        ### Option 1: Python Script with openpyxl
        Use vtcode's `execute_code` tool with Python and openpyxl library:

        ```python
        import openpyxl
        wb = openpyxl.Workbook()
        # ... create spreadsheet
        wb.save("output.xlsx")
        ```
        "#;

        let skill = Skill::new(manifest, PathBuf::from("/tmp"), instructions.to_string()).unwrap();
        let result = validator.analyze_skill(&skill);

        assert_eq!(result.requirement, ContainerSkillsRequirement::RequiredWithFallback);
        assert!(!result.should_filter);
        assert!(result.patterns_found.len() >= 2);

        // Test enhanced recommendations
        let recommendations = result.recommendations.join(" ");
        assert!(recommendations.contains("container skills"));
        assert!(recommendations.contains("fallback"));
        assert!(recommendations.contains("execute_code"));
    }

    #[test]
    fn test_enhanced_validation_without_fallback() {
        let validator = ContainerSkillsValidator::new();

        let manifest = SkillManifest {
            name: "pdf-report-generator".to_string(),
            description: "Generate PDFs".to_string(),
            version: Some("1.0.0".to_string()),
            author: Some("Test".to_string()),
            vtcode_native: None,
        };

        let instructions = r#"
        Generate PDF documents using Anthropic's pdf skill.

        ```python
        response = client.messages.create(
            model="claude-3-5-sonnet-20241022",
            container={
                "type": "skills",
                "skills": [{"type": "anthropic", "skill_id": "pdf", "version": "latest"}]
            },
            betas=["skills-2025-10-02"]
        )
        ```
        "#;

        let skill = Skill::new(manifest, PathBuf::from("/tmp"), instructions.to_string()).unwrap();
        let result = validator.analyze_skill(&skill);

        assert_eq!(result.requirement, ContainerSkillsRequirement::Required);
        assert!(result.should_filter);

        // Test enhanced recommendations
        let recommendations = result.recommendations.join(" ");
        assert!(recommendations.contains("cannot be used"));
        assert!(recommendations.contains("reportlab"));
        assert!(recommendations.contains("execute_code"));
    }

    #[test]
    fn test_validation_report_formatting() {
        let mut report = ContainerValidationReport::new();

        // Add test data
        report.add_incompatible_skill(
            "pdf-report-generator".to_string(),
            "Generate PDFs".to_string(),
            "Requires container skills".to_string()
        );

        report.add_skill_analysis(
            "spreadsheet-generator".to_string(),
            ContainerValidationResult {
                requirement: ContainerSkillsRequirement::RequiredWithFallback,
                analysis: "Has fallback".to_string(),
                patterns_found: vec!["execute_code".to_string()],
                recommendations: vec!["Use fallback".to_string()],
                should_filter: false,
            }
        );

        report.finalize();

        let formatted = report.format_report();
        assert!(formatted.contains("Container Skills Validation Report"));
        assert!(formatted.contains("pdf-report-generator"));
        assert!(formatted.contains("spreadsheet-generator"));
        assert!(formatted.contains("Incompatible Skills"));
        assert!(formatted.contains("Skills with Fallbacks"));
        assert!(formatted.contains("Total Skills Analyzed"));
    }
}