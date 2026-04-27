//! Skills prompt integration
//!
//! Dynamically injects available skills information into system prompt,
//! similar to OpenAI Codex's approach.

use crate::skills::model::{SkillMetadata, SkillScope};
use std::fmt::Write;

// Re-export PromptFormat from config for consistency
pub use vtcode_config::core::skills::PromptFormat;

/// Rendering mode for skills section
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SkillsRenderMode {
    /// Full metadata (currently same core fields as lean mode).
    Full,
    /// Lean mode: only name + description + file path (Codex-style, 40-60% token savings)
    #[default]
    Lean,
}

/// Usage rules embedded in skills section (Codex pattern)
const SKILL_USAGE_RULES: &str = r#"
**Usage Rules:**
- **Discovery**: Skills listed above (name + description + file path)
- **Trigger**: Use skill if user mentions `$SkillName` OR task matches description
- **Progressive disclosure**:
  1. Open SKILL.md to get full instructions
  2. Load referenced files (scripts/, references/) only if needed
  3. Prefer running existing scripts vs. retyping code
- **Missing/blocked**: State issue briefly and continue with fallback approach
- **Routing**: Treat `description` as the primary trigger signal
"#;

/// Generate skills section for system prompt (full mode - backward compatible)
pub fn generate_skills_prompt(skills: &[SkillMetadata]) -> String {
    generate_skills_prompt_with_mode(skills, SkillsRenderMode::Full)
}

/// Generate skills section with specified rendering mode
pub fn generate_skills_prompt_with_mode(
    skills: &[SkillMetadata],
    mode: SkillsRenderMode,
) -> String {
    if skills.is_empty() {
        return String::new();
    }

    match mode {
        SkillsRenderMode::Full => render_skills_full(skills),
        SkillsRenderMode::Lean => render_skills_lean(skills),
    }
}

/// Render skills in full mode.
fn render_skills_full(skills: &[SkillMetadata]) -> String {
    render_skills_lean(skills)
}

/// Render skills in lean mode (Codex-style: name + description + path only)
///
/// This keeps only the metadata required by the strict SKILL.md spec.
fn render_skills_lean(skills: &[SkillMetadata]) -> String {
    let mut prompt = String::from("\n\n## Skills\n");
    prompt.push_str(
        "Available skills (name: description + directory + scope). Content on disk; open SKILL.md when triggered.\n\n",
	);

    // Sort skills by name for stable ordering
    let mut skill_list: Vec<_> = skills.iter().collect();
    skill_list.sort_by_key(|skill| &skill.name);

    // Show up to 10 skills to keep prompt lean
    let overflow = skill_list.len().saturating_sub(10);
    if overflow > 0 {
        skill_list.truncate(10);
    }

    for skill in skill_list {
        let location = skill.path.display().to_string();
        let scope = match skill.scope {
            SkillScope::User => "user",
            SkillScope::Repo => "repo",
            SkillScope::System => "system",
            SkillScope::Admin => "admin",
        };

        let line = format!(
            "- {}: {} (file: {}, scope: {})",
            skill.name, skill.description, location, scope
        );

        let _ = writeln!(prompt, "{}", line);
    }

    if overflow > 0 {
        let _ = write!(prompt, "\n(+{} more skills available)", overflow);
    }

    // Append usage rules (Codex pattern)
    prompt.push_str(SKILL_USAGE_RULES);

    prompt
}

