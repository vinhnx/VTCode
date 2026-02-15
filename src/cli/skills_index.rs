use std::path::Path;

use anyhow::{Context, Result};

use crate::cli::SkillsCommandOptions;

/// Regenerate skills index file
pub async fn handle_skills_regenerate_index(options: &SkillsCommandOptions) -> Result<()> {
    println!("Regenerating skills index...\n");

    match generate_comprehensive_skills_index(&options.workspace).await {
        Ok(index_path) => {
            use vtcode_core::skills::loader::EnhancedSkillLoader;
            let mut loader = EnhancedSkillLoader::new(options.workspace.clone());

            match loader.discover_all_skills().await {
                Ok(discovery_result) => {
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
    let discovery_result = loader.discover_all_skills().await?;

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

        content.push_str("## Quick Reference\n\n");

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

    let index_path = skill_manager.index_path();
    vtcode_core::utils::file_utils::write_file_with_context(
        &index_path,
        &content,
        "comprehensive skills index",
    )
    .await
    .with_context(|| {
        format!(
            "Failed to write comprehensive skills index: {}",
            index_path.display()
        )
    })?;

    Ok(index_path)
}
