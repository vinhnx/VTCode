//! CLI command handlers for agent skills management
//!
//! Provides `/skills` command palette for discovering, loading, and managing
//! Anthropic Agent Skills within VT Code.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::info;
use vtcode_core::skills::loader::EnhancedSkillLoader;
use vtcode_core::skills::manifest::generate_skill_template;

use crate::cli::SkillsCommandOptions;

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
                println!(
                    "  2. Skills marked with   work but require following fallback instructions"
                );
                println!(
                    "  3. For incompatible skills, use the suggested Python libraries with execute_code"
                );
                println!("  4. Check 'vtcode skills info <name>' for detailed compatibility info");
            }
        }
        Err(e) => {
            println!(" Failed to generate validation report: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

/// List available skills (OpenAI-style listing)
pub async fn handle_skills_list(options: &SkillsCommandOptions) -> Result<()> {
    let mut loader = EnhancedSkillLoader::new(options.workspace.clone());

    println!("Discovering skills from standard locations...\n");

    let discovery_result = loader
        .discover_all_skills()
        .await
        .context("Failed to discover skills")?;
    let skills = discovery_result.skills;
    let cli_tools = discovery_result.tools;

    // Regenerate the skills index to ensure it's up to date with any newly added skills
    {
        use vtcode_core::exec::skill_manager::SkillManager;
        let skill_manager = SkillManager::new(&options.workspace);
        if let Err(e) = skill_manager.generate_index().await {
            eprintln!("Warning: Failed to regenerate skills index: {}", e);
        }
    }

    if skills.is_empty() && cli_tools.is_empty() {
        println!("No skills found.");
        println!("\nCreate a traditional skill:");
        println!("  vtcode skills create ./my-skill");
        println!("\nOr install skills in standard locations:");
        println!("  ~/.vtcode/skills/     (VT Code user skills)");
        println!("  .agents/skills/       (Project skills)");
        println!("  .vtcode/skills/       (Legacy project skills - deprecated)");
        println!("  ~/.claude/skills/     (Legacy compatibility)");
        println!("  ~/.codex/skills/      (Codex compatibility)");
        return Ok(());
    }

    // List traditional skills (OpenAI-style)
    if !skills.is_empty() {
        println!("Available Traditional Skills:");
        println!("{:-<70}", "");

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
                                    warnings.push(format!("❌ {} - Requires container skills (not compatible)", manifest.name));
                                    "❌"
                                }
                                vtcode_core::skills::container_validation::ContainerSkillsRequirement::RequiredWithFallback => {
                                    warnings.push(format!("⚠️  {} - Has container skills fallback", manifest.name));
                                    "⚠️"
                                }
                                _ => "✓",
                            };

                            let mode_suffix = if manifest.mode.unwrap_or(false) {
                                " [mode]"
                            } else {
                                ""
                            };

                            println!(
                                "{} {}{}\n  {}\n",
                                status_indicator, manifest.name, mode_suffix, manifest.description
                            );
                        }
                        vtcode_core::skills::loader::EnhancedSkill::CliTool(_) => {
                            // CLI tools handled separately below
                        }
                    }
                }
                Err(_) => {
                    // Skill failed to load, likely due to container skills validation
                    warnings.push(format!(
                        "❌ {} - Requires container skills (validation failed)",
                        manifest.name
                    ));
                    println!("❌ {}\n  {}\n", manifest.name, manifest.description);
                }
            }
        }

        if !warnings.is_empty() {
            println!("\n⚠️  Compatibility Notes:");
            for warning in warnings {
                println!("  {}", warning);
            }
            println!("\n  Use 'vtcode skills info <name>' for details and alternatives.");
        }
    }

    // List CLI tools separately
    if !cli_tools.is_empty() {
        println!("\nAvailable CLI Tool Skills:");
        println!("{:-<70}", "");

        for tool in &cli_tools {
            println!(
                "⚡ {}\n  {}\n  Path: {}\n",
                tool.name,
                tool.description,
                tool.executable_path.display()
            );
        }
    }

    println!("\n💡 Usage:");
    println!("  Load skill:    vtcode skills load <name>");
    println!("  Skill info:    vtcode skills info <name>");
    println!("  Use in chat:   /skills load <name>");
    println!("  Or:            /skills use <name> <input>");
    Ok(())
}

