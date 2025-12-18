//! Skill Authoring Tool for VT Code
//!
//! Implements Anthropic's Agent Skills specification for creating, validating,
//! and packaging skills that extend VT Code's capabilities.
//!
//! Based on: https://github.com/anthropics/skills/blob/main/spec/agent-skills-spec.md

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Skill template following Anthropic's specification
pub const SKILL_TEMPLATE: &str = r#"---
name: {skill_name}
description: [TODO: Complete and informative explanation of what the skill does and when to use it. Include WHEN to use this skill - specific scenarios, file types, or tasks that trigger it.]
---

# {skill_title}

## Overview

[TODO: 1-2 sentences explaining what this skill enables]

## Quick Start

[TODO: Provide a simple example of using this skill]

## Resources

This skill includes example resource directories:

### scripts/
Executable code (Python/Bash/etc.) for deterministic operations.

### references/
Documentation loaded into context as needed.

### assets/
Files used in output (templates, icons, fonts, etc.).

Delete unused directories.
"#;

/// Example Python script template
pub const EXAMPLE_SCRIPT: &str = r#"#!/usr/bin/env python3
"""
Example script for {skill_name}

This demonstrates how to include executable scripts in a skill.
Scripts provide deterministic, token-efficient operations.
"""

import sys

def main():
    print("Example script for {skill_name}")
    print("Replace with actual functionality")

if __name__ == "__main__":
    main()
"#;

/// Example reference document template
pub const EXAMPLE_REFERENCE: &str = r#"# {skill_title} API Reference

This is an example reference document that Claude can read as needed.

## Overview

Reference docs are ideal for:
- Comprehensive API documentation
- Detailed workflow guides
- Complex multi-step processes
- Content that's only needed for specific use cases

## Usage

Include specific API endpoints, schemas, or detailed instructions here.
"#;

/// YAML frontmatter for SKILL.md
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "allowed-tools")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

/// Skill authoring operations
pub struct SkillAuthor {
    workspace_root: PathBuf,
}

impl SkillAuthor {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    /// Create a new skill from template
    ///
    /// Following Anthropic's init_skill.py pattern
    pub fn create_skill(&self, skill_name: &str, output_dir: Option<PathBuf>) -> Result<PathBuf> {
        // Validate skill name
        self.validate_skill_name(skill_name)?;

        // Determine output directory
        let skills_dir = output_dir.unwrap_or_else(|| self.workspace_root.join("skills"));
        let skill_dir = skills_dir.join(skill_name);

        // Check if exists
        if skill_dir.exists() {
            return Err(anyhow!(
                "Skill directory already exists: {}",
                skill_dir.display()
            ));
        }

        // Create skill directory
        fs::create_dir_all(&skill_dir)?;
        info!("Created skill directory: {}", skill_dir.display());

        // Create SKILL.md
        let skill_title = self.title_case_skill_name(skill_name);
        let skill_content = SKILL_TEMPLATE
            .replace("{skill_name}", skill_name)
            .replace("{skill_title}", &skill_title);

        fs::write(skill_dir.join("SKILL.md"), skill_content)?;
        info!("Created SKILL.md");

        // Create resource directories with examples
        self.create_scripts_dir(&skill_dir, skill_name)?;
        self.create_references_dir(&skill_dir, &skill_title)?;
        self.create_assets_dir(&skill_dir)?;

        Ok(skill_dir)
    }

