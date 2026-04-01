//! Slash command handlers for in-chat skill management
//!
//! Implements `/skills` command palette for loading, listing, and executing skills
//! within interactive chat sessions.
//!
//! Supports both explicit commands (`/skills load pdf-analyzer`) and Codex-style
//! mention detection (`$pdf-analyzer` or description keyword matching).

use anyhow::Result;
use std::path::PathBuf;
#[cfg(test)]
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::skills::authoring::SkillAuthor;
use vtcode_core::skills::loader::EnhancedSkillLoader;
#[cfg(test)]
use vtcode_core::skills::loader::{
    SkillMentionDetectionOptions, detect_skill_mentions_with_options,
};
use vtcode_core::skills::types::{Skill, SkillManifest};

use super::skills_commands_parser::parse_skill_command as parse_skill_command_impl;

async fn regenerate_skills_index_best_effort(workspace: &std::path::Path) {
    if let Err(e) = crate::cli::skills_index::generate_comprehensive_skills_index(workspace).await {
        tracing::warn!("Failed to regenerate skills index: {}", e);
    }
}

fn workspace_skill_dir(workspace: &std::path::Path, name: &str) -> PathBuf {
    workspace.join(".agents").join("skills").join(name)
}

fn list_label_for_manifest(manifest: &SkillManifest) -> &'static str {
    match manifest.variety {
        vtcode_core::skills::types::SkillVariety::BuiltIn => "built_in",
        vtcode_core::skills::types::SkillVariety::SystemUtility => "system_utility",
        vtcode_core::skills::types::SkillVariety::AgentSkill => "agent_skill",
    }
}

