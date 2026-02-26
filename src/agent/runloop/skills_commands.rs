//! Slash command handlers for in-chat skill management
//!
//! Implements `/skills` command palette for loading, listing, and executing skills
//! within interactive chat sessions.
//!
//! Supports both explicit commands (`/skills load pdf-analyzer`) and Codex-style
//! mention detection (`$pdf-analyzer` or description keyword matching).

use anyhow::Result;
use std::path::PathBuf;
use vtcode_core::skills::authoring::SkillAuthor;
use vtcode_core::skills::loader::{EnhancedSkillLoader, detect_skill_mentions};
use vtcode_core::skills::types::{Skill, SkillManifest};

use super::skills_commands_parser::parse_skill_command as parse_skill_command_impl;

/// Skill-related command actions
#[derive(Clone, Debug)]
pub enum SkillCommandAction {
    /// Show help
    Help,
    /// List available skills
    List { query: Option<String> },
    /// Create a new skill from template
    Create { name: String, path: Option<PathBuf> },
    /// Validate a skill
    Validate { name: String },
    /// Package a skill into .skill file
    Package { name: String },
    /// Load a skill by name
    Load { name: String },
    /// Unload a skill
    Unload { name: String },
    /// Execute a skill with input
    Use { name: String, input: String },
    /// Show skill details
    Info { name: String },
    /// Regenerate skills index file
    RegenerateIndex,
}

/// Result of a skill command
#[derive(Clone, Debug)]
pub enum SkillCommandOutcome {
    /// Command handled, display info
    Handled { message: String },
    /// Load skill into session
    LoadSkill { skill: Skill, message: String },
    /// Unload skill from session
    UnloadSkill { name: String },
    /// Execute skill with input
    UseSkill { skill: Skill, input: String },
    /// Error occurred
    Error { message: String },
}

/// Parse skill subcommand from input
pub fn parse_skill_command(input: &str) -> Result<Option<SkillCommandAction>> {
    parse_skill_command_impl(input)
}

