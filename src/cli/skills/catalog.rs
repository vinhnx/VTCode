use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use tracing::info;
use vtcode_core::exec::skill_manager::SkillManager;
use vtcode_core::skills::container_validation::ContainerSkillsRequirement;
use vtcode_core::skills::loader::{EnhancedSkill, EnhancedSkillLoader};
use vtcode_core::skills::manifest::parse_skill_file;
use vtcode_core::skills::types::Skill;

use crate::cli::SkillsCommandOptions;

use super::render::{
    CliToolRow, LoadedSkillSummary, TraditionalSkillRow, print_cli_tools, print_empty_list,
    print_list_header, print_list_usage, print_loaded_skill, print_skill_config, print_skill_ready,
    print_traditional_skills,
};

/// List available skills.
pub async fn handle_skills_list(options: &SkillsCommandOptions) -> Result<()> {
    let mut loader = EnhancedSkillLoader::new(options.workspace.clone());
    refresh_skill_index(options).await;

    print_list_header();

    let discovery_result = loader
        .discover_all_skills()
        .await
        .context("Failed to discover skills")?;

    if discovery_result.skills.is_empty() && discovery_result.tools.is_empty() {
        print_empty_list();
        return Ok(());
    }

    let mut warnings = Vec::new();
    let mut skill_rows = Vec::new();

    for skill_ctx in &discovery_result.skills {
        let manifest = skill_ctx.manifest();
        let mut temp_loader = EnhancedSkillLoader::new(options.workspace.clone());

        match temp_loader.get_skill(&manifest.name).await {
            Ok(EnhancedSkill::Traditional(skill)) => {
                let analysis = temp_loader.check_container_requirements(&skill);
                let status = match analysis.requirement {
                    ContainerSkillsRequirement::Required => {
                        warnings.push(format!(
                            "x {} - Requires container skills (not compatible)",
                            manifest.name
                        ));
                        "x"
                    }
                    ContainerSkillsRequirement::RequiredWithFallback => {
                        warnings.push(format!(
                            "[!]  {} - Has container skills fallback",
                            manifest.name
                        ));
                        "[!]"
                    }
                    _ => "✓",
                };
                skill_rows.push(TraditionalSkillRow {
                    status,
                    name: manifest.name.clone(),
                    description: manifest.description.clone(),
                });
            }
            Ok(EnhancedSkill::CliTool(_)) | Ok(EnhancedSkill::NativePlugin(_)) => {}
            Err(_) => {
                warnings.push(format!(
                    "x {} - Requires container skills (validation failed)",
                    manifest.name
                ));
                skill_rows.push(TraditionalSkillRow {
                    status: "x",
                    name: manifest.name.clone(),
                    description: manifest.description.clone(),
                });
            }
        }
    }

    let cli_tool_rows = discovery_result
        .tools
        .iter()
        .map(|tool| CliToolRow {
            name: tool.name.clone(),
            description: tool.description.clone(),
            path: tool.executable_path.display().to_string(),
        })
        .collect::<Vec<_>>();

    print_traditional_skills(&skill_rows, &warnings);
    print_cli_tools(&cli_tool_rows);
    print_list_usage();

    Ok(())
}

/// Load a skill for the current session.
pub async fn handle_skills_load(
    options: &SkillsCommandOptions,
    name: &str,
    path: Option<PathBuf>,
) -> Result<()> {
    println!("Loading skill: {}...", name);

    let skill = resolve_skill_load(options, name, path.as_deref()).await?;
    let loaded_name = print_loaded_skill_summary(skill);
    print_skill_ready(&loaded_name);

    info!("Loaded skill: {}", loaded_name);
    Ok(())
}

