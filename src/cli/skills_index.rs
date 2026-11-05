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
                    let built_in_count = discovery_result
                        .skills
                        .iter()
                        .filter(|skill_ctx| {
                            skill_ctx.manifest().variety == vtcode_core::skills::types::SkillVariety::BuiltIn
                        })
                        .count();
                    let traditional_count = discovery_result.skills.len() - built_in_count;

                    println!("Skills index regenerated successfully!");
                    println!("Index file: {}", index_path.display());
                    println!(
                        "Found {} skills (traditional: {}, built-in: {}, CLI tools: {})",
                        total_skills,
                        traditional_count,
                        built_in_count,
                        discovery_result.tools.len()
                    );

                    if traditional_count > 0 {
                        println!("\nTraditional skills:");
                        for skill_ctx in &discovery_result.skills {
                            let manifest = skill_ctx.manifest();
                            if manifest.variety == vtcode_core::skills::types::SkillVariety::BuiltIn {
                                continue;
                            }
                            println!("   - {} - {}", manifest.name, manifest.description);
                        }
                    }

                    if built_in_count > 0 {
                        println!("\nBuilt-in command skills:");
                        for skill_ctx in &discovery_result.skills {
                            let manifest = skill_ctx.manifest();
                            if manifest.variety != vtcode_core::skills::types::SkillVariety::BuiltIn {
                                continue;
                            }
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
                    tracing::warn!("Could not list skills: {e}");
                    println!("Skills index regenerated successfully!");
                    println!("Index file: {}", index_path.display());
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to regenerate skills index: {e}");
            return Err(e);
        }
    }

    Ok(())
}

/// Render a skill path for the index without machine-local prefixes: workspace
/// skills relative to the workspace, user-scope skills under `~/`.
fn index_display_path(path: &Path, workspace: &Path, home: Option<&Path>) -> String {
    if let Ok(relative) = path.strip_prefix(workspace) {
        return relative.display().to_string();
    }
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

/// Generate comprehensive skills index including all types of skills
pub async fn generate_comprehensive_skills_index(workspace: &Path) -> Result<std::path::PathBuf> {
    use std::fmt::Write;
    use vtcode_core::exec::skill_manager::{
        SKILLS_INDEX_LINT_DIRECTIVE, SkillManager, finish_skills_index, push_index_table_header,
    };
    use vtcode_core::skills::loader::EnhancedSkillLoader;

    let skill_manager = SkillManager::new(workspace);
    let mut loader = EnhancedSkillLoader::new(workspace.to_path_buf());
    let discovery_result = loader.discover_all_skills().await?;

    let mut content = String::from(SKILLS_INDEX_LINT_DIRECTIVE);
    content.push_str("# Skills Index\n\n");
    content.push_str("This file lists all available skills for dynamic discovery.\n");
    content.push_str("Use `read_file` on individual skill directories for full documentation.\n\n");

    let total_skills = discovery_result.skills.len() + discovery_result.tools.len();

    if total_skills == 0 {
        content.push_str("*No skills available yet.*\n\n");
        content.push_str("Create skills using the `save_skill` tool.\n");
    } else {
        content.push_str(&format!("## Available Skills ({total_skills} total)\n\n"));

        if !discovery_result.skills.is_empty() {
            content.push_str("### Traditional Skills\n\n");
            push_index_table_header(&mut content, &["Name", "Description", "Type"]);

            for skill_ctx in &discovery_result.skills {
                let manifest = skill_ctx.manifest();
                if manifest.variety == vtcode_core::skills::types::SkillVariety::BuiltIn {
                    continue;
                }
                let desc = manifest.description.replace('|', "\\|");
                let _ = writeln!(content, "| `{}` | {} | Skill |", manifest.name, desc);
            }
            content.push('\n');
        }

        let built_in_skills = discovery_result
            .skills
            .iter()
            .filter(|skill_ctx| skill_ctx.manifest().variety == vtcode_core::skills::types::SkillVariety::BuiltIn)
            .collect::<Vec<_>>();
        if !built_in_skills.is_empty() {
            content.push_str("### Built-In Command Skills\n\n");
            push_index_table_header(&mut content, &["Name", "Description", "Slash Alias"]);

            for skill_ctx in &built_in_skills {
                let manifest = skill_ctx.manifest();
                let desc = manifest.description.replace('|', "\\|");
                let slash_alias = manifest
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("slash_alias"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("-");
                let _ = writeln!(content, "| `{}` | {} | `{}` |", manifest.name, desc, slash_alias);
            }
            content.push('\n');
        }

        if !discovery_result.tools.is_empty() {
            content.push_str("### CLI Tool Skills\n\n");
            push_index_table_header(&mut content, &["Name", "Description", "Executable"]);

            for tool in &discovery_result.tools {
                let desc = tool.description.replace('|', "\\|");
                let _ = writeln!(content, "| `{}` | {} | `{}` |", tool.name, desc, tool.executable_path.display());
            }
            content.push('\n');
        }

        content.push_str("## Quick Reference\n\n");

        for skill_ctx in &discovery_result.skills {
            let manifest = skill_ctx.manifest();
            let _ = writeln!(content, "### {}\n", manifest.name);
            let _ = writeln!(content, "{}\n", manifest.description);
            if manifest.variety == vtcode_core::skills::types::SkillVariety::BuiltIn {
                let slash_alias = manifest
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("slash_alias"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("-");
                let usage = manifest
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("usage"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("-");
                let _ = writeln!(
                    content,
                    "- **Type**: Built-in command skill\n- **Slash Alias**: `{slash_alias}`\n- **Usage**: `{usage}`\n"
                );
            } else {
                let skill_md = skill_ctx.path().join("SKILL.md");
                let shown = index_display_path(&skill_md, workspace, dirs::home_dir().as_deref());
                let _ = writeln!(content, "- **Type**: Skill\n- **Path**: `{shown}`\n");
            }
        }

        for tool in &discovery_result.tools {
            let _ = writeln!(content, "### {}\n", tool.name);
            let _ = writeln!(content, "{}\n", tool.description);
            let _ = writeln!(content, "- **Type**: CLI Tool\n- **Executable**: `{}`\n", tool.executable_path.display());
        }
    }

    finish_skills_index(&mut content);

    let index_path = skill_manager.index_path();
    vtcode_core::utils::file_utils::write_file_with_context(&index_path, &content, "comprehensive skills index")
        .await
        .with_context(|| format!("Failed to write comprehensive skills index: {}", index_path.display()))?;

    Ok(index_path)
}

#[cfg(test)]
mod tests {
    use super::index_display_path;
    use std::path::Path;

    #[test]
    fn index_paths_drop_machine_local_prefixes() {
        let workspace = Path::new("/home/dev/repo");
        let home = Some(Path::new("/home/dev"));
        let cases = [
            ("/home/dev/repo/.agents/skills/a/SKILL.md", ".agents/skills/a/SKILL.md"),
            ("/home/dev/.agents/skills/b/SKILL.md", "~/.agents/skills/b/SKILL.md"),
            ("/opt/skills/c/SKILL.md", "/opt/skills/c/SKILL.md"),
        ];
        for (input, expected) in cases {
            assert_eq!(index_display_path(Path::new(input), workspace, home), expected);
        }
        assert_eq!(index_display_path(Path::new("/home/dev/x/SKILL.md"), workspace, None), "/home/dev/x/SKILL.md");
    }
}
