//! # Skill File Tracker
//! 
//! Generic file tracking for ALL skills, not just execute_code.
//! Automatically intercepts skill execution and verifies any files mentioned in output.

use crate::tools::file_tracker::FileTracker;
use anyhow::Result;
use std::path::PathBuf;
use std::collections::HashSet;
use regex::Regex;

/// Generic file tracker that works with any skill output
pub struct SkillFileTracker {
    workspace_root: PathBuf,
    file_tracker: FileTracker,
    file_patterns: Vec<Regex>,
}

impl SkillFileTracker {
    pub fn new(workspace_root: PathBuf) -> Self {
        let file_tracker = FileTracker::new(workspace_root.clone());
        
        // Common file patterns in skill output
        let patterns = vec![
            // Pattern: "file.ext", 'file.ext', or just file.ext
            Regex::new("['\"]?([\\w.-]+\\.(?:pdf|xlsx|csv|docx|png|jpg|json|xml|txt|md))['\"]?").unwrap(),
            // Pattern: path/to/file.ext
            Regex::new("['\"]?([\\w/\\\\.-]+\\.(?:pdf|xlsx|csv|docx|png|jpg|json|xml|txt|md))['\"]?").unwrap(),
            // Pattern: Generated: filename
            Regex::new("(?:[Gg]enerated|[Cc]reated):\\s*([\\w.-]+\\.(?:pdf|xlsx|csv|docx|png|jpg|json|xml|txt|md))").unwrap(),
            // Pattern: Output saved to filename
            Regex::new("[Oo]utput (?:saved|written) to:?(?:\\s*)([\\w.-]+\\.(?:pdf|xlsx|csv|docx|png|jpg|json|xml|txt|md))").unwrap(),
        ];
        
        Self {
            workspace_root,
            file_tracker,
            file_patterns: patterns,
        }
    }
    
    /// Scan skill output for file references and verify their existence
    pub async fn scan_and_verify_skill_output(&self, output: &str) -> Result<SkillFileVerification> {
        let mut detected_files = HashSet::new();
        
        // Extract potential filenames from output
        for pattern in &self.file_patterns {
            for capture in pattern.captures_iter(output) {
                if let Some(file_match) = capture.get(1) {
                    let filename = file_match.as_str();
                    // Filter out common false positives
                    if !Self::is_false_positive(filename) {
                        detected_files.insert(filename.to_string());
                    }
                }
            }
        }
        
        // Verify each detected file
        let mut verified_files = Vec::new();
        let mut missing_files = Vec::new();
        
        for filename in detected_files {
            match self.file_tracker.verify_file_exists(&filename).await? {
                Some(file_info) => {
                    verified_files.push(VerifiedFile {
                        filename: filename.clone(),
                        absolute_path: file_info.absolute_path,
                        size: file_info.size,
                        status: FileStatus::Found,
                    });
                }
                None => {
                    // Try alternative: maybe it's in a subdirectory 
                    let alt_path = self.find_alternative_location(&filename).await?;
                    if let Some(alt_file) = alt_path {
                        verified_files.push(VerifiedFile {
                            filename: filename.clone(),
                            absolute_path: alt_file.absolute_path,
                            size: alt_file.size,
                            status: FileStatus::FoundAlternative,
                        });
                    } else {
                        missing_files.push(MissingFile {
                            filename: filename.clone(),
                            attempted_locations: vec![self.workspace_root.join(&filename)],
                            suggestions: self.generate_suggestions(&filename),
                        });
                    }
                }
            }
        }
        
        let summary = self.generate_verification_summary(&verified_files, &missing_files);
        let suggestion = self.generate_user_suggestion(&verified_files, &missing_files);
        
        Ok(SkillFileVerification {
            verified_files,
            missing_files,
            summary,
            suggestion,
        })
    }
    
    /// Post-process skill output to add file verification information
    pub async fn enhance_skill_output(&self, original_output: String) -> Result<String> {
        let verification = self.scan_and_verify_skill_output(&original_output).await?;
        
        if verification.verified_files.is_empty() && verification.missing_files.is_empty() {
            // No files detected, return original output
            return Ok(original_output);
        }
        
        let enhanced_output = format!(
            "{original_output}\n\n{}",
            verification.summary
        );
        
        Ok(enhanced_output)
    }
    
    /// Find file in alternative locations (subdirectories, etc.)
    async fn find_alternative_location(&self, filename: &str) -> Result<Option<TrackedFile>> {
        // Search in common subdirectories
        let subdirs = vec!["output", "results", "generated", "dist", "build", "tmp"];
        
        for subdir in subdirs {
            let alt_path = self.workspace_root.join(subdir).join(filename);
            if let Some(file_info) = self.verify_file_at_path(&alt_path).await? {
                return Ok(Some(file_info));
            }
        }
        
        // Search entire workspace recursively
        let pattern = format!("**/{}", filename);
        if let Ok(mut files) = self.file_tracker.find_files_matching_pattern(&pattern).await {
            if let Some(path) = files.pop() {
                if let Ok(Some(file_info)) = self.verify_file_at_path(&path).await {
                    return Ok(Some(file_info));
                }
            }
        }
        
        Ok(None)
    }
    
    /// Verify file at specific path
    async fn verify_file_at_path(&self, path: &PathBuf) -> Result<Option<TrackedFile>> {
        if let Ok(metadata) = tokio::fs::metadata(path).await {
            if metadata.is_file() {
                return Ok(Some(TrackedFile {
                    absolute_path: path.clone(),
                    size: metadata.len(),
                    modified: metadata.modified().unwrap_or(std::time::SystemTime::now()),
                }));
            }
        }
        Ok(None)
    }
    