    /// Validate a skill following Anthropic's specification
    pub fn validate_skill(&self, skill_dir: &Path) -> Result<ValidationReport> {
        let mut report = ValidationReport::new(skill_dir.to_path_buf());

        // Check SKILL.md exists
        let skill_md = skill_dir.join("SKILL.md");
        if !skill_md.exists() {
            report.errors.push("SKILL.md not found".to_string());
            return Ok(report);
        }

        // Read and parse content
        let content = fs::read_to_string(&skill_md)?;

        // Extract frontmatter
        if !content.starts_with("---") {
            report.errors.push("No YAML frontmatter found".to_string());
            return Ok(report);
        }

        let parts: Vec<&str> = content.splitn(3, "---").collect();
        if parts.len() < 3 {
            report.errors.push("Invalid frontmatter format".to_string());
            return Ok(report);
        }

        // Parse YAML
        let frontmatter: SkillFrontmatter = serde_yaml::from_str(parts[1].trim())
            .map_err(|e| anyhow!("Invalid YAML frontmatter: {}", e))?;

        // Validate frontmatter properties (only allowed: name, description, license, allowed-tools, metadata)
        let raw_frontmatter: serde_yaml::Value =
            serde_yaml::from_str(parts[1].trim()).map_err(|e| anyhow!("Invalid YAML: {}", e))?;
        if let serde_yaml::Value::Mapping(map) = raw_frontmatter {
            let allowed = [
                "name",
                "description",
                "license",
                "allowed-tools",
                "metadata",
            ];
            for key in map.keys() {
                if let Some(key_str) = key.as_str().filter(|s| !allowed.contains(s)) {
                    report.errors.push(format!(
                        "Unexpected property '{}' in frontmatter. Allowed: {}",
                        key_str,
                        allowed.join(", ")
                    ));
                }
            }
        }

        // Validate name
        if frontmatter.name.is_empty() {
            report.errors.push("Skill name is required".to_string());
        } else if !self.is_valid_skill_name(&frontmatter.name) {
            let mut reasons = Vec::new();
            if frontmatter.name.chars().any(|c| c.is_ascii_uppercase()) {
                reasons.push("must be lowercase");
            }
            if frontmatter.name.contains('_') {
                reasons.push("no underscores allowed");
            }
            if frontmatter.name.starts_with('-') || frontmatter.name.ends_with('-') {
                reasons.push("cannot start or end with hyphen");
            }
            if frontmatter.name.contains("--") {
                reasons.push("no consecutive hyphens");
            }
            if frontmatter.name.len() > 64 {
                reasons.push("max 64 characters");
            }
            if frontmatter.name.contains("anthropic") || frontmatter.name.contains("claude") {
                reasons.push("reserved words not allowed");
            }
            report.errors.push(format!(
                "Invalid skill name '{}': {}",
                frontmatter.name,
                reasons.join(", ")
            ));
        }

        // Validate description
        if frontmatter.description.is_empty() {
            report.errors.push("Description is required".to_string());
        } else if frontmatter.description.contains("[TODO") {
            report
                .warnings
                .push("Description contains TODO placeholder".to_string());
            if frontmatter.description.contains('<') || frontmatter.description.contains('>') {
                report
                    .errors
                    .push("Description cannot contain angle brackets (< or >)".to_string());
            }
            if frontmatter.description.len() > 1024 {
                report.errors.push(format!(
                    "Description is too long ({} characters). Maximum is 1024 characters.",
                    frontmatter.description.len()
                ));
            }
        } else {
            if frontmatter.description.contains('<') || frontmatter.description.contains('>') {
                report
                    .errors
                    .push("Description cannot contain angle brackets (< or >)".to_string());
            }
            if frontmatter.description.len() > 1024 {
                report.errors.push(format!(
                    "Description is too long ({} characters). Maximum is 1024 characters.",
                    frontmatter.description.len()
                ));
            }
        }

        // Check body content
        let body = parts[2].trim();
        if body.is_empty() {
            report.warnings.push("SKILL.md body is empty".to_string());
        }

        if body.len() > 50000 {
            report.warnings.push(
                "SKILL.md body is very long (>50k chars). Consider splitting into reference files."
                    .to_string(),
            );
        }

        // Validate directory structure
        self.validate_structure(skill_dir, &mut report)?;

        report.valid = report.errors.is_empty();
        Ok(report)
    }

