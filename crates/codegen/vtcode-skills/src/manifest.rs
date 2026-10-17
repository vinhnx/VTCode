//! SKILL.md manifest parsing
//!
//! Parses YAML frontmatter from SKILL.md files to extract skill metadata and instructions.

use crate::file_references::FileReferenceValidator;
use crate::types::{SkillManifest, SkillManifestMetadata};
use anyhow::Context;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value as JsonValue;
use std::fs;
use std::path::Path;

/// Supported YAML frontmatter keys for SKILL.md validation.
pub(crate) const SUPPORTED_FRONTMATTER_KEYS: &[&str] = &[
    "name",
    "description",
    "license",
    "allowed-tools",
    "argument-hint",
    "disable-model-invocation",
    "compatibility",
    "hooks",
    "metadata",
];

/// Coerce `argument-hint` to a string.
///
/// Claude Code coerces non-string values (e.g. YAML sequences such as
/// `[topic: foo | bar]`) to a string instead of failing; match that so
/// third-party skills do not fail to parse here.
fn deserialize_argument_hint_opt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<JsonValue>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        JsonValue::Null => None,
        JsonValue::String(s) => Some(s),
        JsonValue::Bool(b) => Some(b.to_string()),
        JsonValue::Number(n) => Some(n.to_string()),
        JsonValue::Array(items) => {
            let parts: Vec<String> = items
                .into_iter()
                .filter_map(|item| match item {
                    JsonValue::Null => None,
                    JsonValue::String(s) => Some(s),
                    JsonValue::Bool(b) => Some(b.to_string()),
                    JsonValue::Number(n) => Some(n.to_string()),
                    other => Some(other.to_string()),
                })
                .collect();
            if parts.is_empty() { None } else { Some(parts.join(" ")) }
        }
        // Objects have no scalar form; keep compact JSON rather than failing.
        other => Some(other.to_string()),
    }))
}

/// YAML frontmatter structure for SKILL.md
#[derive(Debug, Serialize, Deserialize)]
pub struct SkillYaml {
    pub(crate) name: String,
    pub(crate) description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "allowed-tools")]
    allowed_tools: Option<AllowedToolsField>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "deserialize_argument_hint_opt"
    )]
    #[serde(rename = "argument-hint")]
    #[serde(alias = "argument_hint")]
    argument_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "disable-model-invocation")]
    #[serde(alias = "disable_model_invocation")]
    disable_model_invocation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hooks: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<SkillManifestMetadata>,
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
    anyhow::ensure!(skill_md.exists(), "SKILL.md not found at {}", skill_md.display());

    let content =
        fs::read_to_string(&skill_md).context(format!("Failed to read SKILL.md at {}", skill_md.display()))?;

    let (manifest, instructions) = parse_skill_content(&content)?;

    // Directory-name match is a spec SHOULD, not a load gate: warn and load
    // anyway so skills authored for other clients (whose directory was renamed
    // on install) still work. `vtcode skills validate` still surfaces the
    // mismatch via the comprehensive validator. Safe to load: `Skill::new`
    // does not depend on the directory name, and discovery keys collisions by
    // manifest name.
    if let Err(err) = manifest.validate_directory_name_match(&skill_md) {
        tracing::warn!("{}; loading skill anyway", err);
    }

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