/// Generate skills prompt in XML format (Agent Skills spec recommendation for LLM models)
///
/// Wraps skills in `<available_skills>` tags for improved safety and isolation.
/// This is the recommended format per the Agent Skills specification.
pub fn generate_skills_prompt_xml(skills: &[SkillMetadata]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let mut xml = String::from("\n<available_skills>\n");

    // Sort skills by name for stable ordering
    let mut skill_list: Vec<_> = skills.iter().collect();
    skill_list.sort_by_key(|skill| &skill.name);

    // Show up to 10 skills to keep prompt lean
    let overflow = skill_list.len().saturating_sub(10);
    if overflow > 0 {
        skill_list.truncate(10);
    }

    for skill in skill_list {
        xml.push_str("  <skill>\n");
        let _ = writeln!(xml, "    <name>{}</name>", xml_escape(&skill.name));
        let _ = writeln!(
            xml,
            "    <description>{}</description>",
            xml_escape(&skill.description)
        );
        let _ = writeln!(
            xml,
            "    <location>{}</location>",
            xml_escape(&skill.path.display().to_string())
        );

        // Optional fields per Agent Skills spec
        if let Some(manifest) = &skill.manifest {
            if let Some(ref compatibility) = manifest.compatibility {
                let _ = writeln!(
                    xml,
                    "    <compatibility>{}</compatibility>",
                    xml_escape(compatibility)
                );
            }

            if let Some(ref allowed_tools) = manifest.allowed_tools {
                let _ = writeln!(
                    xml,
                    "    <allowed-tools>{}</allowed-tools>",
                    xml_escape(allowed_tools)
                );
            }
        }

        xml.push_str("  </skill>\n");
    }

    if overflow > 0 {
        let _ = writeln!(xml, "  <!-- +{} more skills available -->", overflow);
    }

    xml.push_str("</available_skills>\n");
    xml
}

/// Escape special XML characters
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Generate skills prompt with format specification
pub fn generate_skills_prompt_with_format(
    skills: &[SkillMetadata],
    render_mode: SkillsRenderMode,
    format: PromptFormat,
) -> String {
    match format {
        PromptFormat::Xml => generate_skills_prompt_xml(skills),
        PromptFormat::Markdown => generate_skills_prompt_with_mode(skills, render_mode),
    }
}