/// Execute a skill command
pub async fn handle_skill_command(
    action: SkillCommandAction,
    workspace: PathBuf,
) -> Result<SkillCommandOutcome> {
    let author = SkillAuthor::new(workspace.clone());
    let mut loader = EnhancedSkillLoader::new(workspace.clone());

    match action {
        SkillCommandAction::Help => {
            let help_text = r#"Skills Commands:

Authoring:
  /skills --create <name> [--path <dir>]   Create new skill from template
  /skills --validate <name>               Validate skill structure
  /skills --package <name>                Package skill to .skill file

Management:
  /skills --list [query]                  List available skills (optional search)
  /skills --search <query>                Search for skills by name/description
  /skills --load <name>                   Load skill into session
  /skills --unload <name>                 Unload skill from session
  /skills --info <name>                   Show skill details
  /skills --use <name> <input>            Execute skill with input
  /skills --regenerate-index              Regenerate skills index file

Shortcuts:
  /skills -l [query], /skills -s <query>, /skills -h, /skills --regen"#;

            Ok(SkillCommandOutcome::Handled {
                message: help_text.to_string(),
            })
        }

        SkillCommandAction::Create { name, path } => match author.create_skill(&name, path) {
            Ok(skill_dir) => Ok(SkillCommandOutcome::Handled {
                message: format!(
                    "✓ Created skill: {}\n\nNext steps:\n1. Edit {}/SKILL.md to complete the frontmatter and instructions\n2. Add scripts, references, or assets as needed\n3. Validate with: /skills validate {}\n4. Package with: /skills package {}",
                    name,
                    skill_dir.display(),
                    name,
                    name
                ),
            }),
            Err(e) => Ok(SkillCommandOutcome::Error {
                message: format!("Failed to create skill: {}", e),
            }),
        },

        SkillCommandAction::Validate { name } => {
            let skill_dir = workspace.join("skills").join(&name);
            if !skill_dir.exists() {
                return Ok(SkillCommandOutcome::Error {
                    message: format!("Skill directory not found: {}", skill_dir.display()),
                });
            }

            match author.validate_skill(&skill_dir) {
                Ok(report) => Ok(SkillCommandOutcome::Handled {
                    message: report.format(),
                }),
                Err(e) => Ok(SkillCommandOutcome::Error {
                    message: format!("Validation error: {}", e),
                }),
            }
        }

        SkillCommandAction::Package { name } => {
            let skill_dir = workspace.join("skills").join(&name);
            if !skill_dir.exists() {
                return Ok(SkillCommandOutcome::Error {
                    message: format!("Skill directory not found: {}", skill_dir.display()),
                });
            }

            match author.package_skill(&skill_dir, Some(workspace.clone())) {
                Ok(output_file) => Ok(SkillCommandOutcome::Handled {
                    message: format!("✓ Packaged skill to: {}", output_file.display()),
                }),
                Err(e) => Ok(SkillCommandOutcome::Error {
                    message: format!("Packaging failed: {}", e),
                }),
            }
        }

        SkillCommandAction::List { query } => {
            // Regenerate the skills index to ensure it's up to date with any newly added skills
            use vtcode_core::exec::skill_manager::SkillManager;
            let skill_manager = SkillManager::new(&workspace);
            if let Err(e) = skill_manager.generate_index().await {
                tracing::warn!("Failed to regenerate skills index: {}", e);
            }

            let discovery_result = loader.discover_all_skills().await?;
            let mut skills = discovery_result.skills;
            let _cli_tools = discovery_result.tools;
            // Apply query filter if provided
            if let Some(q) = query {
                let q_lower = q.to_lowercase();
                skills.retain(|ctx| {
                    let manifest = ctx.manifest();
                    manifest.name.to_lowercase().contains(&q_lower)
                        || manifest.description.to_lowercase().contains(&q_lower)
                });
            }

            if skills.is_empty() {
                return Ok(SkillCommandOutcome::Handled {
                    message: "No matching skills found.".to_string(),
                });
            }

            let mut output = String::from("Available Skills:\n");
            for skill_ctx in &skills {
                let manifest = skill_ctx.manifest();
                output.push_str(&format!(
                    "  • {} - {}\n",
                    manifest.name, manifest.description
                ));
            }
            output.push_str(
                "\nUse `/skills --info <name>` for details, `/skills --load <name>` to load",
            );

            Ok(SkillCommandOutcome::Handled { message: output })
        }

        SkillCommandAction::Load { name } => {
            // Regenerate the skills index to ensure it's up to date with any newly added skills
            use vtcode_core::exec::skill_manager::SkillManager;
            let skill_manager = SkillManager::new(&workspace);
            if let Err(e) = skill_manager.generate_index().await {
                tracing::warn!("Failed to regenerate skills index: {}", e);
            }

            match loader.get_skill(&name).await {
                Ok(enhanced_skill) => match enhanced_skill {
                    vtcode_core::skills::loader::EnhancedSkill::Traditional(skill) => {
                        let message = format!(
                            "✓ Loaded skill: {}\nℹ Instructions are now [ACTIVE] and persistent in the agent prompt.",
                            skill.name()
                        );
                        Ok(SkillCommandOutcome::LoadSkill {
                            skill: *skill,
                            message,
                        })
                    }
                    vtcode_core::skills::loader::EnhancedSkill::CliTool(_) => {
                        Ok(SkillCommandOutcome::Error {
                            message: format!(
                                "Skill '{}' is a CLI tool, not a traditional skill",
                                name
                            ),
                        })
                    }
                },
                Err(e) => Ok(SkillCommandOutcome::Error {
                    message: format!("Failed to load skill '{}': {}", name, e),
                }),
            }
        }

        SkillCommandAction::Unload { name } => Ok(SkillCommandOutcome::UnloadSkill { name }),

        SkillCommandAction::Info { name } => {
            // Regenerate the skills index to ensure it's up to date with any newly added skills
            use vtcode_core::exec::skill_manager::SkillManager;
            let skill_manager = SkillManager::new(&workspace);
            if let Err(e) = skill_manager.generate_index().await {
                tracing::warn!("Failed to regenerate skills index: {}", e);
            }

            match loader.get_skill(&name).await {
                Ok(enhanced_skill) => match enhanced_skill {
                    vtcode_core::skills::loader::EnhancedSkill::Traditional(skill) => {
                        let mut output = String::new();
                        output.push_str(&format!("Skill: {}\n", skill.name()));
                        output.push_str(&format!("Description: {}\n", skill.description()));

                        if let Some(version) = &skill.manifest.version {
                            output.push_str(&format!("Version: {}\n", version));
                        }

                        output.push_str("\n--- Instructions ---\n");
                        output.push_str(&skill.instructions);

                        if !skill.list_resources().is_empty() {
                            output.push_str("\n\n--- Resources ---\n");
                            for resource in skill.list_resources() {
                                output.push_str(&format!("  • {}\n", resource));
                            }
                        }

                        Ok(SkillCommandOutcome::Handled { message: output })
                    }
                    vtcode_core::skills::loader::EnhancedSkill::CliTool(bridge) => {
                        let mut output = String::new();
                        output.push_str(&format!("CLI Tool Skill: {}\n", bridge.config.name));
                        output.push_str(&format!("Description: {}\n", bridge.config.description));
                        output.push_str("\n--- Tool Configuration ---\n");
                        output.push_str("Tool available for execution");
                        Ok(SkillCommandOutcome::Handled { message: output })
                    }
                },
                Err(e) => Ok(SkillCommandOutcome::Error {
                    message: format!("Failed to load skill '{}': {}", name, e),
                }),
            }
        }

        SkillCommandAction::Use { name, input } => match loader.get_skill(&name).await {
            Ok(enhanced_skill) => match enhanced_skill {
                vtcode_core::skills::loader::EnhancedSkill::Traditional(skill) => {
                    Ok(SkillCommandOutcome::UseSkill {
                        skill: *skill,
                        input,
                    })
                }
                vtcode_core::skills::loader::EnhancedSkill::CliTool(_) => {
                    Ok(SkillCommandOutcome::Error {
                        message: format!("Skill '{}' is a CLI tool, not a traditional skill", name),
                    })
                }
            },
            Err(e) => Ok(SkillCommandOutcome::Error {
                message: format!("Failed to load skill '{}': {}", name, e),
            }),
        },

        SkillCommandAction::RegenerateIndex => {
            // Use the EnhancedSkillLoader to discover all skills and regenerate the index
            let discovery_result = loader.discover_all_skills().await?;
            let total_skills = discovery_result.skills.len() + discovery_result.tools.len();

            // Use the traditional SkillManager to update the index file
            use vtcode_core::exec::skill_manager::SkillManager;
            let skill_manager = SkillManager::new(&workspace);

            match skill_manager.generate_index().await {
                Ok(index_path) => {
                    let message = format!(
                        "Skills index regenerated successfully!\nIndex file: {}\nFound {} skills.",
                        index_path.display(),
                        total_skills
                    );

                    Ok(SkillCommandOutcome::Handled { message })
                }
                Err(e) => Ok(SkillCommandOutcome::Error {
                    message: format!("Failed to regenerate skills index: {}", e),
                }),
            }
        }
    }
}

