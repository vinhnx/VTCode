//! SKILL.md manifest parsing
//!
//! Parses YAML frontmatter from SKILL.md files to extract skill metadata and instructions.

use crate::skills::types::SkillManifest;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// YAML frontmatter structure for SKILL.md
#[derive(Debug, Serialize, Deserialize)]
pub struct SkillYaml {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "vtcode-native")]
    #[serde(alias = "vtcode_native")]
    pub vtcode_native: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "allowed-tools")]
    #[serde(alias = "allowed_tools")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "disable-model-invocation")]
    #[serde(alias = "disable_model_invocation")]
    pub disable_model_invocation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "when-to-use")]
    #[serde(alias = "when_to_use")]
    pub when_to_use: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "requires-container")]
    #[serde(alias = "requires_container")]
    pub requires_container: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "disallow-container")]
    #[serde(alias = "disallow_container")]
    pub disallow_container: Option<bool>,
}

/// Parse SKILL.md file and extract manifest + instructions
pub fn parse_skill_file(skill_path: &Path) -> anyhow::Result<(SkillManifest, String)> {
    let skill_md = skill_path.join("SKILL.md");
    anyhow::ensure!(
        skill_md.exists(),
        "SKILL.md not found at {}",
        skill_md.display()
    );

    let content = fs::read_to_string(&skill_md)
        .context(format!("Failed to read SKILL.md at {}", skill_md.display()))?;

    parse_skill_content(&content)
}

/// Parse SKILL.md content string
pub fn parse_skill_content(content: &str) -> anyhow::Result<(SkillManifest, String)> {
    // Split YAML frontmatter (between --- markers)
    let parts: Vec<&str> = content.splitn(3, "---").collect();

    anyhow::ensure!(
        parts.len() >= 3,
        "SKILL.md must start with YAML frontmatter: --- ... ---"
    );

    let yaml_str = parts[1].trim();
    let instructions = parts[2].trim_start().to_string();

    // Parse YAML frontmatter
    let yaml: SkillYaml =
        serde_yaml::from_str(yaml_str).context("Failed to parse SKILL.md YAML frontmatter")?;

    let manifest = SkillManifest {
        name: yaml.name,
        description: yaml.description,
        version: yaml.version,
        author: yaml.author,
        license: yaml.license,
        model: yaml.model,
        mode: yaml.mode,
        vtcode_native: yaml.vtcode_native,
        allowed_tools: yaml.allowed_tools,
        disable_model_invocation: yaml.disable_model_invocation,
        when_to_use: yaml.when_to_use,
        requires_container: yaml.requires_container,
        disallow_container: yaml.disallow_container,
    };

    manifest.validate()?;

    Ok((manifest, instructions))
}

/// Generate a skill template with YAML frontmatter
pub fn generate_skill_template(name: &str, description: &str) -> String {
    format!(
        r#"---
name: {}
description: {}
version: 0.1.0
author: Anonymous
license: MIT
model: inherit
mode: false
# Optional skill controls (uncomment to use)
# allowed-tools:
#   - code_execution
#   - bash
# disable-model-invocation: false
# when-to-use: "Trigger for data-heavy spreadsheet generation"
# requires-container: false
# disallow-container: false
---

# {} Skill

## Instructions

[Add step-by-step guidance for Claude to follow when this skill is active]

## Examples

- Example usage 1
- Example usage 2

## Guidelines

- Guideline 1
- Guideline 2
"#,
        name, description, name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_skill() {
        let content = r#"---
name: test-skill
description: A test skill for parsing
version: 1.0.0
author: Test Author
---

# Test Skill

## Instructions
This is the instruction section.

## Examples
- Example 1
- Example 2
"#;

        let (manifest, instructions) = parse_skill_content(content).unwrap();

        assert_eq!(manifest.name, "test-skill");
        assert_eq!(manifest.description, "A test skill for parsing");
        assert_eq!(manifest.version, Some("1.0.0".to_string()));
        assert_eq!(manifest.author, Some("Test Author".to_string()));
        assert!(instructions.contains("# Test Skill"));
        assert!(instructions.contains("## Instructions"));
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let content = "This is not valid";
        let result = parse_skill_content(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_yaml() {
        let content = r#"---
invalid: yaml: content: here
missing_required_fields: true
---

# Instructions
"#;

        let result = parse_skill_content(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_template() {
        let template = generate_skill_template("my-skill", "Does cool things");
        assert!(template.contains("name: my-skill"));
        assert!(template.contains("description: Does cool things"));
        assert!(template.contains("---"));
        assert!(template.contains("## Instructions"));
    }
}