/// Collect unknown top-level frontmatter keys from a YAML string.
///
/// Only keys at column 0 are examined; nested keys indented under a supported
/// parent (e.g. `metadata:`) are not flagged. Returns keys in first-seen
/// order, deduplicated. This is a pure helper extracted so the filtering logic
/// is independently testable without capturing `tracing` output.
fn collect_unknown_frontmatter_keys(yaml_str: &str) -> Vec<&str> {
    let mut unknown_keys: Vec<&str> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for line in yaml_str.lines() {
        // Only top-level keys begin at column 0; indented lines are nested
        // under a parent (e.g. `metadata:`) and must not be flagged.
        match line.as_bytes().first() {
            None => continue,
            Some(&b) if b == b' ' || b == b'\t' => continue,
            _ => {}
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(colon_pos) = trimmed.find(':') {
            let key = trimmed[..colon_pos].trim();
            if !key.is_empty()
                && !key.starts_with('#')
                && !SUPPORTED_FRONTMATTER_KEYS.contains(&key)
                && seen.insert(key)
            {
                unknown_keys.push(key);
            }
        }
    }
    unknown_keys
}

/// Validate that all YAML frontmatter keys are in the supported set.
///
/// Unknown **top-level** keys are logged as a single consolidated warning but
/// do not fail parsing, preserving forward compatibility when newer vtcode
/// versions add new fields. Nested keys under a supported parent (e.g.
/// `metadata:`) are not flagged. Consolidating to one warning per skill (with
/// all unknown keys listed once) avoids the per-key log spam that previously
/// produced ~180 warning lines per startup, each repeating the full
/// supported-keys list.
fn validate_frontmatter_keys(yaml_str: &str) {
    let unknown_keys = collect_unknown_frontmatter_keys(yaml_str);
    if !unknown_keys.is_empty() {
        tracing::warn!(
            unknown_keys = ?unknown_keys,
            supported = ?SUPPORTED_FRONTMATTER_KEYS,
            "SKILL.md frontmatter has {} unknown top-level key(s); they are ignored but may indicate a typo or a field this vtcode version does not recognize yet",
            unknown_keys.len()
        );
    }
}

/// Parse SKILL.md content string
pub fn parse_skill_content(content: &str) -> anyhow::Result<(SkillManifest, String)> {
    // Split YAML frontmatter (between --- markers)
    let parts: Vec<&str> = content.splitn(3, "---").collect();

    anyhow::ensure!(parts.len() >= 3, "SKILL.md must start with YAML frontmatter: --- ... ---");

    let yaml_str = parts[1].trim();
    let instructions = parts[2].trim_start().to_string();

    // Validate frontmatter keys before parsing. This replaces the stricter
    // #[serde(deny_unknown_fields)] with a forward-compatible approach:
    // unknown keys are warned about but do not fail parsing.
    validate_frontmatter_keys(yaml_str);

    // Parse YAML frontmatter
    let yaml: SkillYaml = match serde_saphyr::from_str(yaml_str) {
        Ok(yaml) => yaml,
        Err(first_err) => match fold_bare_description_to_block_scalar(yaml_str) {
            Some(fixed) => {
                tracing::debug!("SKILL.md frontmatter needed description block-scalar fallback to parse");
                serde_saphyr::from_str(&fixed)
                    .with_context(|| format!("Failed to parse SKILL.md YAML frontmatter ({first_err:#})"))?
            }
            None => return Err(first_err).context("Failed to parse SKILL.md YAML frontmatter"),
        },
    };

    let name = yaml.name.trim().to_string();
    anyhow::ensure!(!name.is_empty(), "name is required and must not be empty");

    let description = yaml.description.trim().to_string();
    anyhow::ensure!(!description.is_empty(), "description is required and must not be empty");

    // Convert allowed-tools into space-delimited string for compatibility.
    // Both the space-delimited string and the YAML list forms are accepted
    // (Claude Code supports YAML lists); normalization is silent.
    let allowed_tools_string = yaml.allowed_tools.map(normalize_allowed_tools).transpose()?;

    let argument_hint = yaml
        .argument_hint
        .map(|hint| hint.trim().to_string())
        .filter(|hint| !hint.is_empty());

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
        disable_model_invocation: yaml.disable_model_invocation,
        when_to_use: None,
        when_not_to_use: None,
        argument_hint,
        user_invocable: None,
        context: None,
        agent: None,
        hooks: yaml.hooks,
        requires_container: None,
        disallow_container: None,
        compatibility: yaml.compatibility,
        variety: crate::types::SkillVariety::AgentSkill,
        metadata: yaml.metadata,
        tools: None,
        network_policy: None,
        permissions: None,
    };

    manifest.validate()?;

    Ok((manifest, instructions))
}
/// Whether `line` looks like a new top-level `key:` mapping entry.
///
/// Column-0 lines starting a mapping key terminate description folding; `- `
/// sequence entries, comments, blank lines, and `scheme://...` runs (colon not
/// followed by space/end) stay inside the folded value, matching YAML plain
/// scalar rules closely enough for a last-resort retry.
fn looks_like_top_level_key(line: &str) -> bool {
    match line.as_bytes().first() {
        None | Some(b' ') | Some(b'\t') | Some(b'#') | Some(b'-') => return false,
        _ => {}
    }
    let trimmed = line.trim_start();
    let Some(colon_pos) = trimmed.find(':') else {
        return false;
    };
    if colon_pos == 0 {
        return false;
    }
    matches!(trimmed.as_bytes().get(colon_pos + 1), None | Some(b' ') | Some(b'\t'))
}

