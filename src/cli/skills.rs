//! CLI command handlers for agent skills management
//!
//! Provides `/skills` command palette for discovering, loading, and managing
//! Anthropic Agent Skills within VTCode.

use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing::info;
use vtcode_core::skills::loader::EnhancedSkillLoader;
use vtcode_core::skills::manifest::generate_skill_template;

/// Skills command options
#[derive(Debug)]
pub struct SkillsCommandOptions {
    pub workspace: PathBuf,
}

/// Generate a comprehensive validation report
pub async fn handle_skills_validate_all(options: &SkillsCommandOptions) -> Result<()> {
    let mut loader = EnhancedSkillLoader::new(options.workspace.clone());

    println!(" Generating comprehensive container skills validation report...\n");

    match loader.generate_validation_report().await {
        Ok(report) => {
            println!("{}", report.format_report());
            
            if !report.incompatible_skills.is_empty() {
                println!("\n Next Steps:");
                println!("  1. Use skills marked with  for guaranteed compatibility");
                println!("  2. Skills marked with   work but require following fallback instructions");
                println!("  3. For incompatible skills, use the suggested Python libraries with execute_code");
                println!("  4. Check 'vtcode skills info <name>' for detailed compatibility info");
            }
        }
        Err(e) => {
            println!(" Failed to generate validation report: {}", e);
            return Err(e.into());
        }
    }

    Ok(())
}

/// List available skills
pub async fn handle_skills_list(options: &SkillsCommandOptions) -> Result<()> {
    let mut loader = EnhancedSkillLoader::new(options.workspace.clone());

    println!("Discovering skills...\n");

    let discovery_result = loader.discover_all_skills().await.context("Failed to discover skills")?;
    let skills = discovery_result.traditional_skills;

    if skills.is_empty() {
        println!("No skills found. Create one with: vtcode skills create ./my-skill");
        return Ok(());
    }

    println!("Available Skills:");
    println!("{:<30} | {}", "Name", "Description");
    println!("{:-<30}-+-{:-<60}", "", "");

    // Track skills that need warnings
    let mut warnings = Vec::new();

    for skill_ctx in &skills {
        let manifest = skill_ctx.manifest();
        
        // Quick validation check for display
        let mut temp_loader = EnhancedSkillLoader::new(options.workspace.clone());
        match temp_loader.get_skill(&manifest.name).await {
            Ok(enhanced_skill) => {
                match enhanced_skill {
                    vtcode_core::skills::loader::EnhancedSkill::Traditional(skill) => {
                        let analysis = temp_loader.check_container_requirements(&skill);
                        
                        let status_indicator = match analysis.requirement {
                            vtcode_core::skills::container_validation::ContainerSkillsRequirement::Required => {
                                warnings.push(format!(" {} - Requires container skills (not compatible)", manifest.name));
                                ""
                            }
                            vtcode_core::skills::container_validation::ContainerSkillsRequirement::RequiredWithFallback => {
                                warnings.push(format!("  {} - Has container skills fallback", manifest.name));
                                ""
                            }
                            _ => "",
                        };
                        
                        println!("{} {:<28} | {}", status_indicator, manifest.name, manifest.description);
                    }
                    vtcode_core::skills::loader::EnhancedSkill::CliTool(_) => {
                        println!(" {:<30} | {}", manifest.name, manifest.description);
                    }
                }
            }
            Err(_) => {
                // Skill failed to load, likely due to container skills validation
                warnings.push(format!(" {} - Requires container skills (validation failed)", manifest.name));
                println!(" {:<28} | {}", manifest.name, manifest.description);
            }
        }
    }

    if !warnings.is_empty() {
        println!("\n  Container Skills Compatibility Warnings:");
        for warning in warnings {
            println!("  {}", warning);
        }
        println!("\n Use 'vtcode skills info <name>' to see compatibility details and alternatives.");
    }

    println!("\nUse 'vtcode skills info <name>' for details");
    Ok(())
}

