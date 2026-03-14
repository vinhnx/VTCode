//! SKILL.md manifest parsing
//!
//! Parses YAML frontmatter from SKILL.md files to extract skill metadata and instructions.

use crate::skills::file_references::FileReferenceValidator;
use crate::skills::types::{SkillManifest, SkillManifestMetadata};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static ALLOWED_TOOLS_ARRAY_WARNED: AtomicBool = AtomicBool::new(false);

/// Supported YAML frontmatter keys for SKILL.md validation.
pub const SUPPORTED_FRONTMATTER_KEYS: &[&str] = &[
    "name",
    "description",
    "version",
    "default-version",
    "default_version",
    "latest-version",
    "latest_version",
    "author",
    "license",
    "model",
    "mode",
    "tags",
    "vtcode-native",
    "vtcode_native",
    "allowed-tools",
    "allowed_tools",
    "disable-model-invocation",
    "disable_model_invocation",
    "when-to-use",
    "when_to_use",
    "when-not-to-use",
    "when_not_to_use",
    "argument-hint",
    "argument_hint",
    "user-invocable",
    "user_invocable",
    "context",
    "agent",
    "hooks",
    "requires-container",
    "requires_container",
    "disallow-container",
    "disallow_container",
    "compatibility",
    "metadata",
    "tools",
    "network",
    "network_policy",
    "permissions",
    "permission_profile",
];

/// YAML frontmatter structure for SKILL.md
#[derive(Debug, Serialize, Deserialize)]
pub struct SkillYaml {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "default-version")]
    #[serde(alias = "default_version")]
    pub default_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "latest-version")]
    #[serde(alias = "latest_version")]
    pub latest_version: Option<String>,
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
    pub allowed_tools: Option<AllowedToolsField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "disable-model-invocation")]
    #[serde(alias = "disable_model_invocation")]
    pub disable_model_invocation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "when-to-use")]
    #[serde(alias = "when_to_use")]
    pub when_to_use: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "when-not-to-use")]
    #[serde(alias = "when_not_to_use")]
    pub when_not_to_use: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "argument-hint")]
    #[serde(alias = "argument_hint")]
    pub argument_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "user-invocable")]
    #[serde(alias = "user_invocable")]
    pub user_invocable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "requires-container")]
    #[serde(alias = "requires_container")]
    pub requires_container: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "disallow-container")]
    #[serde(alias = "disallow_container")]
    pub disallow_container: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SkillManifestMetadata>,
    /// Tool dependencies for this skill (Agent Skills spec extension)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "network")]
    #[serde(alias = "network_policy")]
    pub network_policy: Option<crate::skills::types::SkillNetworkPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "permission_profile")]
    pub permissions: Option<crate::skills::types::SkillPermissionProfile>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AllowedToolsField {
    List(Vec<String>),
    String(String),
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

    let default_name = skill_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string());

    let (manifest, instructions) =
        parse_skill_content_with_defaults(&content, default_name.as_deref())?;

    // Validate directory name matches per Agent Skills spec
    // For traditional skills (not CLI tools), the name must match the directory
    manifest.validate_directory_name_match(&skill_md)?;

    // Validate file references in instructions
    // For traditional skills (SKILL.md files), validate references
    let skill_root = skill_md.parent().unwrap_or_else(|| Path::new("."));
    let reference_validator = FileReferenceValidator::new(skill_root.to_path_buf());
    let reference_errors = reference_validator.validate_references(&instructions);

    if !reference_errors.is_empty() {
        let sample_count = reference_errors.len().min(3);
        let sample = &reference_errors[..sample_count];
        tracing::warn!(
            warning_count = reference_errors.len(),
            sample = ?sample,
            "File reference validation warnings detected (showing first {})",
            sample_count
        );
        tracing::debug!(
            warnings = ?reference_errors,
            "File reference validation warnings (full list)"
        );
    }

    Ok((manifest, instructions))
}

/// Parse SKILL.md content string
pub fn parse_skill_content(content: &str) -> anyhow::Result<(SkillManifest, String)> {
    parse_skill_content_with_defaults(content, None)
}

