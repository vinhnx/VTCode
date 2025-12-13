//! Slash command handlers for in-chat skill management
//!
//! Implements `/skills` command palette for loading, listing, and executing skills
//! within interactive chat sessions.

use anyhow::Result;
use std::path::PathBuf;
use vtcode_core::skills::loader::SkillLoader;
use vtcode_core::skills::types::Skill;

/// Skill-related command actions
#[derive(Clone, Debug)]
pub enum SkillCommandAction {
    /// List available skills
    List,
    /// Load a skill by name
    Load { name: String },
    /// Unload a skill
    Unload { name: String },
    /// Execute a skill with input
    Use { name: String, input: String },
    /// Show skill details
    Info { name: String },
}

/// Result of a skill command
#[derive(Clone, Debug)]
pub enum SkillCommandOutcome {
    /// Command handled, display info
    Handled { message: String },
    /// Load skill into session
    LoadSkill { skill: Skill },
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

    if rest.is_empty() || rest == "list" {
        return Ok(Some(SkillCommandAction::List));
    }

    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    match parts[0] {
        "load" => {
            if let Some(name) = parts.get(1) {
                Ok(Some(SkillCommandAction::Load {
                    name: name.to_string(),
                }))
            } else {
                Err(anyhow::anyhow!("load: skill name required"))
            }
        }
        "unload" => {
            if let Some(name) = parts.get(1) {
                Ok(Some(SkillCommandAction::Unload {
                    name: name.to_string(),
                }))
            } else {
                Err(anyhow::anyhow!("unload: skill name required"))
            }
        }
        "use" => {
            if let Some(rest_str) = parts.get(1) {
                let use_parts: Vec<&str> = rest_str.splitn(2, ' ').collect();
                if let Some(name) = use_parts.get(0) {
                    let input = use_parts
                        .get(1)
                        .map(|s| s.to_string())
                        .unwrap_or_default();
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
        "info" => {
            if let Some(name) = parts.get(1) {
                Ok(Some(SkillCommandAction::Info {
                    name: name.to_string(),
                }))
            } else {
                Err(anyhow::anyhow!("info: skill name required"))
            }
        }
        cmd => Err(anyhow::anyhow!("unknown skills subcommand: {}", cmd)),
    }
}

/// Execute a skill command
pub async fn handle_skill_command(
    action: SkillCommandAction,
    workspace: PathBuf,
) -> Result<SkillCommandOutcome> {
    let loader = SkillLoader::new(workspace);

    match action {
        SkillCommandAction::List => {
            let skills = loader.discover_skills()?;

            if skills.is_empty() {
                return Ok(SkillCommandOutcome::Handled {
                    message: "No skills found. Create one with: /skills create <path>".to_string(),
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
            output.push_str("\nUse `/skills info <name>` for details, `/skills load <name>` to load");

            Ok(SkillCommandOutcome::Handled { message: output })
        }

        SkillCommandAction::Load { name } => {
            match loader.load_skill(&name) {
                Ok(skill) => Ok(SkillCommandOutcome::LoadSkill { skill }),
                Err(e) => Ok(SkillCommandOutcome::Error {
                    message: format!("Failed to load skill '{}': {}", name, e),
                }),
            }
        }

        SkillCommandAction::Unload { name } => Ok(SkillCommandOutcome::UnloadSkill { name }),

        SkillCommandAction::Info { name } => {
            match loader.load_skill(&name) {
                Ok(skill) => {
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
                Err(e) => Ok(SkillCommandOutcome::Error {
                    message: format!("Failed to load skill '{}': {}", name, e),
                }),
            }
        }

        SkillCommandAction::Use { name, input } => {
            match loader.load_skill(&name) {
                Ok(skill) => Ok(SkillCommandOutcome::UseSkill { skill, input }),
                Err(e) => Ok(SkillCommandOutcome::Error {
                    message: format!("Failed to load skill '{}': {}", name, e),
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skills_list() {
        let result = parse_skill_command("/skills list").unwrap();
        assert!(matches!(result, Some(SkillCommandAction::List)));
    }

    #[test]
    fn test_parse_skills_list_default() {
        let result = parse_skill_command("/skills").unwrap();
        assert!(matches!(result, Some(SkillCommandAction::List)));
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
}
