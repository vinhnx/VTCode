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
    let trimmed = input.trim();

    if !trimmed.starts_with("/skills") {
        return Ok(None);
    }

    // Remove "/skills" prefix and split remaining args
    let rest = trimmed[7..].trim();

    if rest.is_empty() {
        return Ok(Some(SkillCommandAction::Help));
    }

    if rest == "list" || rest == "--list" || rest == "-l" {
        return Ok(Some(SkillCommandAction::List { query: None }));
    }

    if rest == "help" || rest == "--help" || rest == "-h" {
        return Ok(Some(SkillCommandAction::Help));
    }

    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    match parts[0] {
        "search" | "--search" | "-s" => {
            if let Some(query) = parts.get(1) {
                Ok(Some(SkillCommandAction::List {
                    query: Some(query.to_string()),
                }))
            } else {
                Err(anyhow::anyhow!("search: query string required"))
            }
        }
        "create" | "--create" => {
            if let Some(name) = parts.get(1) {
                // Optional: parse --path flag
                let mut name_str = name.to_string();
                let mut path = None;

                if name.contains("--path") {
                    let name_parts: Vec<&str> = name.split_whitespace().collect();
                    name_str = name_parts[0].to_string();
                    if let Some(idx) = name_parts.iter().position(|&x| x == "--path")
                        && let Some(path_str) = name_parts.get(idx + 1)
                    {
                        path = Some(PathBuf::from(path_str));
                    }
                }

                Ok(Some(SkillCommandAction::Create {
                    name: name_str,
                    path,
                }))
            } else {
                Err(anyhow::anyhow!("create: skill name required"))
            }
        }
        "validate" | "--validate" => {
            if let Some(name) = parts.get(1) {
                Ok(Some(SkillCommandAction::Validate {
                    name: name.to_string(),
                }))
            } else {
                Err(anyhow::anyhow!("validate: skill name required"))
            }
        }
        "package" | "--package" => {
            if let Some(name) = parts.get(1) {
                Ok(Some(SkillCommandAction::Package {
                    name: name.to_string(),
                }))
            } else {
                Err(anyhow::anyhow!("package: skill name required"))
            }
        }
        "load" | "--load" => {
            if let Some(name) = parts.get(1) {
                Ok(Some(SkillCommandAction::Load {
                    name: name.to_string(),
                }))
            } else {
                Err(anyhow::anyhow!("load: skill name required"))
            }
        }
        "unload" | "--unload" => {
            if let Some(name) = parts.get(1) {
                Ok(Some(SkillCommandAction::Unload {
                    name: name.to_string(),
                }))
            } else {
                Err(anyhow::anyhow!("unload: skill name required"))
            }
        }
        "use" | "--use" | "exec" | "--exec" => {
            if let Some(rest_str) = parts.get(1) {
                let use_parts: Vec<&str> = rest_str.splitn(2, ' ').collect();
                if let Some(name) = use_parts.first() {
                    let input = use_parts.get(1).map(|s| s.to_string()).unwrap_or_default();
                    Ok(Some(SkillCommandAction::Use {
                        name: name.to_string(),
                        input,
                    }))
                } else {
                    Err(anyhow::anyhow!("use: skill name required"))
                }
            } else {
                Err(anyhow::anyhow!("use: skill name required"))
            }
        }
        "info" | "--info" | "show" | "--show" => {
            if let Some(name) = parts.get(1) {
                Ok(Some(SkillCommandAction::Info {
                    name: name.to_string(),
                }))
            } else {
                Err(anyhow::anyhow!("info: skill name required"))
            }
        }
        "regenerate-index" | "--regenerate-index" | "regen" | "--regen" => {
            Ok(Some(SkillCommandAction::RegenerateIndex))
        }
        cmd => Err(anyhow::anyhow!("unknown skills subcommand: {}", cmd)),
    }
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
                eprintln!("Warning: Failed to regenerate skills index: {}", e);
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
                eprintln!("Warning: Failed to regenerate skills index: {}", e);
            }

            match loader.get_skill(&name).await {
                Ok(enhanced_skill) => match enhanced_skill {
                    vtcode_core::skills::loader::EnhancedSkill::Traditional(skill) => {
                        let message = format!(
                            "✓ Loaded skill: {}\nℹ Instructions are now [ACTIVE] and persistent in the agent prompt.",
                            skill.name()
                        );
                        Ok(SkillCommandOutcome::LoadSkill { skill, message })
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
                eprintln!("Warning: Failed to regenerate skills index: {}", e);
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
                    Ok(SkillCommandOutcome::UseSkill { skill, input })
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
            skills.push((name.clone(), skill));
        }
    }

    Ok(skills)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skills_list() {
        let result = parse_skill_command("/skills --list").unwrap();
        assert!(matches!(
            result,
            Some(SkillCommandAction::List { query: None })
        ));
    }

    #[test]
    fn test_parse_skills_search() {
        let result = parse_skill_command("/skills --search rust").unwrap();
        match result {
            Some(SkillCommandAction::List { query: Some(q) }) => {
                assert_eq!(q, "rust");
            }
            _ => panic!("Expected List with query variant"),
        }
    }

    #[test]
    fn test_parse_skills_list_default() {
        let result = parse_skill_command("/skills").unwrap();
        assert!(matches!(result, Some(SkillCommandAction::Help)));
    }

    #[test]
    fn test_parse_skills_load() {
        let result = parse_skill_command("/skills load my-skill").unwrap();
        match result {
            Some(SkillCommandAction::Load { name }) => {
                assert_eq!(name, "my-skill");
            }
            _ => panic!("Expected Load variant"),
        }
    }

    #[test]
    fn test_parse_skills_info() {
        let result = parse_skill_command("/skills info my-skill").unwrap();
        match result {
            Some(SkillCommandAction::Info { name }) => {
                assert_eq!(name, "my-skill");
            }
            _ => panic!("Expected Info variant"),
        }
    }

    #[test]
    fn test_parse_skills_use() {
        let result = parse_skill_command("/skills use my-skill hello world").unwrap();
        match result {
            Some(SkillCommandAction::Use { name, input }) => {
                assert_eq!(name, "my-skill");
                assert_eq!(input, "hello world");
            }
            _ => panic!("Expected Use variant"),
        }
    }

    #[test]
    fn test_parse_skills_unload() {
        let result = parse_skill_command("/skills unload my-skill").unwrap();
        match result {
            Some(SkillCommandAction::Unload { name }) => {
                assert_eq!(name, "my-skill");
            }
            _ => panic!("Expected Unload variant"),
        }
    }

    #[test]
    fn test_parse_non_skill_command() {
        let result = parse_skill_command("/help").unwrap();
        assert!(result.is_none());
    }

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