    /// Package a skill into .skill file (zip format)
    pub fn package_skill(&self, skill_dir: &Path, output_dir: Option<PathBuf>) -> Result<PathBuf> {
        // Validate first
        let report = self.validate_skill(skill_dir)?;
        if !report.valid {
            return Err(anyhow!("Skill validation failed:\n{}", report.format()));
        }

        // Determine output path
        let skill_name = skill_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow!("Invalid skill directory name"))?;

        let output_dir = output_dir.unwrap_or_else(|| self.workspace_root.clone());
        let output_file = output_dir.join(format!("{}.skill", skill_name));

        // Create zip file
        use zip::ZipWriter;

        let file = fs::File::create(&output_file)?;
        let mut zip = ZipWriter::new(file);

        // Add all files from skill_dir
        add_directory_to_zip(&mut zip, skill_dir, skill_dir)?;

        zip.finish()?;
        info!("Packaged skill to: {}", output_file.display());

        Ok(output_file)
    }

    // Helper methods

    fn validate_skill_name(&self, name: &str) -> Result<()> {
        if !self.is_valid_skill_name(name) {
            return Err(anyhow!(
                "Invalid skill name '{}'. Must be lowercase alphanumeric with hyphens only",
                name
            ));
        }
        Ok(())
    }

    fn is_valid_skill_name(&self, name: &str) -> bool {
        !name.is_empty()
            && name.len() <= 64
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !name.starts_with('-')
            && !name.ends_with('-')
            && !name.contains("--")
            && !name.contains("anthropic")
            && !name.contains("claude")
    }

    fn title_case_skill_name(&self, name: &str) -> String {
        name.split('-')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn create_scripts_dir(&self, skill_dir: &Path, skill_name: &str) -> Result<()> {
        let scripts_dir = skill_dir.join("scripts");
        fs::create_dir(&scripts_dir)?;

        let example_script = scripts_dir.join("example.py");
        let content = EXAMPLE_SCRIPT.replace("{skill_name}", skill_name);
        fs::write(&example_script, content)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&example_script)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&example_script, perms)?;
        }

        info!("Created scripts/example.py");
        Ok(())
    }

    fn create_references_dir(&self, skill_dir: &Path, skill_title: &str) -> Result<()> {
        let references_dir = skill_dir.join("references");
        fs::create_dir(&references_dir)?;

        let reference_file = references_dir.join("api_reference.md");
        let content = EXAMPLE_REFERENCE.replace("{skill_title}", skill_title);
        fs::write(reference_file, content)?;

        info!("Created references/api_reference.md");
        Ok(())
    }

    fn create_assets_dir(&self, skill_dir: &Path) -> Result<()> {
        let assets_dir = skill_dir.join("assets");
        fs::create_dir(&assets_dir)?;

        let placeholder = assets_dir.join(".gitkeep");
        fs::write(
            placeholder,
            "# Place template files, images, icons, etc. here\n",
        )?;

        info!("Created assets/ directory");
        Ok(())
    }

    fn validate_structure(&self, skill_dir: &Path, report: &mut ValidationReport) -> Result<()> {
        // Check for common mistakes
        if skill_dir.join("README.md").exists() {
            report
                .warnings
                .push("README.md found - not needed for skills (use SKILL.md only)".to_string());
        }

        if skill_dir.join("INSTALLATION_GUIDE.md").exists() {
            report.warnings.push(
                "INSTALLATION_GUIDE.md found - installation info should be in SKILL.md".to_string(),
            );
        }

        // Warn about Windows-style paths
        let skill_md_content = fs::read_to_string(skill_dir.join("SKILL.md"))?;
        if skill_md_content.contains('\\') {
            report
                .warnings
                .push("SKILL.md contains backslashes - use forward slashes for paths".to_string());
        }

        Ok(())
    }
}