/// Retry helper for cross-client SKILL.md files whose unquoted `description:`
/// value contains a colon (invalid YAML that lenient parsers accept, e.g.
/// `description: Use this skill when: the user asks about PDFs`).
///
/// Rewrites the description value as a `|` block scalar and returns the
/// rewritten frontmatter, or `None` when there is no bare top-level
/// `description:` key to repair. Only runs after a hard parse failure, so it
/// can never break a file that already parses.
fn fold_bare_description_to_block_scalar(yaml_str: &str) -> Option<String> {
    const KEY: &str = "description:";
    let lines: Vec<&str> = yaml_str.lines().collect();
    let desc_idx = lines.iter().position(|line| {
        if matches!(line.as_bytes().first(), None | Some(b' ') | Some(b'\t') | Some(b'#')) {
            return false;
        }
        let trimmed = line.trim_start();
        if !trimmed.starts_with(KEY) {
            return false;
        }
        matches!(trimmed.as_bytes().get(KEY.len()), None | Some(b' ') | Some(b'\t'))
    })?;
    let first_value = lines[desc_idx].trim_start()[KEY.len()..].trim_start();
    // Only engage for the classic failure: an unquoted scalar containing a
    // colon. Explicit `|`/`>`/quoted scalars and empty values fail elsewhere.
    if first_value.is_empty() || first_value.starts_with(['|', '>', '"', '\'']) || !first_value.contains(':') {
        return None;
    }

    let mut rebuilt: Vec<String> = lines[..desc_idx].iter().map(|line| line.to_string()).collect();
    rebuilt.push("description: |".to_string());
    rebuilt.push(format!("  {first_value}"));
    let mut folding = true;
    for line in &lines[desc_idx + 1..] {
        if folding && looks_like_top_level_key(line) {
            folding = false;
        }
        if folding && !line.is_empty() {
            rebuilt.push(format!("  {}", line.trim_start()));
        } else {
            rebuilt.push(line.to_string());
        }
    }
    Some(rebuilt.join("\n"))
}