/// Load a skill for current session
pub async fn handle_skills_load(
    options: &SkillsCommandOptions,
    name: &str,
    _path: Option<PathBuf>,
) -> Result<()> {
    let mut loader = EnhancedSkillLoader::new(options.workspace.clone());

    // Regenerate the skills index to ensure it's up to date with any newly added skills
    {
        use vtcode_core::exec::skill_manager::SkillManager;
        let skill_manager = SkillManager::new(&options.workspace);
        if let Err(e) = skill_manager.generate_index().await {
            eprintln!("Warning: Failed to regenerate skills index: {}", e);
        }
    }

    println!("Loading skill: {}...", name);

    // Ensure skills are discovered before loading
    loader
        .discover_all_skills()
        .await
        .context("Failed to discover skills")?;

    let skill = loader
        .get_skill(name)
        .await
        .context(format!("Failed to load skill '{}'", name))?;

    match skill {
        vtcode_core::skills::loader::EnhancedSkill::Traditional(skill) => {
            println!(
                "Loaded skill: {} (v{})",
                skill.name(),
                skill.manifest.version.as_deref().unwrap_or("0.0.1")
            );
            println!("  Description: {}", skill.description());
            println!("  Resources: {} files", skill.list_resources().len());
        }
        vtcode_core::skills::loader::EnhancedSkill::CliTool(bridge) => {
            println!("Loaded CLI tool skill: {}", bridge.config.name);
            println!("  Description: {}", bridge.config.description);
        }
    }

    println!(
        "\nSkill is ready to use. Use it in chat mode or with: vtcode ask 'Use {} for...'",
        name
    );

    info!("Loaded skill: {}", name);
    Ok(())
}