fn add_directory_to_zip<W: std::io::Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    dir: &Path,
    base: &Path,
) -> Result<()> {
    use std::io::Read;
    use zip::write::SimpleFileOptions;

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path.strip_prefix(base)?;

        if path.is_file() {
            debug!("Adding file: {}", name.display());
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file(name.to_string_lossy().to_string(), options)?;
            let mut file = fs::File::open(&path)?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            zip.write_all(&buffer)?;
        } else if path.is_dir() {
            add_directory_to_zip(zip, &path, base)?;
        }
    }

    Ok(())
}

/// Validation report for skills
#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub skill_dir: PathBuf,
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationReport {
    pub fn new(skill_dir: PathBuf) -> Self {
        Self {
            skill_dir,
            valid: false,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn format(&self) -> String {
        let mut output = format!("Validation Report for: {}\n", self.skill_dir.display());
        output.push_str(&format!(
            "Status: {}\n\n",
            if self.valid {
                "✓ VALID"
            } else {
                "✗ INVALID"
            }
        ));

        if !self.errors.is_empty() {
            output.push_str("Errors:\n");
            for error in &self.errors {
                output.push_str(&format!("  ✗ {}\n", error));
            }
            output.push('\n');
        }

        if !self.warnings.is_empty() {
            output.push_str("Warnings:\n");
            for warning in &self.warnings {
                output.push_str(&format!("  ⚠ {}\n", warning));
            }
        }

        output
    }
}

/// Render skills section for system prompt (Codex-style lean format)
///
/// Only includes name + description + file path. Body stays on disk for progressive disclosure.
pub fn render_skills_lean(skills: &[crate::skills::types::Skill]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    lines.push("## Skills".to_string());
    lines.push("These skills are discovered at startup; each entry shows name, description, scope (user/repo), and file path. Content is not inlined to keep context lean - open the file when needed.".to_string());

    // Group by scope for better organization
    let user_skills: Vec<_> = skills
        .iter()
        .filter(|s| matches!(s.scope, crate::skills::types::SkillScope::User))
        .collect();
    let repo_skills: Vec<_> = skills
        .iter()
        .filter(|s| matches!(s.scope, crate::skills::types::SkillScope::Repo))
        .collect();

    if !user_skills.is_empty() {
        lines.push("\n### User Skills".to_string());
        for skill in user_skills {
            let skill_md_path = skill.path.join("SKILL.md");
            let path_str = skill_md_path.to_string_lossy().replace('\\', "/");
            lines.push(format!(
                "- {}: {} (file: {})",
                skill.name(),
                skill.description(),
                path_str
            ));
        }
    }

    if !repo_skills.is_empty() {
        lines.push("\n### Repository Skills".to_string());
        for skill in repo_skills {
            let skill_md_path = skill.path.join("SKILL.md");
            let path_str = skill_md_path.to_string_lossy().replace('\\', "/");
            lines.push(format!(
                "- {}: {} (file: {})",
                skill.name(),
                skill.description(),
                path_str
            ));
        }
    }

    lines.push(r###"- Discovery: Available skills are listed above (name + description + file path). These are the sources of truth; skill bodies live on disk at the listed paths.
- Trigger rules: If the user names a skill (with `$SkillName` or plain text) OR the task clearly matches a skill's description, use that skill for that turn. Multiple mentions mean use them all. Do not carry skills across turns unless re-mentioned.
- Missing/blocked: If a named skill isn't in the list or the path can't be read, say so briefly and continue with the best fallback.
- How to use a skill (progressive disclosure):
  1) After deciding to use a skill, open its `SKILL.md`. Read only enough to follow the workflow.
  2) If `SKILL.md` points to extra folders such as `references/`, load only the specific files needed for the request.
  3) If `scripts/` exist, prefer running them instead of retyping code.
  4) If `assets/` or templates exist, reuse them.
- Package managers (IMPORTANT - use modern tools):
  • Python: ALWAYS use `uv pip install` (NOT pip). If uv unavailable, use `pip install` as fallback.
  • JavaScript: ALWAYS use `bun install` or `bun add` (NOT npm/pnpm). If bun unavailable, use npm as fallback.
  • Examples: `uv pip install pandas`, `bun add axios`, `uv run python script.py`, `bun run script.js`
