//! File reference validation for Agent Skills
//!
//! Validates file references in SKILL.md bodies to ensure they meet
//! the Agent Skills specification requirements.

use regex::Regex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Validates that file references in skill instructions follow the Agent Skills spec
///
/// Requirements:
/// - References must be relative paths from the skill root
/// - Must be one level deep (no nested chains like `references/subdir/file.md`)
/// - Must reference files in supported directories: scripts/, references/, assets/
/// - Referenced files must exist
pub struct FileReferenceValidator {
    skill_root: PathBuf,
}

impl FileReferenceValidator {
    /// Create a new validator for a skill at the given root path
    pub fn new(skill_root: PathBuf) -> Self {
        Self { skill_root }
    }

    /// Validate all file references in the instruction text
    ///
    /// Returns a list of validation errors (empty if valid)
    pub fn validate_references(&self, instructions: &str) -> Vec<String> {
        let mut errors = Vec::new();
        let references = self.extract_references(instructions);

        for reference in &references {
            if let Err(e) = self.validate_reference(reference) {
                errors.push(format!("Invalid reference '{}': {}", reference, e));
            }
        }

        errors
    }

    /// Extract file references from instruction text
    ///
    /// Looks for patterns like:
    /// - `[text](references/FILE.md)`
    /// - `scripts/script.py`
    /// - `assets/image.png`
    fn extract_references(&self, instructions: &str) -> HashSet<String> {
        let mut references = HashSet::new();
        let md_link_regex = Regex::new(r"\[.*?\]\((.*?)\)").unwrap();
        let plain_path_regex = Regex::new(r"\b(scripts|references|assets)/[^\s\),\]]+").unwrap();

        // Extract markdown links
        for cap in md_link_regex.captures_iter(instructions) {
            if let Some(path_match) = cap.get(1) {
                let path = path_match.as_str();
                references.insert(path.to_string());
            }
        }

        // Extract plain paths
        for cap in plain_path_regex.captures_iter(instructions) {
            if let Some(path_match) = cap.get(0) {
                references.insert(path_match.as_str().to_string());
            }
        }

        references
    }

    /// Validate a single file reference
    fn validate_reference(&self, reference: &str) -> Result<(), String> {
        // Check if it's a valid path format
        let path = Path::new(reference);

        // Must be relative (no absolute paths)
        if path.is_absolute() {
            return Err("Absolute paths are not allowed".to_string());
        }

        // Must be within supported directories
        let components: Vec<_> = path.components().collect();
        if components.is_empty() {
            return Err("Empty path".to_string());
        }

        // Check first component is a supported directory
        if let Some(first_component) = components.first() {
            let first_dir = first_component.as_os_str().to_string_lossy();
            if !matches!(first_dir.as_ref(), "scripts" | "references" | "assets") {
                return Err(format!(
                    "Invalid directory '{}'. Must be 'scripts/', 'references/', or 'assets/'",
                    first_dir
                ));
            }
        }

        // Check depth - must be one level deep (e.g., scripts/file.py, not scripts/subdir/file.py)
        if components.len() > 2 {
            return Err(format!(
                "Path is too deep: '{}'. Per Agent Skills spec, references must be one level deep.",
                reference
            ));
        }

        // For paths with 2 components (dir + file), validate file exists
        if components.len() == 2 {
            let full_path = self.skill_root.join(path);
            if !full_path.exists() {
                return Err(format!("Referenced file does not exist: {:?}", full_path));
            }
        }

        Ok(())
    }

    /// Get all valid references from a skill directory
    pub fn list_valid_references(&self) -> Vec<PathBuf> {
        let mut references = Vec::new();

        for subdir in &["scripts", "references", "assets"] {
            let dir = self.skill_root.join(subdir);
            if dir.is_dir()
                && let Ok(entries) = std::fs::read_dir(&dir)
            {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        references.push(
                            path.strip_prefix(&self.skill_root)
                                .unwrap_or(&path)
                                .to_path_buf(),
                        );
                    }
                }
            }
        }

        references
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_valid_file_references() {
        let temp_dir = TempDir::new().unwrap();
        let skill_root = temp_dir.path().to_path_buf();

        // Create test files
        fs::create_dir(skill_root.join("scripts")).unwrap();
        fs::write(skill_root.join("scripts/helper.py"), "# test").unwrap();

        let validator = FileReferenceValidator::new(skill_root);
        let instructions = r#"
            See [the reference](references/REFERENCE.md) for details.
            Run the extraction script: scripts/helper.py
        "#;

        let errors = validator.validate_references(instructions);
        // Should have an error for references/REFERENCE.md (doesn't exist)
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("references/REFERENCE.md"));
    }

    #[test]
    fn test_invalid_directory() {
        let validator = FileReferenceValidator::new(PathBuf::from("/tmp"));
        // Use a valid directory pattern but non-existent file
        let errors = validator.validate_references("See `scripts/nonexistent.py`");
        assert!(!errors.is_empty());
        assert!(errors[0].contains("nonexistent.py"));
    }

    #[test]
    fn test_deep_path_error() {
        let validator = FileReferenceValidator::new(PathBuf::from("/tmp"));
        let errors = validator.validate_references("See `scripts/subdir/deep.py`");
        assert!(!errors.is_empty());
        assert!(errors[0].contains("too deep"));
    }

    #[test]
    fn test_list_valid_references() {
        let temp_dir = TempDir::new().unwrap();
        let skill_root = temp_dir.path().to_path_buf();

        fs::create_dir(skill_root.join("scripts")).unwrap();
        fs::create_dir(skill_root.join("references")).unwrap();
        fs::write(skill_root.join("scripts/test.py"), "# test").unwrap();
        fs::write(skill_root.join("references/ref.md"), "# ref").unwrap();

        let validator = FileReferenceValidator::new(skill_root);
        let refs = validator.list_valid_references();

        assert_eq!(refs.len(), 2);
        assert!(
            refs.iter()
                .any(|p| p.to_string_lossy() == "scripts/test.py")
        );
        assert!(
            refs.iter()
                .any(|p| p.to_string_lossy() == "references/ref.md")
        );
    }
}