/// Detect skill mentions in user input using Codex-style patterns
///
/// Returns list of skill names that should be auto-triggered based on:
/// 1. Explicit `$skill-name` mention (e.g., "Use $pdf-analyzer")
/// 2. Description keyword matches (fuzzy, requires 2+ matches)
///
/// # Examples
/// ```
/// // Explicit mention
/// "Use $pdf-analyzer to process the document" -> ["pdf-analyzer"]
///
/// // Description matching
/// "Extract tables from PDF document" -> ["pdf-analyzer"] (if description contains "extract" + "tables" or "PDF")
/// ```
#[allow(dead_code)]
pub async fn detect_mentioned_skills(
    user_input: &str,
    workspace: PathBuf,
) -> Result<Vec<(String, Skill)>> {
    let mut loader = EnhancedSkillLoader::new(workspace);

    // Discover available skills
    let discovery_result = loader.discover_all_skills().await?;
    let manifests: Vec<SkillManifest> = discovery_result
        .skills
        .iter()
        .map(|s| s.manifest().clone())
        .collect();

    // Detect mentions using the same logic as vtcode-core
    let mentioned_names = detect_skill_mentions(user_input, &manifests);

    // Load the mentioned skills
    let mut skills = Vec::new();
    for name in mentioned_names {
        if let Ok(enhanced_skill) = loader.get_skill(&name).await
            && let vtcode_core::skills::loader::EnhancedSkill::Traditional(skill) = enhanced_skill
        {
            skills.push((name.clone(), *skill));
        }
    }

    Ok(skills)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_detect_explicit_skill_mention() {
        // This test would need actual skills in a temp directory
        // Just verify the function signature compiles
        let input = "Use $pdf-analyzer to process the document";
        let workspace = PathBuf::from("/tmp");
        let _result = detect_mentioned_skills(input, workspace).await;
        // In real test, would assert skills are detected
    }
}