    /// Generate suggestions for missing files
    fn generate_suggestions(&self, filename: &str) -> Vec<String> {
        vec![
            format!("Check if '{}' was created with a different name", filename),
            "Verify the skill execution completed successfully".to_string(),
            "Check subdirectories like 'output/', 'generated/', or 'dist/'".to_string(),
            format!("Run 'find . -name \"{}\"' to search for the file", filename),
        ]
    }
    
    /// Check if filename is a false positive
    fn is_false_positive(filename: &str) -> bool {
        let false_positives = vec![
            "example.pdf", "template.xlsx", "sample.csv",  // Template names
            "Cargo.toml", "package.json", "go.mod",       // Config files
            "README.md", "LICENSE.txt", ".gitignore",      // Project files
        ];
        
        false_positives.contains(&filename) || filename.starts_with('.')
    }
    
    /// Generate summary of verification results
    fn generate_verification_summary(
        &self,
        verified: &[VerifiedFile],
        missing: &[MissingFile],
    ) -> String {
        let mut summary = String::new();
        
        if !verified.is_empty() {
            summary.push_str("✅ Generated Files:\n");
            for file in verified {
                match file.status {
                    FileStatus::Found => {
                        summary.push_str(&format!(
                            "   ✓ {} → {} ({} bytes)\n",
                            file.filename,
                            file.absolute_path.display(),
                            file.size
                        ));
                    }
                    FileStatus::FoundAlternative => {
                        summary.push_str(&format!(
                            "   ✓ {} → {} ({} bytes) [found in alternative location]\n",
                            file.filename,
                            file.absolute_path.display(),
                            file.size
                        ));
                    }
                }
            }
        }
        
        if !missing.is_empty() {
            if !summary.is_empty() {
                summary.push('\n');
            }
            summary.push_str("⚠️  Missing Files:\n");
            for file in missing {
                summary.push_str(&format!("   ✗ {}\n", file.filename));
                for suggestion in &file.suggestions {
                    summary.push_str(&format!("     • {}\n", suggestion));
                }
            }
        }
        
        summary
    }
    
    /// Generate user-friendly suggestion
    fn generate_user_suggestion(&self, verified: &[VerifiedFile], missing: &[MissingFile]) -> String {
        if missing.is_empty() && verified.len() == 1 {
            format!(
                "File generated at: {}",
                verified[0].absolute_path.display()
            )
        } else if missing.is_empty() && !verified.is_empty() {
            format!("{} files generated successfully", verified.len())
        } else if !missing.is_empty() && verified.is_empty() {
            "Some files could not be found. Please check the output above.".to_string()
        } else {
            format!(
                "Generated {} files, {} files missing. See summary above.",
                verified.len(),
                missing.len()
            )
        }
    }
}

impl From<crate::tools::file_tracker::TrackedFile> for TrackedFile {
    fn from(file: crate::tools::file_tracker::TrackedFile) -> Self {
        Self {
            absolute_path: file.absolute_path,
            size: file.size,
            modified: file.modified,
        }
    }
}

/// Verification result for skill-generated files
#[derive(Debug, Clone)]
pub struct SkillFileVerification {
    pub verified_files: Vec<VerifiedFile>,
    pub missing_files: Vec<MissingFile>,
    pub summary: String,
    pub suggestion: String,
}

/// Verified file information
#[derive(Debug, Clone)]
pub struct VerifiedFile {
    pub filename: String,
    pub absolute_path: PathBuf,
    pub size: u64,
    pub status: FileStatus,
}

/// File verification status
#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Found,
    FoundAlternative,
}

/// Missing file information
#[derive(Debug, Clone)]
pub struct MissingFile {
    pub filename: String,
    pub attempted_locations: Vec<PathBuf>,
    pub suggestions: Vec<String>,
}

/// Tracked file (simplified from file_tracker::TrackedFile)
#[derive(Debug, Clone)]
pub struct TrackedFile {
    pub absolute_path: PathBuf,
    pub size: u64,
    pub modified: std::time::SystemTime,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_skill_file_scanning() {
        let temp_dir = TempDir::new().unwrap();
        let tracker = SkillFileTracker::new(temp_dir.path().to_path_buf());
        
        // Test output with file references
        let output = r#"
Generated PDF report: quarterly_report.pdf
Also created summary.csv with key metrics.
Output saved to: chart.png
"#;
        
        let result = tracker.scan_and_verify_skill_output(output).await.unwrap();
        assert_eq!(result.verified_files.len(), 0); // No files actually created
        assert_eq!(result.missing_files.len(), 3); // All detected but missing
        
        let missing_names: Vec<String> = result.missing_files.iter()
            .map(|m| m.filename.clone())
            .collect();
        
        assert!(missing_names.contains(&"quarterly_report.pdf".to_string()));
        assert!(missing_names.contains(&"summary.csv".to_string()));
        assert!(missing_names.contains(&"chart.png".to_string()));
    }
    
    #[tokio::test]
    async fn test_enhance_skill_output() {
        let temp_dir = TempDir::new().unwrap();
        let tracker = SkillFileTracker::new(temp_dir.path().to_path_buf());
        
        let original = "Generated: report.pdf".to_string();
        let enhanced = tracker.enhance_skill_output(original.clone()).await.unwrap();
        
        assert!(enhanced.contains("Generated: report.pdf"));
        assert!(enhanced.contains("Generated Files") || enhanced.contains("Missing Files"));
    }
    
    #[test]
    fn test_false_positive_detection() {
        assert!(SkillFileTracker::is_false_positive("Cargo.toml"));
        assert!(SkillFileTracker::is_false_positive("README.md"));
        assert!(!SkillFileTracker::is_false_positive("report.pdf"));
        assert!(!SkillFileTracker::is_false_positive("my_chart.png"));
    }
}