/// Test helper
pub fn test_skills_prompt_generation() {
    use crate::skills::types::SkillManifest;
    use std::path::PathBuf;

    let mut skills = Vec::new();

    let manifest = SkillManifest {
        name: "pdf-analyzer".to_string(),
        description: "Analyze PDF documents".to_string(),
        ..Default::default()
    };

    let skill = SkillMetadata {
        name: manifest.name.clone(),
        description: manifest.description.clone(),
        short_description: None,
        path: PathBuf::from("/tmp/test"),
        scope: SkillScope::User,
        manifest: Some(manifest.into()),
    };

    skills.push(skill);

    let prompt = generate_skills_prompt(&skills);
    assert!(prompt.contains("pdf-analyzer"));
    assert!(prompt.contains("Analyze PDF documents"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_empty_skills() {
        let skills = Vec::new();
        let prompt = generate_skills_prompt(&skills);
        assert!(prompt.is_empty());
    }

    #[test]
    fn test_skills_rendering() {
        test_skills_prompt_generation();
    }

    #[test]
    fn test_lean_rendering_mode() {
        use crate::skills::types::SkillManifest;
        let mut skills = Vec::new();

        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test skill description".to_string(),
            ..Default::default()
        };

        let skill = SkillMetadata {
            name: manifest.name.clone(),
            description: manifest.description.clone(),
            short_description: None,
            path: PathBuf::from("/tmp/test-skill"),
            scope: SkillScope::User,
            manifest: Some(manifest.into()),
        };

        skills.push(skill);

        let lean_prompt = generate_skills_prompt_with_mode(&skills, SkillsRenderMode::Lean);

        // Lean mode should include name, description, and file path.
        assert!(lean_prompt.contains("test-skill"));
        assert!(lean_prompt.contains("Test skill description"));
        assert!(lean_prompt.contains("(file: /tmp/test-skill"));

        // Lean mode should include usage rules
        assert!(lean_prompt.contains("Usage Rules"));
        assert!(lean_prompt.contains("$SkillName"));
    }

    #[test]
    fn test_full_vs_lean_token_savings() {
        use crate::skills::types::SkillManifest;
        let mut skills = Vec::new();

        for i in 0..5 {
            let manifest = SkillManifest {
                name: format!("skill-{}", i),
                description: format!("Example skill number {}", i),
                ..Default::default()
            };

            let skill = SkillMetadata {
                name: manifest.name.clone(),
                description: manifest.description.clone(),
                short_description: None,
                path: PathBuf::from(format!("/path/to/skill-{}", i)),
                scope: SkillScope::User,
                manifest: Some(manifest.into()),
            };

            skills.push(skill);
        }

        let full_prompt = generate_skills_prompt_with_mode(&skills, SkillsRenderMode::Full);
        let lean_prompt = generate_skills_prompt_with_mode(&skills, SkillsRenderMode::Lean);

        assert_eq!(full_prompt, lean_prompt);
        assert!(lean_prompt.contains("Usage Rules"));
        assert!(full_prompt.contains("Available skills"));
    }

    #[test]
    fn test_xml_generation() {
        use crate::skills::types::SkillManifest;
        let mut skills = Vec::new();
        use hashbrown::HashMap as StdHashMap;

        let mut metadata = StdHashMap::new();
        metadata.insert("author".to_string(), serde_json::json!("Test Author"));

        let manifest = SkillManifest {
            name: "test-xml-skill".to_string(),
            description: "Test XML generation".to_string(),
            allowed_tools: Some("Read Write Bash".to_string()),
            compatibility: Some("Designed for VT Code".to_string()),
            metadata: Some(metadata),
            ..Default::default()
        };

        let skill = SkillMetadata {
            name: manifest.name.clone(),
            description: manifest.description.clone(),
            short_description: None,
            path: PathBuf::from("/tmp/test-xml-skill"),
            scope: SkillScope::User,
            manifest: Some(manifest.into()),
        };

        skills.push(skill);

        let xml_prompt = generate_skills_prompt_xml(&skills);

        // Should be wrapped in XML tags
        assert!(xml_prompt.contains("<available_skills>"));
        assert!(xml_prompt.contains("</available_skills>"));
        assert!(xml_prompt.contains("<skill>"));
        assert!(xml_prompt.contains("</skill>"));

        // Should include required fields
        assert!(xml_prompt.contains("<name>test-xml-skill</name>"));
        assert!(xml_prompt.contains("<description>Test XML generation</description>"));
        assert!(xml_prompt.contains("<location>/tmp/test-xml-skill</location>"));

        // Should include optional fields
        assert!(xml_prompt.contains("<compatibility>Designed for VT Code</compatibility>"));
        assert!(xml_prompt.contains("<allowed-tools>Read Write Bash</allowed-tools>"));
    }

    #[test]
    fn test_xml_escaping() {
        use crate::skills::types::SkillManifest;
        let mut skills = Vec::new();

        let manifest = SkillManifest {
            name: "test-escape".to_string(),
            description: "Test <special> & \"characters\"".to_string(),
            ..Default::default()
        };

        let skill = SkillMetadata {
            name: manifest.name.clone(),
            description: manifest.description.clone(),
            short_description: None,
            path: PathBuf::from("/tmp/test"),
            scope: SkillScope::User,
            manifest: Some(manifest.into()),
        };

        skills.push(skill);

        let xml_prompt = generate_skills_prompt_xml(&skills);

        // XML special characters should be escaped
        assert!(xml_prompt.contains("&lt;special&gt;"));
        assert!(xml_prompt.contains("&amp;"));
        assert!(xml_prompt.contains("&quot;"));
    }

    #[test]
    fn test_prompt_format_selection() {
        use crate::skills::types::SkillManifest;
        let mut skills = Vec::new();

        let manifest = SkillManifest {
            name: "test-format".to_string(),
            description: "Test format selection".to_string(),
            ..Default::default()
        };

        let skill = SkillMetadata {
            name: manifest.name.clone(),
            description: manifest.description.clone(),
            short_description: None,
            path: PathBuf::from("/tmp/test"),
            scope: SkillScope::User,
            manifest: Some(manifest.into()),
        };

        skills.push(skill);

        let xml_output =
            generate_skills_prompt_with_format(&skills, SkillsRenderMode::Lean, PromptFormat::Xml);
        let markdown_output = generate_skills_prompt_with_format(
            &skills,
            SkillsRenderMode::Lean,
            PromptFormat::Markdown,
        );

        // XML format should have XML tags
        assert!(xml_output.contains("<available_skills>"));
        assert!(!markdown_output.contains("<available_skills>"));

        // Markdown format should have markdown headers
        assert!(markdown_output.contains("## Skills"));
        assert!(!xml_output.contains("## Skills"));
    }
}
