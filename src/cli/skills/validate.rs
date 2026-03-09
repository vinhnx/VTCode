use anyhow::{Result, anyhow};
use std::path::Path;
use vtcode_core::skills::enhanced_validator::ComprehensiveSkillValidator;
use vtcode_core::skills::loader::EnhancedSkillLoader;
use vtcode_core::skills::manifest::parse_skill_file;

use crate::cli::SkillsCommandOptions;

/// Generate a comprehensive validation report.
pub async fn handle_skills_validate_all(options: &SkillsCommandOptions) -> Result<()> {
    let mut loader = EnhancedSkillLoader::new(options.workspace.clone());

    println!(" Generating comprehensive container skills validation report...\n");

    match loader.generate_validation_report().await {
        Ok(report) => {
            println!("{}", report.format_report());

            if !report.incompatible_skills.is_empty() {
                println!("\n Next Steps:");
                println!("  1. Use skills marked with  for guaranteed compatibility");
                println!(
                    "  2. Skills marked with   work but require following fallback instructions"
                );
                println!(
                    "  3. For incompatible skills, use the suggested Python libraries with `unified_exec` (action='code')"
                );
                println!("  4. Check 'vtcode skills info <name>' for detailed compatibility info");
            }
        }
        Err(err) => {
            println!(" Failed to generate validation report: {}", err);
            return Err(err);
        }
    }

    Ok(())
}

/// Validate SKILL.md.
pub async fn handle_skills_validate(skill_path: &Path, strict: bool) -> Result<()> {
    println!("Validating skill at: {}\n", skill_path.display());

    let (manifest, instructions) = parse_skill_file(skill_path)?;
    manifest.validate()?;

    let validator = if strict {
        ComprehensiveSkillValidator::strict()
    } else {
        ComprehensiveSkillValidator::new()
    };
    let mut report = validator.validate_manifest(&manifest, skill_path);

    if !instructions.is_empty() {
        validator.validate_file_references(&manifest, skill_path, &instructions, &mut report);
    }

    report.finalize();
    println!("{}", report.generate_summary());

    if report.is_valid {
        Ok(())
    } else {
        Err(anyhow!(
            "Skill validation failed with {} errors",
            report.stats.error_count
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn validates_skill_manifest() {
        let temp_dir = TempDir::new().expect("temp dir");
        let skill_path = temp_dir.path().join("test-skill");
        std::fs::create_dir(&skill_path).expect("create skill dir");

        let skill_content = r#"---
name: test-skill
description: A test skill
---

# Test Skill
## Instructions
Test instructions
"#;

        std::fs::write(skill_path.join("SKILL.md"), skill_content).expect("write skill");

        let result = handle_skills_validate(&skill_path, false).await;

        assert!(result.is_ok());
    }
}