/// Load a skill for current session
pub async fn handle_skills_load(
    options: &SkillsCommandOptions,
    name: &str,
    _path: Option<PathBuf>,
) -> Result<()> {
    let mut loader = EnhancedSkillLoader::new(options.workspace.clone());

    println!("Loading skill: {}...", name);
    
    // Ensure skills are discovered before loading
    loader.discover_all_skills().await.context("Failed to discover skills")?;

    let skill = loader
        .get_skill(name)
        .await
        .context(format!("Failed to load skill '{}'", name))?;

    match skill {
        vtcode_core::skills::loader::EnhancedSkill::Traditional(skill) => {
            println!("Loaded skill: {} (v{})", skill.name(), skill.manifest.version.as_deref().unwrap_or("0.0.1"));
            println!("  Description: {}", skill.description());
            println!("  Resources: {} files", skill.list_resources().len());
        }
        vtcode_core::skills::loader::EnhancedSkill::CliTool(bridge) => {
            println!("Loaded CLI tool skill: {}", bridge.config.name);
            println!("  Description: {}", bridge.config.description);
        }
    }
    
    println!("\nSkill is ready to use. Use it in chat mode or with: vtcode ask 'Use {} for...'", name);

    info!("Loaded skill: {}", name);
    Ok(())
}

/// Show skill details
pub async fn handle_skills_info(options: &SkillsCommandOptions, name: &str) -> Result<()> {
    let mut loader = EnhancedSkillLoader::new(options.workspace.clone());

    println!("Loading skill: {}...\n", name);
    
    // Ensure skills are discovered before loading
    loader.discover_all_skills().await.context("Failed to discover skills")?;

    let skill = loader
        .get_skill(name)
        .await
        .context(format!("Failed to load skill '{}'", name))?;

    match skill {
        vtcode_core::skills::loader::EnhancedSkill::Traditional(skill) => {
            println!("Skill: {}", skill.name());
            println!("Description: {}", skill.description());
            if let Some(version) = &skill.manifest.version {
                println!("Version: {}", version);
            }
            if let Some(author) = &skill.manifest.author {
                println!("Author: {}", author);
            }

            // Add container skills compatibility check
            let analysis = loader.check_container_requirements(&skill);
            println!("\n--- Compatibility ---");
            match analysis.requirement {
                vtcode_core::skills::container_validation::ContainerSkillsRequirement::Required => {
                    println!(" Requires Anthropic container skills - NOT COMPATIBLE with VTCode");
                }
                vtcode_core::skills::container_validation::ContainerSkillsRequirement::RequiredWithFallback => {
                    println!("  Uses container skills but provides VTCode-compatible alternatives");
                }
                vtcode_core::skills::container_validation::ContainerSkillsRequirement::NotRequired => {
                    println!(" Fully compatible with VTCode");
                }
                vtcode_core::skills::container_validation::ContainerSkillsRequirement::Unknown => {
                    println!(" Compatibility unknown - proceed with caution");
                }
            }
            
            if !analysis.recommendations.is_empty() {
                println!("\n--- Recommendations ---");
                for rec in &analysis.recommendations {
                    println!("{}", rec);
                }
            }

            println!("\n--- Instructions ---");
            println!("{}", skill.instructions);

            if !skill.list_resources().is_empty() {
                println!("\n--- Available Resources ---");
                for resource in skill.list_resources() {
                    println!("  • {}", resource);
                }
            }
        }
        vtcode_core::skills::loader::EnhancedSkill::CliTool(bridge) => {
            println!("CLI Tool Skill: {}", bridge.config.name);
            println!("Description: {}", bridge.config.description);
            println!("\n--- Tool Configuration ---");
            println!("Tool available for execution");
        }
    }

    Ok(())
}