fn normalize_allowed_tools(field: AllowedToolsField) -> anyhow::Result<String> {
    match field {
        AllowedToolsField::List(tools) => {
            let normalized = tools.join(" ");
            if normalized.trim().is_empty() {
                return Err(anyhow::anyhow!("allowed-tools must not be empty if specified"));
            }
            tracing::debug!("normalized allowed-tools from YAML list to space-delimited string");
            Ok(normalized)
        }
        AllowedToolsField::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(anyhow::anyhow!("allowed-tools must not be empty if specified"));
            }
            let has_commas = trimmed.contains(',');
            if has_commas {
                tracing::debug!("normalized allowed-tools from comma-separated to space-delimited");
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
name: {name}
description: {description}
license: Apache-2.0
# Optional fields (uncomment to use):
# compatibility: "Requires git and network access"
# allowed-tools: "Read Write Bash"
# argument-hint: "[expected argument]"
# disable-model-invocation: true
# metadata:
#   author: your-team
#   version: "1.0"
---

# {skill_title}

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

## Gotchas

- List non-obvious facts the agent will get wrong without being told.
- Example: soft-delete filters, ID aliases across systems, health-check quirks.

## Output Format

Use this shape for the final artifact; adapt sections as needed:

```markdown
# [Title]

## Summary
[One-paragraph overview]

## Findings
- Finding with supporting data

## Next Steps
1. Specific actionable step
```

For longer templates, store them under `assets/` and reference them here.

## Checklist

- [ ] Confirm trigger matches `description` routing
- [ ] Load only needed `references/` files
- [ ] Run bundled `scripts/` instead of retyping logic
- [ ] Validate output before finishing

## Validation

1. Make the change or produce the artifact.
2. Run validation: `python3 scripts/validate.py --help` (or skill-specific validator).
3. If validation fails, fix and re-run. Only finish when validation passes.

## Notes

- Keep SKILL.md concise; move deep detail into `references/` files.
- If output needs a fixed shape, store a starter template or asset alongside the skill.
- Prefer one default tool path with a brief escape hatch over a menu of options.
- Describe reusable procedures, not single-instance answers.
""#
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
        result.unwrap_err();
    }

    #[test]
    fn test_parse_skill_accepts_non_spec_fields_with_warning() {
        // Unknown frontmatter keys are now warned about but do not fail parsing,
        // preserving forward compatibility when newer vtcode versions add fields.
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

        let (manifest, _) = parse_skill_content(content)
            .expect("unknown frontmatter keys should be accepted for forward compatibility");
        assert_eq!(manifest.name, "sandboxed-skill");
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
        result.unwrap_err();
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
        assert_eq!(metadata.get("sources"), Some(&json!(["Rust API Guidelines", "Rust Performance Book"])));
    }

    #[test]
    fn test_parse_skill_disable_model_invocation_flag() {
        let content = r#"---
name: command-skill
description: A skill hidden from model-driven activation
disable-model-invocation: true
---

# Command Skill
"#;

        let (manifest, _) = parse_skill_content(content).expect("flag should parse");
        assert_eq!(manifest.disable_model_invocation, Some(true));
    }

    #[test]
    fn test_parse_skill_description_with_bare_colon() {
        let content = r#"---
name: colon-skill
description: Use this skill when: the user asks about PDFs
---

# Body
"#;

        let (manifest, _) = parse_skill_content(content).expect("bare colon should fold to block scalar");
        assert_eq!(manifest.name, "colon-skill");
        assert_eq!(manifest.description, "Use this skill when: the user asks about PDFs");
    }

    #[test]
    fn test_parse_skill_multiline_description_with_colon() {
        let content = "---\nname: multi-skill\ndescription: First line: overview\ncontinued second line\nlicense: MIT\n---\n\n# Body\n";

        let (manifest, _) = parse_skill_content(content).expect("multiline description should fold");
        assert_eq!(manifest.name, "multi-skill");
        assert!(manifest.description.contains("First line: overview"), "got: {}", manifest.description);
        assert!(manifest.description.contains("continued second line"), "got: {}", manifest.description);
        assert_eq!(manifest.license.as_deref(), Some("MIT"));
    }

    #[test]
    fn test_parse_skill_garbage_frontmatter_still_fails() {
        let content = "---\nname: [unclosed\ndescription: Broken\n---\n\n# Body\n";

        let err = parse_skill_content(content).expect_err("unrelated YAML failure must still error");
        assert!(err.to_string().contains("frontmatter"), "got: {err:#}");
    }

    #[test]
    fn test_generate_template() {
        let template = generate_skill_template("my-skill", "Does cool things");
        assert!(template.contains("name: my-skill"));
        assert!(template.contains("description: Does cool things"));
        assert!(template.contains("license: Apache-2.0"));
        assert!(template.contains("## Workflow"));
        assert!(template.contains("assets/`: reusable output skeletons"));
        assert!(template.contains("## Gotchas"));
        assert!(template.contains("## Output Format"));
        assert!(template.contains("## Checklist"));
        assert!(template.contains("## Validation"));
    }

    #[test]
    fn collect_unknown_frontmatter_keys_ignores_nested_keys() {
        // Nested keys under `metadata:` (a supported key) must NOT be flagged.
        // This is the regression that produced ~180 false-positive warning
        // lines per startup: author/version/sources/category are nested under
        // metadata in well-formed third-party skills, not top-level.
        let yaml = "name: test\ndescription: test\nmetadata:\n  author: leo\n  version: \"1.0\"\n  sources:\n    - a\n    - b\n  category: foo\n  backend: bar\n";
        let unknown = collect_unknown_frontmatter_keys(yaml);
        assert!(unknown.is_empty(), "nested keys under a supported parent must not be flagged, got {unknown:?}");
    }

    #[test]
    fn collect_unknown_frontmatter_keys_flags_top_level_only() {
        // `permissions` and `backend` are top-level unknown keys; `file_system`
        // and `write` are nested under `permissions` and must be skipped.
        let yaml =
            "name: test\ndescription: test\npermissions:\n  file_system:\n    write:\n      - outputs\nbackend: foo\n";
        let unknown = collect_unknown_frontmatter_keys(yaml);
        assert_eq!(unknown, vec!["permissions", "backend"]);
    }

    #[test]
    fn collect_unknown_frontmatter_keys_deduplicates() {
        let yaml = "name: test\ndescription: test\nbackend: foo\nbackend: bar\n";
        let unknown = collect_unknown_frontmatter_keys(yaml);
        assert_eq!(unknown, vec!["backend"]);
    }

    #[test]
    fn collect_unknown_frontmatter_keys_skips_comments_and_blank_lines() {
        let yaml = "# a comment\nname: test\n\ndescription: test\n# another\n";
        let unknown = collect_unknown_frontmatter_keys(yaml);
        assert!(unknown.is_empty());
    }

    #[test]
    fn collect_unknown_frontmatter_keys_preserves_first_seen_order() {
        let yaml = "name: test\ndescription: test\nzee: 1\nalpha: 2\nmid: 3\n";
        let unknown = collect_unknown_frontmatter_keys(yaml);
        assert_eq!(unknown, vec!["zee", "alpha", "mid"]);
    }

    #[test]
    fn collect_unknown_frontmatter_keys_accepts_argument_hint() {
        let yaml = "name: test\ndescription: test\nargument-hint: \"<input>\"\n";
        let unknown = collect_unknown_frontmatter_keys(yaml);
        assert!(unknown.is_empty(), "argument-hint is supported, got {unknown:?}");
    }

    #[test]
    fn parse_skill_content_accepts_codemod_style_frontmatter() {
        let content = r#"---
name: codemod
description: Use Codemod CLI whenever the user wants to migrate something.
allowed-tools:
  - Bash(codemod *)
argument-hint: "<migration-intent>"
---

# Codemod
"#;
        let (manifest, _) = parse_skill_content(content).expect("codemod-style frontmatter should parse");
        assert_eq!(manifest.allowed_tools.as_deref(), Some("Bash(codemod *)"));
        assert_eq!(manifest.argument_hint.as_deref(), Some("<migration-intent>"));
    }

    #[test]
    fn parse_skill_content_coerces_sequence_argument_hint() {
        let content =
            "---\nname: seq-skill\ndescription: Test skill\nargument-hint:\n  - topic\n  - foo\n---\n\n# Body\n";
        let (manifest, _) = parse_skill_content(content).expect("sequence argument-hint should coerce");
        assert_eq!(manifest.argument_hint.as_deref(), Some("topic foo"));
    }

    #[test]
    fn parse_skill_content_rejects_empty_allowed_tools_list() {
        let content = "---\nname: empty-tools\ndescription: Test skill\nallowed-tools: []\n---\n\n# Body\n";
        let err = parse_skill_content(content).expect_err("empty allowed-tools list must fail");
        assert!(err.to_string().contains("allowed-tools"), "got: {err:#}");
    }

    #[test]
    fn parse_skill_file_loads_despite_directory_name_mismatch() {
        // Agent Skills client guide: directory-name mismatch warns but loads,
        // so skills renamed on install (cross-client) still work.
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let skill_dir = tmp.path().join("renamed-dir");
        fs::create_dir(&skill_dir).expect("create skill dir");
        fs::write(skill_dir.join("SKILL.md"), "---\nname: original-name\ndescription: Test skill\n---\n\n# Body\n")
            .expect("write SKILL.md");

        let (manifest, _) = parse_skill_file(&skill_dir).expect("mismatched directory must still load");
        assert_eq!(manifest.name, "original-name");
    }
}
