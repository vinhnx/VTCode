//! # Enhanced Skill Harness
//!
//! Provides an improved skill execution harness that automatically tracks and reports
//! generated files, eliminating the "where is it?" problem identified in session logs.

use crate::tools::generation_helpers::GenerationHelper;
use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;

/// Enhanced skill result with automatic file tracking
pub struct EnhancedSkillResult {
    pub message: String,
    pub generated_files: Vec<String>,
    pub metadata: Value,
}

/// Enhanced skill harness that provides automatic file verification
pub struct EnhancedSkillHarness {
    workspace_root: PathBuf,
    helper: GenerationHelper,
}

impl EnhancedSkillHarness {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root: workspace_root.clone(),
            helper: GenerationHelper::new(workspace_root),
        }
    }

    /// Execute a skill and automatically verify generated files
    pub async fn execute_skill_with_verification(
        &self,
        skill_name: &str,
        output_filename: Option<&str>,
        skill_logic: impl FnOnce() -> Result<()>,
    ) -> Result<EnhancedSkillResult> {
        // Execute the skill logic
        skill_logic()?;

        // If output filename is provided, verify it
        let mut verification_messages = Vec::new();
        let mut generated_files = Vec::new();

        if let Some(filename) = output_filename {
            let verification = self.helper.verify_and_report(filename).await?;
            verification_messages.push(verification.clone());
            generated_files.push(filename.to_string());
        }

        // Create result message
        let message = if verification_messages.is_empty() {
            format!("✓ Skill '{}' executed successfully", skill_name)
        } else {
            format!(
                "✓ Skill '{}' executed successfully\n\n{}",
                skill_name,
                verification_messages.join("\n")
            )
        };

        Ok(EnhancedSkillResult {
            message,
            generated_files,
            metadata: serde_json::json!({
                "skill_name": skill_name,
                "workspace_root": self.workspace_root.display().to_string(),
            }),
        })
    }

    /// Creates a standardized response for file generation skills
    pub async fn create_file_generation_response(
        &self,
        file_type: &str,
        filename: &str,
        generation_details: Option<&str>,
    ) -> Result<String> {
        let verification = self.helper.verify_and_report(filename).await?;

        let details_section = if let Some(details) = generation_details {
            format!("\n\n{}", details)
        } else {
            String::new()
        };

        Ok(format!(
            "✓ Generated {}: {}\n\n{}{}",
            file_type, filename, verification, details_section
        ))
    }

    /// Creates a quick sample response with immediate file verification
    pub async fn create_sample_response(
        &self,
        file_type: &str,
        generator_code: impl FnOnce(&PathBuf) -> Result<()>,
    ) -> Result<String> {
        // Generate a timestamped filename
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("sample_{}_{}", file_type.to_lowercase(), timestamp);
        let filepath = self.workspace_root.join(&filename);

        // Execute the generator
        generator_code(&filepath)?;

        // Verify and report
        self.create_file_generation_response(
            file_type,
            &filename,
            Some("This is a sample file. You can modify the content as needed."),
        )
        .await
    }
}

/// Quick execution helper for common patterns
pub async fn execute_file_generation_skill(
    workspace_root: PathBuf,
    skill_name: &str,
    filename: &str,
    success_message: Option<&str>,
) -> Result<String> {
    let harness = EnhancedSkillHarness::new(workspace_root);

    let verification = harness.helper.verify_and_report(filename).await?;

    let message = if let Some(custom_msg) = success_message {
        format!("{}\n\n{}", custom_msg, verification)
    } else {
        format!("✓ Skill '{}' completed.\n\n{}", skill_name, verification)
    };

    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_enhanced_harness_creation() {
        let temp_dir = TempDir::new().unwrap();
        let harness = EnhancedSkillHarness::new(temp_dir.path().to_path_buf());

        // Test basic creation
        assert_eq!(harness.workspace_root, temp_dir.path().to_path_buf());
    }

    #[tokio::test]
    async fn test_execute_skill_with_verification() {
        let temp_dir = TempDir::new().unwrap();
        let harness = EnhancedSkillHarness::new(temp_dir.path().to_path_buf());

        // Test with a skill that doesn't generate files
        let result = harness
            .execute_skill_with_verification("test-skill", None, || Ok(()))
            .await
            .unwrap();

        assert!(result.message.contains("test-skill"));
        assert!(result.message.contains("executed successfully"));
        assert!(result.generated_files.is_empty());
    }

    #[tokio::test]
    async fn test_create_file_generation_response() {
        let temp_dir = TempDir::new().unwrap();
        let harness = EnhancedSkillHarness::new(temp_dir.path().to_path_buf());

        let response = harness
            .create_file_generation_response("PDF", "nonexistent.pdf", None)
            .await
            .unwrap();

        assert!(response.contains("Generated PDF"));
        assert!(response.contains("nonexistent.pdf"));
    }

    #[tokio::test]
    async fn test_quick_execution_helper() {
        let temp_dir = TempDir::new().unwrap();
        let result = execute_file_generation_skill(
            temp_dir.path().to_path_buf(),
            "test-skill",
            "test.pdf",
            Some("Custom success message"),
        )
        .await
        .unwrap();

        assert!(result.contains("Custom success message"));
    }
}
