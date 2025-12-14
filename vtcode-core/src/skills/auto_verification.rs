//! # Auto Skill Verification
//! 
//! Automatically enhances ALL skill outputs with file verification.
//! This is the generic layer that works for pdf-generator-vtcode, spreadsheet-generator,
//! and any other skill without requiring skill-specific code.

use crate::skills::skill_file_tracker::SkillFileTracker;
use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;
use tracing::{debug, info};

/// Auto-verification wrapper for skill outputs
pub struct AutoSkillVerifier {
    tracker: SkillFileTracker,
    enabled: bool,
}

impl AutoSkillVerifier {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            tracker: SkillFileTracker::new(workspace_root),
            enabled: true,
        }
    }
    
    /// Enable or disable auto-verification
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    /// Process skill output and add file verification (generic for ALL skills)
    pub async fn process_skill_output(
        &self,
        skill_name: &str,
        original_output: String,
    ) -> Result<String> {
        if !self.enabled {
            return Ok(original_output);
        }
        
        info!("Auto-verifying skill output for: {}", skill_name);
        
        // Check if output already contains verification (prevent double-verification)
        if Self::already_verified(&original_output) {
            debug!("Skill output already contains verification, skipping");
            return Ok(original_output);
        }
        
        // Enhance the output with file verification
        let enhanced = self.tracker.enhance_skill_output(original_output.clone()).await?;
        
        // Add skill-specific header
        let final_output = if enhanced.len() > original_output.len() {
            // Files were detected and verification added
            let verification = enhanced.strip_prefix(&format!("{}\n\n", original_output))
                .unwrap_or(&enhanced);
            
            format!(
                "✓ Skill '{}' executed\n\n{}{}",
                skill_name,
                original_output,
                if !verification.is_empty() { 
                    format!("\n\n{}", verification) 
                } else { 
                    String::new() 
                }
            )
        } else {
            // No files detected, return original
            enhanced
        };
        
        Ok(final_output)
    }
    
    /// Process JSON skill result and enhance it
    pub async fn process_skill_result(
        &self,
        skill_name: &str,
        mut result: Value,
    ) -> Result<Value> {
        if !self.enabled {
            return Ok(result);
        }
        
        // Extract text output from various skill result formats
        let output_text = Self::extract_output_text(&result);
        
        if let Some(text) = output_text {
            let enhanced = self.process_skill_output(skill_name, text).await?;
            
            // Update the result with enhanced output
            if let Some(output_field) = result.get_mut("output") {
                *output_field = Value::String(enhanced);
            } else if let Some(message_field) = result.get_mut("message") {
                *message_field = Value::String(enhanced);
            } else {
                result["enhanced_output"] = Value::String(enhanced);
            }
        }
        
        Ok(result)
    }
    
    /// Check if output already contains verification
    fn already_verified(output: &str) -> bool {
        output.contains("Generated Files:") || 
        output.contains("Missing Files:") ||
        output.contains("✓ Generated:") ||
        output.contains("File generated at:")
    }
    
    /// Extract output text from various skill result formats
    fn extract_output_text(result: &Value) -> Option<String> {
        // Check common output fields
        if let Some(output) = result.get("output").and_then(|v| v.as_str()) {
            return Some(output.to_string());
        }
        
        if let Some(message) = result.get("message").and_then(|v| v.as_str()) {
            return Some(message.to_string());
        }
        
        if let Some(result_str) = result.get("result").and_then(|v| v.as_str()) {
            return Some(result_str.to_string());
        }
        
        // Fallback: stringify the entire result
        Some(serde_json::to_string_pretty(result).unwrap_or_default())
    }
    
    /// Create a standard success response with verification
    pub async fn create_success_response(
        &self,
        skill_name: &str,
        details: &str,
        output_hint: Option<&str>,
    ) -> Result<String> {
        let mut response = format!(
            "✓ Skill '{}' executed successfully\n\n{}",
            skill_name, details
        );
        
        if let Some(hint) = output_hint {
            // Scan the hint for files
            let verification = self.tracker.scan_and_verify_skill_output(hint).await?;
            
            if !verification.verified_files.is_empty() || !verification.missing_files.is_empty() {
                response.push_str("\n\n");
                response.push_str(&verification.summary);
            }
        }
        
        Ok(response)
    }
    
    /// Create error response with helpful suggestions
    pub fn create_error_response(skill_name: &str, error: &str) -> String {
        format!(
            "❌ Skill '{}' failed\n\nError: {}\n\n💡 Try:\n   • Verify the skill is properly installed\n   • Check that all dependencies are available\n   • Ensure you have the required permissions",
            skill_name, error
        )
    }
}

// Note: Global instance intentionally omitted to avoid static mut warnings.
// Use AutoSkillVerifier::new() to create local instances as needed.

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_auto_verifier_creation() {
        let temp_dir = TempDir::new().unwrap();
        let verifier = AutoSkillVerifier::new(temp_dir.path().to_path_buf());
        assert!(verifier.enabled);
    }
    
    #[tokio::test]
    async fn test_process_skill_output() {
        let temp_dir = TempDir::new().unwrap();
        let verifier = AutoSkillVerifier::new(temp_dir.path().to_path_buf());
        
        let output = "Generated report.pdf".to_string();
        let enhanced = verifier.process_skill_output("test-skill", output).await.unwrap();
        
        assert!(enhanced.contains("test-skill"));
        assert!(enhanced.contains("Generated report.pdf"));
    }
    
    #[tokio::test]
    async fn test_already_verified_detection() {
        let output = "File generated at: test.pdf";
        assert!(AutoSkillVerifier::already_verified(output));
        
        let output = "Some random text";
        assert!(!AutoSkillVerifier::already_verified(output));
    }
    
    #[tokio::test]
    async fn test_extract_output_text() {
        let json = serde_json::json!({
            "output": "Generated file.pdf"
        });
        assert_eq!(
            AutoSkillVerifier::extract_output_text(&json),
            Some("Generated file.pdf".to_string())
        );
        
        let json_str = serde_json::json!("Plain string output");
        assert!(AutoSkillVerifier::extract_output_text(&json_str).is_some());
    }
}