/// Parse SKILL.md content with optional defaults from path context
pub fn parse_skill_content_with_defaults(
    content: &str,
    default_name: Option<&str>,
) -> anyhow::Result<(SkillManifest, String)> {
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

    let name = yaml
        .name
        .and_then(|name| {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .or_else(|| default_name.map(|name| name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("name is required and must not be empty"))?;

    let description = yaml
        .description
        .and_then(|description| {
            let trimmed = description.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .or_else(|| infer_description_from_instructions(&instructions))
        .ok_or_else(|| anyhow::anyhow!("description is required and must not be empty"))?;

    // Convert allowed-tools into space-delimited string for compatibility
    let allowed_tools_string = yaml
        .allowed_tools
        .map(normalize_allowed_tools)
        .transpose()?;

    let manifest = SkillManifest {
        name,
        description,
        version: yaml.version,
        default_version: yaml.default_version,
        latest_version: yaml.latest_version,
        author: yaml.author,
        license: yaml.license,
        model: yaml.model,
        mode: yaml.mode,
        vtcode_native: yaml.vtcode_native,
        allowed_tools: allowed_tools_string,
        disable_model_invocation: yaml.disable_model_invocation,
        when_to_use: yaml.when_to_use,
        when_not_to_use: yaml.when_not_to_use,
        argument_hint: yaml.argument_hint,
        user_invocable: yaml.user_invocable,
        context: yaml.context,
        agent: yaml.agent,
        hooks: yaml.hooks,
        requires_container: yaml.requires_container,
        disallow_container: yaml.disallow_container,
        compatibility: yaml.compatibility,
        variety: crate::skills::types::SkillVariety::AgentSkill,
        metadata: yaml.metadata,
        tools: yaml.tools,
        network_policy: yaml.network_policy,
        permissions: yaml.permissions,
    };

    manifest.validate()?;

    Ok((manifest, instructions))
}

fn infer_description_from_instructions(instructions: &str) -> Option<String> {
    let lines = instructions.lines();
    let mut paragraph = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        paragraph.push(trimmed);
    }
    if paragraph.is_empty() {
        None
    } else {
        Some(paragraph.join(" "))
    }
}

fn normalize_allowed_tools(field: AllowedToolsField) -> anyhow::Result<String> {
    match field {
        AllowedToolsField::List(tools) => {
            if !tools.is_empty() && !ALLOWED_TOOLS_ARRAY_WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!(
                    "allowed-tools uses deprecated array format, please use a string instead"
                );
            }
            Ok(tools.join(" "))
        }
        AllowedToolsField::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(anyhow::anyhow!(
                    "allowed-tools must not be empty if specified"
                ));
            }
            let has_commas = trimmed.contains(',');
            if has_commas {
                tracing::warn!(
                    "allowed-tools uses comma-separated format; normalizing to space-delimited"
                );
            }
            let parts = if has_commas {
                trimmed
                    .split(',')
                    .map(|part| part.trim())
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
            } else {
                trimmed.split_whitespace().collect::<Vec<_>>()
            };
            Ok(parts.join(" "))
        }
    }
}

/// Generate a skill template with YAML frontmatter
pub fn generate_skill_template(name: &str, description: &str) -> String {
    let skill_title = name
        .split('-')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        r#"---
name: {}
description: {}
version: 0.1.0
license: MIT
when-to-use: "Use when the request matches this workflow, the inputs are clear, and the skill should produce a repeatable artifact or outcome."
when-not-to-use: "Do not use when a simpler edit, search, or one-line unified_exec command is enough."
# Optional fields (uncomment to use):
# author: Your Name
# compatibility: "Designed for VT Code"
# allowed-tools: "Read Write Bash"  # Space-delimited list per Agent Skills spec
# argument-hint: "[path] [format]"  # Optional slash command hint
# user-invocable: true              # Hide from menu when false
# context: "fork"                   # Run in forked context
# agent: "explore"                  # Optional profile hint when context=fork
# metadata:
#   team: platform
#   owner: your-org
# default-version: "1.0.0"  # Pinned version for production
# latest-version: "1.1.0"  # Latest available version
# network:
#   allowed_domains: ["api.example.com"]
#   denied_domains: []
# permissions:
#   file_system:
#     write: ["outputs"]
---

# {}

## Purpose

Summarize the workflow, expected inputs, and the artifact or outcome this skill should produce.

## Workflow

1. Confirm the request matches the routing guidance above.
2. Keep core instructions here; move detailed reference material into bundled files.
3. Prefer reusable scripts, templates, or assets over re-describing large procedures in prose.
4. Produce the expected artifact or outcome and note any important constraints.

## Resources

- `scripts/`: deterministic helpers for repeatable or fragile steps
- `references/`: detailed docs loaded only when needed
- `templates/` or `assets/`: reusable output skeletons, examples, or supporting files

## Example

**Use when:** [Describe a concrete trigger or input shape]
**Don't use when:** [Describe a nearby case that should route elsewhere]
**Input:** [Describe the request or files]
**Output/Artifact:** [Describe the result this skill should produce]

## Notes

- Keep SKILL.md concise; move deep detail into `references/` files.
- If output needs a fixed shape, store a starter template or asset alongside the skill.
"#,
        name, description, skill_title
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn test_parse_skill_permissions() {
        let content = r#"---
name: sandboxed-skill
description: A skill with filesystem additions
permissions:
  file_system:
    read:
      - references
    write:
      - outputs
---

# Instructions
"#;

        let (manifest, _) = parse_skill_content(content).expect("permissions should parse");
        let permissions = manifest.permissions.expect("permissions should be present");
        let file_system = permissions
            .file_system
            .expect("file_system permissions should be present");

        assert_eq!(
            file_system.read,
            vec![std::path::PathBuf::from("references")]
        );
        assert_eq!(file_system.write, vec![std::path::PathBuf::from("outputs")]);
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
    fn test_parse_skill_metadata_accepts_arrays_and_maps() {
        let content = r#"---
name: rust-skills
description: Rust guidance
license: MIT
metadata:
  author: leonardomso
  version: "1.0.0"
  sources:
    - Rust API Guidelines
    - Rust Performance Book
---

# Rust Best Practices
"#;

        let (manifest, _) = parse_skill_content(content).expect("metadata arrays should parse");
        let metadata = manifest.metadata.expect("metadata should be present");

        assert_eq!(metadata.get("author"), Some(&json!("leonardomso")));
        assert_eq!(metadata.get("version"), Some(&json!("1.0.0")));
        assert_eq!(
            metadata.get("sources"),
            Some(&json!(["Rust API Guidelines", "Rust Performance Book"]))
        );
    }

    #[test]
    fn test_generate_template() {
        let template = generate_skill_template("my-skill", "Does cool things");
        assert!(template.contains("name: my-skill"));
        assert!(template.contains("description: Does cool things"));
        assert!(template.contains("when-to-use:"));
        assert!(template.contains("when-not-to-use:"));
        assert!(template.contains("## Workflow"));
        assert!(template.contains("templates/` or `assets/"));
    }
}
