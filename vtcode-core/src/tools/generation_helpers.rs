//! # Generation Helpers
//!
//! Provides utilities for tracking and verifying files generated through code execution,
//! skills, and other tools. This addresses the common pattern where users ask "where is it?"
//! after file generation.

use crate::tools::file_tracker::FileTracker;
use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;

/// Helper for ensuring generated files are properly tracked and reported
pub struct GenerationHelper {
    file_tracker: FileTracker,
}

impl GenerationHelper {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            file_tracker: FileTracker::new(workspace_root),
        }
    }

    /// Creates a verification message for a specific file
    pub async fn verify_and_report(&self, filename: &str) -> Result<String> {
        match self.file_tracker.verify_file_exists(filename).await? {
            Some(file_info) => Ok(format!(
                "✓ Generated: {} ({} bytes)",
                file_info.absolute_path.display(),
                file_info.size
            )),
            None => Ok(format!(
                "⚠ File not found: {}. Generation may have failed or file was created in a different location.",
                filename
            )),
        }
    }

    /// Generates a complete response with file verification
    pub async fn create_verified_response(
        &self,
        filename: &str,
        additional_info: Option<&str>,
    ) -> Result<String> {
        let file_report = self.verify_and_report(filename).await?;

        let response = if let Some(info) = additional_info {
            format!("{}\n\n{}", info, file_report)
        } else {
            file_report
        };

        Ok(response)
    }

    /// Creates a JSON response suitable for tool execution results
    pub async fn create_json_response(&self, filename: &str, metadata: Value) -> Result<Value> {
        let verification = self.file_tracker.verify_file_exists(filename).await?;

        Ok(serde_json::json!({
            "status": "completed",
            "filename": filename,
            "verification": verification.map(|f| f.to_json()),
            "metadata": metadata,
        }))
    }
}

/// Quick verification function for immediate file existence check
pub async fn quick_verify(workspace_root: PathBuf, filename: &str) -> Result<String> {
    let helper = GenerationHelper::new(workspace_root);
    helper.verify_and_report(filename).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_verification_message() {
        let temp_dir = TempDir::new().unwrap();
        let helper = GenerationHelper::new(temp_dir.path().to_path_buf());

        // Test non-existent file
        let result = helper.verify_and_report("test.pdf").await.unwrap();
        assert!(result.contains("⚠ File not found"));
    }

    #[tokio::test]
    async fn test_create_verified_response() {
        let temp_dir = TempDir::new().unwrap();
        let helper = GenerationHelper::new(temp_dir.path().to_path_buf());

        let response = helper
            .create_verified_response("test.pdf", Some("Additional info"))
            .await
            .unwrap();

        assert!(response.contains("Additional info"));
        assert!(response.contains("⚠ File not found"));
    }
}
