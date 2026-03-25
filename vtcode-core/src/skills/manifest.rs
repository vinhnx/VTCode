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
    "license",
    "allowed-tools",
    "compatibility",
    "metadata",
];

/// YAML frontmatter structure for SKILL.md
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillYaml {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "allowed-tools")]
    pub allowed_tools: Option<AllowedToolsField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SkillManifestMetadata>,
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

    let (manifest, instructions) = parse_skill_content(&content)?;

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

    let name = yaml.name.trim().to_string();
    anyhow::ensure!(!name.is_empty(), "name is required and must not be empty");

    let description = yaml.description.trim().to_string();
    anyhow::ensure!(
        !description.is_empty(),
        "description is required and must not be empty"
    );

    // Convert allowed-tools into space-delimited string for compatibility
    let allowed_tools_string = yaml
        .allowed_tools
        .map(normalize_allowed_tools)
        .transpose()?;

    let manifest = SkillManifest {
        name,
        description,
        version: None,
        default_version: None,
        latest_version: None,
        author: None,
        license: yaml.license,
        model: None,
        mode: None,
        vtcode_native: None,
        allowed_tools: allowed_tools_string,
        disable_model_invocation: None,
        when_to_use: None,
        when_not_to_use: None,
        argument_hint: None,
        user_invocable: None,
        context: None,
        agent: None,
        hooks: None,
        requires_container: None,
        disallow_container: None,
        compatibility: yaml.compatibility,
        variety: crate::skills::types::SkillVariety::AgentSkill,
        metadata: yaml.metadata,
        tools: None,
        network_policy: None,
        permissions: None,
    };

    manifest.validate()?;

    Ok((manifest, instructions))
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
license: Apache-2.0
# Optional fields (uncomment to use):
# compatibility: "Requires git and network access"
# allowed-tools: "Read Write Bash"
# metadata:
#   author: your-team
#   version: "1.0"
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
- `assets/`: reusable output skeletons, examples, or supporting files

## Example

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
    fn test_parse_skill_rejects_non_spec_fields() {
        let content = r#"---
name: sandboxed-skill
description: A skill with unsupported fields
permissions:
  file_system:
    write:
      - outputs
---

# Instructions
"#;

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
        assert!(template.contains("license: Apache-2.0"));
        assert!(template.contains("## Workflow"));
        assert!(template.contains("assets/`: reusable output skeletons"));
    }
}