/// Skill-related command actions
#[derive(Clone, Debug)]
pub(crate) enum SkillCommandAction {
    /// Open interactive skill manager
    Interactive,
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
pub(crate) enum SkillCommandOutcome {
    /// Command handled, display info
    Handled { message: String },
    /// Load skill into session
    LoadSkill { skill: Skill, message: String },
    /// Unload skill from session
    UnloadSkill { name: String },
    /// Execute skill with input
    UseSkill { skill: Skill, input: String },
    /// Execute a built-in command skill with input
    UseBuiltInCommand {
        name: String,
        slash_name: String,
        input: String,
    },
    /// Error occurred
    Error { message: String },
}

/// Parse skill subcommand from input
pub(crate) fn parse_skill_command(input: &str) -> Result<Option<SkillCommandAction>> {
    parse_skill_command_impl(input)
}

/// Execute a skill command
pub(crate) async fn handle_skill_command(
    action: SkillCommandAction,
    workspace: PathBuf,
) -> Result<SkillCommandOutcome> {
    let author = SkillAuthor::new(workspace.clone());
    let mut loader = EnhancedSkillLoader::new(workspace.clone());

    match action {
        SkillCommandAction::Interactive => Ok(SkillCommandOutcome::Handled {
            message: "Interactive skills manager is available in TUI sessions. Use /skills in inline mode to browse and toggle skills.".to_string(),
        }),
        SkillCommandAction::Help => {
            let help_text = r#"Skills Commands:

Interactive:
  /skills                                Open interactive skills manager in TUI
  /skills manager                        Alias for interactive skills manager

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
            let skill_dir = workspace_skill_dir(&workspace, &name);
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
            let skill_dir = workspace_skill_dir(&workspace, &name);
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
            regenerate_skills_index_best_effort(&workspace).await;

            let discovery_result = loader.discover_all_skills().await?;
            let mut skills = discovery_result.skills;
            let mut cli_tools = discovery_result.tools;
            // Apply query filter if provided
            if let Some(q) = query {
                let q_lower = q.to_lowercase();
                skills.retain(|ctx| {
                    let manifest = ctx.manifest();
                    manifest.name.to_lowercase().contains(&q_lower)
                        || manifest.description.to_lowercase().contains(&q_lower)
                });
                cli_tools.retain(|tool| {
                    tool.name.to_lowercase().contains(&q_lower)
                        || tool.description.to_lowercase().contains(&q_lower)
                });
            }

            if skills.is_empty() && cli_tools.is_empty() {
                return Ok(SkillCommandOutcome::Handled {
                    message: "No matching skills found.".to_string(),
                });
            }

            let mut output = String::from("Available Skills:\n");
            for skill_ctx in &skills {
                let manifest = skill_ctx.manifest();
                output.push_str(&format!(
                    "  • {} [{}] - {}\n",
                    manifest.name,
                    list_label_for_manifest(manifest),
                    manifest.description
                ));
            }
            if !cli_tools.is_empty() {
                output.push_str("\nSystem Utilities:\n");
                for tool in &cli_tools {
                    output.push_str(&format!("  • {} - {}\n", tool.name, tool.description));
                }
            }
            output.push_str(
                "\nUse `/skills --info <name>` for details, `/skills --load <name>` to load",
            );

            Ok(SkillCommandOutcome::Handled { message: output })
        }

        SkillCommandAction::Load { name } => {
            regenerate_skills_index_best_effort(&workspace).await;

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
                    vtcode_core::skills::loader::EnhancedSkill::BuiltInCommand(_) => {
                        Ok(SkillCommandOutcome::Error {
                            message: format!(
                                "Skill '{}' is a built-in command skill and cannot be loaded into the persistent session prompt. Use `/skills use {}` instead.",
                                name, name
                            ),
                        })
                    }
                    vtcode_core::skills::loader::EnhancedSkill::NativePlugin(_) => {
                        Ok(SkillCommandOutcome::Error {
                            message: format!(
                                "Skill '{}' is a native plugin, not a traditional skill",
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
            regenerate_skills_index_best_effort(&workspace).await;

            match loader.get_skill(&name).await {
                Ok(enhanced_skill) => match enhanced_skill {
                    vtcode_core::skills::loader::EnhancedSkill::Traditional(skill) => {
                        let mut output = String::new();
                        output.push_str(&format!("Skill: {}\n", skill.name()));
                        output.push_str(&format!("Description: {}\n", skill.description()));
                        if let Some(license) = &skill.manifest.license {
                            output.push_str(&format!("License: {}\n", license));
                        }
                        if let Some(compatibility) = &skill.manifest.compatibility {
                            output.push_str(&format!("Compatibility: {}\n", compatibility));
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
                    vtcode_core::skills::loader::EnhancedSkill::BuiltInCommand(skill) => {
                        let mut output = String::new();
                        output.push_str(&format!("Built-In Command Skill: {}\n", skill.name()));
                        output.push_str(&format!("Description: {}\n", skill.description()));
                        output.push_str(&format!("Slash alias: /{}\n", skill.slash_name()));
                        output.push_str(&format!("Usage: {}\n", skill.usage()));
                        output.push_str(&format!("Category: {}\n", skill.category()));
                        output.push_str("\n--- Backend ---\n");
                        output.push_str("Executes the existing slash command backend");
                        Ok(SkillCommandOutcome::Handled { message: output })
                    }
                    vtcode_core::skills::loader::EnhancedSkill::NativePlugin(plugin) => {
                        let meta = plugin.metadata();
                        let mut output = String::new();
                        output.push_str(&format!("Native Plugin: {}\n", meta.name));
                        output.push_str(&format!("Description: {}\n", meta.description));
                        output.push_str("\n--- Plugin Configuration ---\n");
                        output.push_str("Native plugin available for execution");
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
                vtcode_core::skills::loader::EnhancedSkill::BuiltInCommand(skill) => {
                    Ok(SkillCommandOutcome::UseBuiltInCommand {
                        name: skill.name().to_string(),
                        slash_name: skill.slash_name().to_string(),
                        input,
                    })
                }
                vtcode_core::skills::loader::EnhancedSkill::NativePlugin(_) => {
                    Ok(SkillCommandOutcome::Error {
                        message: format!("Skill '{}' is a native plugin, not a traditional skill", name),
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

            match crate::cli::skills_index::generate_comprehensive_skills_index(&workspace).await {
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
#[cfg(test)]
async fn detect_mentioned_skills(
    user_input: &str,
    workspace: PathBuf,
) -> Result<Vec<(String, Skill)>> {
    let mut loader = EnhancedSkillLoader::new(workspace.clone());

    // Discover available skills
    let discovery_result = loader.discover_all_skills().await?;
    let manifests: Vec<SkillManifest> = discovery_result
        .skills
        .iter()
        .map(|s| s.manifest().clone())
        .collect();

    // Detect mentions with workspace-aware routing config.
    let detection_options = ConfigManager::load_from_workspace(&workspace)
        .ok()
        .map(|manager| {
            let skills = &manager.config().skills;
            SkillMentionDetectionOptions {
                enable_auto_trigger: skills.enable_auto_trigger,
                enable_description_matching: skills.enable_description_matching,
                min_keyword_matches: skills.min_keyword_matches,
            }
        })
        .unwrap_or_default();
    let mentioned_names =
        detect_skill_mentions_with_options(user_input, &manifests, &detection_options);

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
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_detect_explicit_skill_mention() {
        // This test would need actual skills in a temp directory
        // Just verify the function signature compiles
        let input = "Use $pdf-analyzer to process the document";
        let workspace = PathBuf::from("/tmp");
        let _result = detect_mentioned_skills(input, workspace).await;
        // In real test, would assert skills are detected
    }

    #[tokio::test]
    async fn built_in_command_skill_info_reports_metadata() {
        let temp = tempdir().expect("tempdir");

        let outcome = handle_skill_command(
            SkillCommandAction::Info {
                name: "cmd-status".to_string(),
            },
            temp.path().to_path_buf(),
        )
        .await
        .expect("info outcome");

        match outcome {
            SkillCommandOutcome::Handled { message } => {
                assert!(message.contains("Built-In Command Skill: cmd-status"));
                assert!(message.contains("Slash alias: /status"));
                assert!(message.contains("Usage:"));
            }
            other => panic!("expected handled outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn built_in_command_skill_use_returns_built_in_outcome() {
        let temp = tempdir().expect("tempdir");

        let outcome = handle_skill_command(
            SkillCommandAction::Use {
                name: "cmd-status".to_string(),
                input: "show session".to_string(),
            },
            temp.path().to_path_buf(),
        )
        .await
        .expect("use outcome");

        match outcome {
            SkillCommandOutcome::UseBuiltInCommand {
                name,
                slash_name,
                input,
            } => {
                assert_eq!(name, "cmd-status");
                assert_eq!(slash_name, "status");
                assert_eq!(input, "show session");
            }
            other => panic!("expected built-in use outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn built_in_command_skill_load_is_rejected() {
        let temp = tempdir().expect("tempdir");

        let outcome = handle_skill_command(
            SkillCommandAction::Load {
                name: "cmd-status".to_string(),
            },
            temp.path().to_path_buf(),
        )
        .await
        .expect("load outcome");

        match outcome {
            SkillCommandOutcome::Error { message } => {
                assert!(message.contains("built-in command skill"));
                assert!(message.contains("/skills use cmd-status"));
            }
            other => panic!("expected error outcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn built_in_command_skill_list_includes_query_match() {
        let temp = tempdir().expect("tempdir");

        let outcome = handle_skill_command(
            SkillCommandAction::List {
                query: Some("cmd-status".to_string()),
            },
            temp.path().to_path_buf(),
        )
        .await
        .expect("list outcome");

        match outcome {
            SkillCommandOutcome::Handled { message } => {
                assert!(message.contains("Available Skills:"));
                assert!(message.contains("cmd-status [built_in] -"));
            }
            other => panic!("expected handled outcome, got {other:?}"),
        }
    }
}