- Description as trigger: The YAML `description` in `SKILL.md` is the primary trigger signal.
- Context hygiene: Keep context small - summarize long sections, only load extra files when needed, avoid deeply nested references."###.to_string());

    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::types::Skill;
    use tempfile::TempDir;

    #[test]
    fn test_validate_skill_name() -> anyhow::Result<()> {
        let tmp = TempDir::new().map_err(|e| {
            // Convert TempDir error into a test failure with context
            eprintln!("Failed to create TempDir: {e}");
            e
        })?;
        let author = SkillAuthor::new(tmp.path().to_path_buf());

        // Valid names
        assert!(author.is_valid_skill_name("my-skill"));
        assert!(author.is_valid_skill_name("pdf-analyzer"));
        assert!(author.is_valid_skill_name("skill-123"));
        assert!(author.is_valid_skill_name("a"));
        assert!(author.is_valid_skill_name("skill-v2-beta"));

        // Invalid: uppercase
        assert!(!author.is_valid_skill_name("My-Skill"));
        assert!(!author.is_valid_skill_name("PDF-Analyzer"));

        // Invalid: underscore
        assert!(!author.is_valid_skill_name("my_skill"));
        assert!(!author.is_valid_skill_name("pdf_analyzer"));

        // Invalid: leading/trailing hyphens
        assert!(!author.is_valid_skill_name("-my-skill"));
        assert!(!author.is_valid_skill_name("my-skill-"));
        assert!(!author.is_valid_skill_name("-"));

        // Invalid: consecutive hyphens
        assert!(!author.is_valid_skill_name("my--skill"));
        assert!(!author.is_valid_skill_name("skill---v2"));

        // Invalid: reserved words
        assert!(!author.is_valid_skill_name("anthropic-skill"));
        assert!(!author.is_valid_skill_name("claude-helper"));
        assert!(!author.is_valid_skill_name("anthropic"));
        assert!(!author.is_valid_skill_name("claude"));

        // Invalid: empty or too long
        assert!(!author.is_valid_skill_name(""));
        assert!(!author.is_valid_skill_name(&"a".repeat(65)));

        Ok(())
    }

    #[test]
    fn test_title_case_skill_name() -> anyhow::Result<()> {
        let tmp = TempDir::new().map_err(|e| {
            eprintln!("Failed to create TempDir: {e}");
            e
        })?;
        let author = SkillAuthor::new(tmp.path().to_path_buf());

        assert_eq!(author.title_case_skill_name("my-skill"), "My Skill");
        assert_eq!(author.title_case_skill_name("pdf-analyzer"), "Pdf Analyzer");
        assert_eq!(author.title_case_skill_name("api-helper"), "Api Helper");

        Ok(())
    }

    #[tokio::test]
    async fn test_create_skill() -> anyhow::Result<()> {
        let tmp = TempDir::new().map_err(|e| {
            // Provide context on TempDir failure
            eprintln!("Failed to create TempDir: {e}");
            e
        })?;
        let author = SkillAuthor::new(tmp.path().to_path_buf());

        let skill_dir = author
            .create_skill("test-skill", Some(tmp.path().to_path_buf()))
            .map_err(|e| {
                eprintln!("Failed to write skill file: {e}");
                e
            })?;

        assert!(skill_dir.exists());
        assert!(skill_dir.join("SKILL.md").exists());
        assert!(skill_dir.join("scripts").exists());
        assert!(skill_dir.join("references").exists());
        assert!(skill_dir.join("assets").exists());

        // Verify SKILL.md has correct structure
        let skill_md = fs::read_to_string(skill_dir.join("SKILL.md")).map_err(|e| {
            eprintln!("Failed to read SKILL.md: {e}");
            e
        })?;
        assert!(skill_md.starts_with("---"));
        assert!(skill_md.contains("name: test-skill"));
        assert!(skill_md.contains("# Test Skill"));

        Ok(())
    }

    #[test]
    fn test_validation_report_formatting() {
        let tmp = TempDir::new().unwrap();
        let mut report = ValidationReport::new(tmp.path().to_path_buf());

        report.errors.push("Test error".to_string());
        report.warnings.push("Test warning".to_string());

        let formatted = report.format();
        assert!(formatted.contains("✗ INVALID"));
        assert!(formatted.contains("✗ Test error"));
        assert!(formatted.contains("⚠ Test warning"));

        // Valid report
        report.errors.clear();
        report.valid = true;
        let formatted = report.format();
        assert!(formatted.contains("✓ VALID"));
    }

    #[test]
    fn test_duplicate_skill_creation() {
        let tmp = TempDir::new().unwrap();
        let author = SkillAuthor::new(tmp.path().to_path_buf());

        // Create first skill
        author
            .create_skill("test-skill", Some(tmp.path().to_path_buf()))
            .unwrap();

        // Try to create duplicate - should fail
        let result = author.create_skill("test-skill", Some(tmp.path().to_path_buf()));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn test_render_skills_lean() {
        let tmp = TempDir::new().unwrap();
        let author = SkillAuthor::new(tmp.path().to_path_buf());

        // Create test skills
        let skill1_dir = author
            .create_skill("pdf-analyzer", Some(tmp.path().to_path_buf()))
            .unwrap();
        let skill2_dir = author
            .create_skill("spreadsheet-generator", Some(tmp.path().to_path_buf()))
            .unwrap();

        // Update descriptions to be valid
        let skill1_md = skill1_dir.join("SKILL.md");
        let content1 = fs::read_to_string(&skill1_md).unwrap().replace(
            "[TODO: Complete and informative explanation",
            "Extract text and tables from PDFs",
        );
        fs::write(skill1_md, content1).unwrap();

        let skill2_md = skill2_dir.join("SKILL.md");
        let content2 = fs::read_to_string(&skill2_md).unwrap().replace(
            "[TODO: Complete and informative explanation",
            "Create Excel spreadsheets with charts",
        );
        fs::write(skill2_md, content2).unwrap();

        // Load skills
        use crate::skills::manifest::parse_skill_file;
        let (manifest1, body1) = parse_skill_file(&skill1_dir).unwrap();
        let (manifest2, body2) = parse_skill_file(&skill2_dir).unwrap();

        let skill1 = Skill::new(manifest1, skill1_dir.clone(), body1).unwrap();
        let skill2 = Skill::new(manifest2, skill2_dir.clone(), body2).unwrap();

        // Render lean format
        let rendered = render_skills_lean(&[skill1, skill2]).unwrap();

        // Verify structure
        assert!(rendered.contains("## Skills"));
        assert!(rendered.contains("pdf-analyzer: Extract text and tables from PDFs"));
        assert!(rendered.contains("spreadsheet-generator: Create Excel spreadsheets with charts"));
        assert!(rendered.contains("(file:"));
        assert!(rendered.contains("SKILL.md)"));

        // Verify usage rules present
        assert!(rendered.contains("Trigger rules:"));
        assert!(rendered.contains("$SkillName"));
        assert!(rendered.contains("progressive disclosure"));
        assert!(rendered.contains("Package managers"));
        assert!(rendered.contains("uv pip install"));
        assert!(rendered.contains("bun install"));
        assert!(rendered.contains("Context hygiene"));

        // Verify token efficiency - should be much smaller than full content
        // Lean format: ~200 tokens per skill + 400 for rules (with package manager prefs) = ~800 total
        // Full format would be 5K+ tokens per skill
        assert!(rendered.len() < 2500, "Lean rendering should be compact");
    }

    #[test]
    fn test_render_skills_lean_empty() {
        let rendered = render_skills_lean(&[]);
        assert!(rendered.is_none());
    }
}