/// Show skill details.
pub async fn handle_skills_info(options: &SkillsCommandOptions, name: &str) -> Result<()> {
    let mut loader = prepare_loader(options).await?;

    println!("Loading skill: {}...\n", name);

    let skill = loader
        .get_skill(name)
        .await
        .with_context(|| format!("Failed to load skill '{}'", name))?;

    match skill {
        EnhancedSkill::Traditional(skill) => {
            println!("Skill: {}", skill.name());
            println!("Description: {}", skill.description());
            if let Some(license) = &skill.manifest.license {
                println!("License: {}", license);
            }
            if let Some(compatibility) = &skill.manifest.compatibility {
                println!("Compatibility: {}", compatibility);
            }
            if let Some(allowed_tools) = &skill.manifest.allowed_tools
                && !allowed_tools.is_empty()
            {
                println!("Allowed tools: {}", allowed_tools);
            }
            if let Some(metadata) = &skill.manifest.metadata
                && !metadata.is_empty()
            {
                println!("Metadata keys: {}", metadata.len());
            }
            println!("Scope: {:?}", skill.scope);
            println!("Path: {}", skill.path.join("SKILL.md").display());

            let analysis = loader.check_container_requirements(&skill);
            println!("\n--- Compatibility ---");
            match analysis.requirement {
                ContainerSkillsRequirement::Required => {
                    println!(" Requires Anthropic container skills - NOT COMPATIBLE with VT Code");
                }
                ContainerSkillsRequirement::RequiredWithFallback => {
                    println!(
                        "  Uses container skills but provides VT Code-compatible alternatives"
                    );
                }
                ContainerSkillsRequirement::NotRequired => {
                    println!(" Fully compatible with VT Code");
                }
                ContainerSkillsRequirement::Unknown => {
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
        EnhancedSkill::CliTool(bridge) => {
            println!("CLI Tool Skill: {}", bridge.config.name);
            println!("Description: {}", bridge.config.description);
            println!("\n--- Tool Configuration ---");
            println!("Tool available for execution");
        }
        EnhancedSkill::NativePlugin(plugin) => {
            let meta = plugin.metadata();
            println!("Native Plugin: {}", meta.name);
            println!("Description: {}", meta.description);
            println!("\n--- Plugin Configuration ---");
            println!("Native plugin available for execution");
        }
    }

    Ok(())
}

/// Show skill configuration.
pub async fn handle_skills_config(options: &SkillsCommandOptions) -> Result<()> {
    print_skill_config(&options.workspace);
    Ok(())
}

async fn prepare_loader(options: &SkillsCommandOptions) -> Result<EnhancedSkillLoader> {
    refresh_skill_index(options).await;

    let mut loader = EnhancedSkillLoader::new(options.workspace.clone());
    loader
        .discover_all_skills()
        .await
        .context("Failed to discover skills")?;
    Ok(loader)
}

async fn resolve_skill_load(
    options: &SkillsCommandOptions,
    requested_name: &str,
    path: Option<&Path>,
) -> Result<EnhancedSkill> {
    if let Some(path) = path {
        let skill = load_skill_from_path(path)?;
        if skill.name() != requested_name {
            bail!(
                "Skill name '{}' does not match manifest name '{}' at {}",
                requested_name,
                skill.name(),
                path.display()
            );
        }
        return Ok(EnhancedSkill::Traditional(Box::new(skill)));
    }

    let mut loader = prepare_loader(options).await?;
    loader
        .get_skill(requested_name)
        .await
        .with_context(|| format!("Failed to load skill '{}'", requested_name))
}

fn load_skill_from_path(path: &Path) -> Result<Skill> {
    validate_skill_path(path)?;

    let (manifest, instructions) = parse_skill_file(path)
        .with_context(|| format!("Failed to parse skill manifest at {}", path.display()))?;
    Skill::new(manifest, path.to_path_buf(), instructions)
        .with_context(|| format!("Failed to load skill at {}", path.display()))
}

fn validate_skill_path(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("Skill path does not exist: {}", path.display());
    }
    if !path.is_dir() {
        bail!("Skill path is not a directory: {}", path.display());
    }
    Ok(())
}

fn print_loaded_skill_summary(skill: EnhancedSkill) -> String {
    match skill {
        EnhancedSkill::Traditional(skill) => {
            let name = skill.name().to_string();
            let summary = LoadedSkillSummary {
                headline: format!("Loaded skill: {}", name),
                details: vec![
                    format!("Description: {}", skill.description()),
                    format!("Resources: {} files", skill.list_resources().len()),
                ],
            };
            print_loaded_skill(&summary);
            name
        }
        EnhancedSkill::CliTool(bridge) => {
            let name = bridge.config.name.clone();
            let summary = LoadedSkillSummary {
                headline: format!("Loaded CLI tool skill: {}", name),
                details: vec![format!("Description: {}", bridge.config.description)],
            };
            print_loaded_skill(&summary);
            name
        }
        EnhancedSkill::NativePlugin(plugin) => {
            let meta = plugin.metadata();
            let name = meta.name.clone();
            let summary = LoadedSkillSummary {
                headline: format!("Loaded native plugin: {}", name),
                details: vec![format!("Description: {}", meta.description)],
            };
            print_loaded_skill(&summary);
            name
        }
    }
}

async fn refresh_skill_index(options: &SkillsCommandOptions) {
    let skill_manager = SkillManager::new(&options.workspace);
    if let Err(err) = skill_manager.generate_index().await {
        eprintln!("Warning: Failed to regenerate skills index: {}", err);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn lists_empty_skills_directory() {
        let temp_dir = TempDir::new().expect("temp dir");
        let options = SkillsCommandOptions {
            workspace: temp_dir.path().to_path_buf(),
        };

        let result = handle_skills_list(&options).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn loads_discovered_skill_by_name() {
        let temp_dir = TempDir::new().expect("temp dir");
        let skill_path = temp_dir.path().join(".agents/skills/test-skill");
        std::fs::create_dir_all(&skill_path).expect("create skill dir");
        std::fs::write(
            skill_path.join("SKILL.md"),
            r#"---
name: test-skill
description: A test skill
---

# Test Skill
Use this skill.
"#,
        )
        .expect("write skill");

        let options = SkillsCommandOptions {
            workspace: temp_dir.path().to_path_buf(),
        };
        let result = handle_skills_load(&options, "test-skill", None).await;

        assert!(result.is_ok(), "{result:#?}");
    }

    #[tokio::test]
    async fn loads_skill_directly_from_explicit_path() {
        let temp_dir = TempDir::new().expect("temp dir");
        let skill_path = temp_dir.path().join("standalone-skill");
        std::fs::create_dir_all(&skill_path).expect("create skill dir");
        std::fs::write(
            skill_path.join("SKILL.md"),
            r#"---
name: standalone-skill
description: A standalone skill
---

# Standalone Skill
Use this skill.
"#,
        )
        .expect("write skill");

        let options = SkillsCommandOptions {
            workspace: temp_dir.path().to_path_buf(),
        };
        let result =
            handle_skills_load(&options, "standalone-skill", Some(skill_path.clone())).await;

        assert!(result.is_ok(), "{result:#?}");
    }

    #[tokio::test]
    async fn rejects_missing_direct_skill_path() {
        let temp_dir = TempDir::new().expect("temp dir");
        let options = SkillsCommandOptions {
            workspace: temp_dir.path().to_path_buf(),
        };

        let result = handle_skills_load(
            &options,
            "missing-skill",
            Some(temp_dir.path().join("missing-skill")),
        )
        .await;

        let err = result.expect_err("expected missing path error");
        assert!(err.to_string().contains("does not exist"));
    }
}