/// Create skill template
pub async fn handle_skills_create(skill_path: &PathBuf) -> Result<()> {
    use std::fs;

    if skill_path.exists() {
        anyhow::bail!("Skill path already exists: {}", skill_path.display());
    }

    let skill_name = skill_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my-skill");

    fs::create_dir_all(skill_path).context("Failed to create skill directory")?;

    // Generate template
    let template = generate_skill_template(skill_name, "Brief description of what this skill does");

    // Write SKILL.md
    let skill_md = skill_path.join("SKILL.md");
    fs::write(&skill_md, template).context("Failed to write SKILL.md")?;

    // Create scripts directory
    fs::create_dir(skill_path.join("scripts")).ok(); // Optional

    println!("Created skill template at: {}", skill_path.display());
    println!("  • SKILL.md - Skill metadata and instructions");
    println!("  • scripts/ - Optional: executable scripts");
    println!("\nNext steps:");
    println!("  1. Edit SKILL.md with your skill details");
    println!("  2. Add scripts to scripts/ if needed");
    println!("  3. Load with: vtcode skills load {}", skill_name);

    Ok(())
}

/// Validate SKILL.md
pub async fn handle_skills_validate(skill_path: &PathBuf) -> Result<()> {
    use vtcode_core::skills::manifest::parse_skill_file;

    println!("Validating skill at: {}", skill_path.display());

    let (manifest, _instructions) = parse_skill_file(skill_path)?;

    manifest.validate()?;

    println!("SKILL.md is valid");
    println!("  Name: {}", manifest.name);
    println!("  Description: {}", manifest.description);
    if let Some(version) = &manifest.version {
        println!("  Version: {}", version);
    }

    Ok(())
}

/// Show skill configuration
pub async fn handle_skills_config(options: &SkillsCommandOptions) -> Result<()> {
    println!("Skill Configuration\n");
    println!("Workspace: {}", options.workspace.display());
    println!("\nSkill Search Paths (by precedence):");
    println!("  • ~/.vtcode/skills/     (VTCode user skills - highest precedence)");
    println!("  • .vtcode/skills/       (VTCode project skills)");
    println!("  • ~/.pi/skills/         (Pi framework user skills)");
    println!("  • .pi/skills/           (Pi framework project skills)");
    println!("  • ~/.claude/skills/     (Claude Code user skills)");
    println!("  • .claude/skills/       (Claude Code project skills)");
    println!("  • ~/.codex/skills/      (Codex CLI user skills - lowest precedence)");

    println!("\nSkill Directory Structure:");
    println!("  my-skill/");
    println!("    ├── SKILL.md          (required: metadata + instructions)");
    println!("    ├── ADVANCED.md       (optional: additional guides)");
    println!("    ├── scripts/          (optional: executable scripts)");
    println!("    └── templates/        (optional: reference materials)");

    println!("\nEnvironment Variables:");
    println!("  • HOME - Used to locate user skill directories");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_skills_list_command() {
        let temp_dir = TempDir::new().unwrap();
        let options = SkillsCommandOptions {
            workspace: temp_dir.path().to_path_buf(),
        };

        // Should handle empty skills gracefully
        let result = handle_skills_list(&options).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_skills_create_command() {
        let temp_dir = TempDir::new().unwrap();
        let skill_path = temp_dir.path().join("test-skill");

        let result = handle_skills_create(&skill_path).await;
        assert!(result.is_ok());
        assert!(skill_path.join("SKILL.md").exists());
    }

    #[tokio::test]
    async fn test_skills_validate_command() {
        let temp_dir = TempDir::new().unwrap();

        // Create a valid skill
        let skill_path = temp_dir.path().join("test-skill");
        std::fs::create_dir(&skill_path).unwrap();

        let skill_content = r#"---
name: test-skill
description: A test skill
---

# Test Skill
## Instructions
Test instructions
"#;

        std::fs::write(skill_path.join("SKILL.md"), skill_content).unwrap();

        let result = handle_skills_validate(&skill_path).await;
        assert!(result.is_ok());
    }
}
