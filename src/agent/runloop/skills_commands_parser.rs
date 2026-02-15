use std::path::PathBuf;

use anyhow::Result;

use super::skills_commands::SkillCommandAction;

pub(super) fn parse_skill_command(input: &str) -> Result<Option<SkillCommandAction>> {
    let trimmed = input.trim();

    if !trimmed.starts_with("/skills") {
        return Ok(None);
    }

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
}