/// Show skill details
pub async fn handle_skills_info(options: &SkillsCommandOptions, name: &str) -> Result<()> {
    let mut loader = EnhancedSkillLoader::new(options.workspace.clone());

    // Regenerate the skills index to ensure it's up to date with any newly added skills
    {
        use vtcode_core::exec::skill_manager::SkillManager;
        let skill_manager = SkillManager::new(&options.workspace);
        if let Err(e) = skill_manager.generate_index().await {
            eprintln!("Warning: Failed to regenerate skills index: {}", e);
        }
    }

    println!("Loading skill: {}...\n", name);

    // Ensure skills are discovered before loading
    loader
        .discover_all_skills()
        .await
        .context("Failed to discover skills")?;

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
            if let Some(license) = &skill.manifest.license {
                println!("License: {}", license);
            }
            if let Some(model) = &skill.manifest.model {
                println!("Model: {}", model);
            }
            if let Some(mode) = skill.manifest.mode {
                println!("Mode command: {}", mode);
            }
            if let Some(when_to_use) = &skill.manifest.when_to_use {
                println!("When to use: {}", when_to_use);
            }
            if let Some(allowed_tools) = &skill.manifest.allowed_tools
                && !allowed_tools.is_empty()
            {
                println!("Allowed tools: {}", allowed_tools);
            }
            if let Some(disable) = skill.manifest.disable_model_invocation {
                println!("Disable model invocation: {}", disable);
            }
            if let Some(req) = skill.manifest.requires_container {
                println!("Requires container: {}", req);
            }
            if let Some(disallow) = skill.manifest.disallow_container {
                println!("Disallow container: {}", disallow);
            }

            // Add container skills compatibility check
            let analysis = loader.check_container_requirements(&skill);
            println!("\n--- Compatibility ---");
            match analysis.requirement {
                vtcode_core::skills::container_validation::ContainerSkillsRequirement::Required => {
                    println!(" Requires Anthropic container skills - NOT COMPATIBLE with VT Code");
                }
                vtcode_core::skills::container_validation::ContainerSkillsRequirement::RequiredWithFallback => {
                    println!("  Uses container skills but provides VT Code-compatible alternatives");
                }
                vtcode_core::skills::container_validation::ContainerSkillsRequirement::NotRequired => {
                    println!(" Fully compatible with VT Code");
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
pub async fn handle_skills_validate(skill_path: &Path) -> Result<()> {
    use vtcode_core::skills::enhanced_validator::ComprehensiveSkillValidator;
    use vtcode_core::skills::manifest::parse_skill_file;

    println!("Validating skill at: {}\n", skill_path.display());

    let (manifest, instructions) = parse_skill_file(skill_path)?;

    // Run basic validation first
    manifest.validate()?;

    // Run comprehensive validation with detailed report
    let validator = ComprehensiveSkillValidator::new();
    let mut report = validator.validate_manifest(&manifest, skill_path);

    // Also validate file references if we have instructions
    if !instructions.is_empty() {
        validator.validate_file_references(&manifest, skill_path, &instructions, &mut report);
    }

    report.finalize();

    // Print detailed report
    println!("{}", report.generate_summary());

    if report.is_valid {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Skill validation failed with {} errors",
            report.stats.error_count
        ))
    }
}

/// Show skill configuration
pub async fn handle_skills_config(options: &SkillsCommandOptions) -> Result<()> {
    println!("Skill Configuration\n");
    println!("Workspace: {}", options.workspace.display());
    println!("\nSkill Search Paths (by precedence):");
    println!(
        "  • .github/skills/       (Agent Skills spec recommended - highest project precedence)"
    );
    println!("  • .agents/skills/       (VT Code native project skills)");
    println!("  • .vtcode/skills/       (Legacy VT Code project skills)");
    println!("  • .claude/skills/       (Claude Code legacy compatibility)");
    println!("  • .pi/skills/           (Pi framework project skills)");
    println!("  • .codex/skills/        (Codex compatibility)");
    println!("  • ./skills              (Generic project skills)");
    println!("  • ~/.vtcode/skills/     (VT Code user skills)");
    println!("  • ~/.copilot/skills/    (VS Code Copilot compatibility)");
    println!("  • ~/.claude/skills/     (Claude Code user compatibility)");
    println!("  • ~/.pi/agent/skills/   (Pi framework user skills)");
    println!("  • ~/.codex/skills/      (Codex user compatibility - lowest precedence)");

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

/// Regenerate skills index file
pub async fn handle_skills_regenerate_index(options: &SkillsCommandOptions) -> Result<()> {
    println!("Regenerating skills index...\n");

    match generate_comprehensive_skills_index(&options.workspace).await {
        Ok(index_path) => {
            // Also use EnhancedSkillLoader to get all discoverable skills for display
            use vtcode_core::skills::loader::EnhancedSkillLoader;
            let mut loader = EnhancedSkillLoader::new(options.workspace.clone());

            match loader.discover_all_skills().await {
                Ok(discovery_result) => {
                    // Count total skills
                    let total_skills = discovery_result.skills.len() + discovery_result.tools.len();

                    println!("Skills index regenerated successfully!");
                    println!("Index file: {}", index_path.display());
                    println!(
                        "Found {} skills (traditional: {}, CLI tools: {})",
                        total_skills,
                        discovery_result.skills.len(),
                        discovery_result.tools.len()
                    );

                    if !discovery_result.skills.is_empty() {
                        println!("\nTraditional skills:");
                        for skill_ctx in &discovery_result.skills {
                            let manifest = skill_ctx.manifest();
                            println!("   - {} - {}", manifest.name, manifest.description);
                        }
                    }

                    if !discovery_result.tools.is_empty() {
                        println!("\nCLI tool skills:");
                        for tool in &discovery_result.tools {
                            println!("   - {} - {}", tool.name, tool.description);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Could not list skills: {}", e);
                    println!("Skills index regenerated successfully!");
                    println!("Index file: {}", index_path.display());
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to regenerate skills index: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

/// Generate comprehensive skills index including all types of skills
pub async fn generate_comprehensive_skills_index(workspace: &Path) -> Result<std::path::PathBuf> {
    use std::fmt::Write;
    use vtcode_core::exec::skill_manager::SkillManager;
    use vtcode_core::skills::loader::EnhancedSkillLoader;

    let skill_manager = SkillManager::new(workspace);
    let mut loader = EnhancedSkillLoader::new(workspace.to_path_buf());

    // Discover all skills using EnhancedSkillLoader
    let discovery_result = loader.discover_all_skills().await?;

    // Build comprehensive index content
    let mut content = String::new();
    content.push_str("# Skills Index\n\n");
    content.push_str("This file lists all available skills for dynamic discovery.\n");
    content.push_str("Use `read_file` on individual skill directories for full documentation.\n\n");

    let total_skills = discovery_result.skills.len() + discovery_result.tools.len();

    if total_skills == 0 {
        content.push_str("*No skills available yet.*\n\n");
        content.push_str("Create skills using the `save_skill` tool.\n");
    } else {
        content.push_str(&format!("## Available Skills ({} total)\n\n", total_skills));

        // Add traditional skills section
        if !discovery_result.skills.is_empty() {
            content.push_str("### Traditional Skills\n\n");
            content.push_str("| Name | Description | Type |\n");
            content.push_str("|------|-------------|------|\n");

            for skill_ctx in &discovery_result.skills {
                let manifest = skill_ctx.manifest();
                let desc = manifest.description.replace('|', "\\|");
                let skill_type = if manifest.mode.unwrap_or(false) {
                    "Mode"
                } else {
                    "Skill"
                };
                let _ = writeln!(
                    content,
                    "| `{}` | {} | {} |",
                    manifest.name, desc, skill_type
                );
            }
            content.push('\n');
        }

        // Add CLI tools section
        if !discovery_result.tools.is_empty() {
            content.push_str("### CLI Tool Skills\n\n");
            content.push_str("| Name | Description | Executable |\n");
            content.push_str("|------|-------------|------------|\n");

            for tool in &discovery_result.tools {
                let desc = tool.description.replace('|', "\\|");
                let _ = writeln!(
                    content,
                    "| `{}` | {} | `{}` |",
                    tool.name,
                    desc,
                    tool.executable_path.display()
                );
            }
            content.push('\n');
        }

        // Add quick reference section
        content.push_str("## Quick Reference\n\n");

        // Traditional skills quick reference
        for skill_ctx in &discovery_result.skills {
            let manifest = skill_ctx.manifest();
            let skill_md = skill_ctx.path().join("SKILL.md");
            let _ = writeln!(content, "### {}\n", manifest.name);
            let _ = writeln!(content, "{}\n", manifest.description);
            let _ = writeln!(
                content,
                "- **Type**: {}\n- **Path**: `{}`\n",
                if manifest.mode.unwrap_or(false) {
                    "Mode"
                } else {
                    "Skill"
                },
                skill_md.display()
            );
        }

        // CLI tools quick reference
        for tool in &discovery_result.tools {
            let _ = writeln!(content, "### {}\n", tool.name);
            let _ = writeln!(content, "{}\n", tool.description);
            let _ = writeln!(
                content,
                "- **Type**: CLI Tool\n- **Executable**: `{}`\n",
                tool.executable_path.display()
            );
        }
    }

    content.push_str("\n---\n");
    content.push_str("*Generated automatically. Do not edit manually.*\n");

    // Write to the skills index file
    let index_path = skill_manager.index_path();
    tokio::fs::write(&index_path, &content)
        .await
        .with_context(|| {
            format!(
                "Failed to write comprehensive skills index: {}",
                index_path.display()
            )
        })?;

    Ok(index_path)
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
