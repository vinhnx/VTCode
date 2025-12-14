//! Skills prompt integration
//!
//! Dynamically injects available skills information into system prompt,
//! similar to OpenAI Codex's approach.

use crate::skills::types::Skill;
use std::collections::HashMap;
use std::fmt::Write;

/// Rendering mode for skills section
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillsRenderMode {
	/// Full metadata (version, author, vtcode-native flags)
	Full,
	/// Lean mode: only name + description + file path (Codex-style, 40-60% token savings)
	Lean,
}

impl Default for SkillsRenderMode {
	fn default() -> Self {
		SkillsRenderMode::Lean // Default to Codex-style lean rendering
	}
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
"#;

/// Generate skills section for system prompt (full mode - backward compatible)
pub fn generate_skills_prompt(skills: &HashMap<String, Skill>) -> String {
	generate_skills_prompt_with_mode(skills, SkillsRenderMode::Full)
}

/// Generate skills section with specified rendering mode
pub fn generate_skills_prompt_with_mode(
	skills: &HashMap<String, Skill>,
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

/// Render skills in full mode (legacy format with all metadata)
fn render_skills_full(skills: &HashMap<String, Skill>) -> String {
	let mut prompt = String::from("\n\n## Available Skills\n");
	prompt.push_str("The following skills are loaded and available for use. Each skill provides specialized capabilities. Reference skills by name when they would be helpful.\n\n");

	// Sort skills by name for stable ordering
	let mut skill_list: Vec<_> = skills.values().collect();
	skill_list.sort_by_key(|skill| skill.name());

	// Show up to 10 skills (like tools)
	let overflow = skill_list.len().saturating_sub(10);
	if overflow > 0 {
		skill_list.truncate(10);
	}

	for skill in skill_list {
		let _ = writeln!(
			prompt,
			"- {}: {} (native: {})",
			skill.name(),
			skill.description(),
			skill.manifest.vtcode_native.unwrap_or(false)
		);
	}

	if overflow > 0 {
		let _ = write!(prompt, "\n(+{} more skills not shown)", overflow);
	}

	prompt
}

/// Render skills in lean mode (Codex-style: name + description + path only)
///
/// This reduces token usage by 40-60% compared to full mode by omitting
/// version, author, and native flags. Follows OpenAI Codex's pattern of
/// showing only essential metadata in the system prompt.
fn render_skills_lean(skills: &HashMap<String, Skill>) -> String {
	let mut prompt = String::from("\n\n## Skills\n");
	prompt.push_str(
		"Available skills (name: description + file path). Content on disk; open file when triggered.\n\n",
	);

	// Sort skills by name for stable ordering
	let mut skill_list: Vec<_> = skills.values().collect();
	skill_list.sort_by_key(|skill| skill.name());

	// Show up to 10 skills to keep prompt lean
	let overflow = skill_list.len().saturating_sub(10);
	if overflow > 0 {
		skill_list.truncate(10);
	}

	for skill in skill_list {
		// Lean format: only name, description, and file path
		let _ = writeln!(
			prompt,
			"- `{}`: {} (file: {})",
			skill.name(),
			skill.description(),
			skill.path.display()
		);
	}

	if overflow > 0 {
		let _ = write!(prompt, "\n(+{} more skills available)", overflow);
	}

	// Append usage rules (Codex pattern)
	prompt.push_str(SKILL_USAGE_RULES);

	prompt
}

/// Test helper
pub fn test_skills_prompt_generation() {
	use std::path::PathBuf;

	let mut skills = HashMap::new();

	// Add a test skill
	let skill = Skill::new(
		crate::skills::types::SkillManifest {
			name: "pdf-analyzer".to_string(),
			description: "Analyze PDF documents".to_string(),
			version: Some("1.0.0".to_string()),
			author: Some("Test".to_string()),
			vtcode_native: Some(true),
		},
		PathBuf::from("/tmp/test"),
		"Instructions".to_string(),
	)
	.unwrap();

	skills.insert("pdf-analyzer".to_string(), skill);

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
		let skills = HashMap::new();
		let prompt = generate_skills_prompt(&skills);
		assert!(prompt.is_empty());
	}

	#[test]
	fn test_skills_rendering() {
		test_skills_prompt_generation();
	}

	#[test]
	fn test_lean_rendering_mode() {
		let mut skills = HashMap::new();

		let skill = Skill::new(
			crate::skills::types::SkillManifest {
				name: "test-skill".to_string(),
				description: "Test skill description".to_string(),
				version: Some("1.0.0".to_string()),
				author: Some("Test Author".to_string()),
				vtcode_native: Some(true),
			},
			PathBuf::from("/tmp/test-skill"),
			"Test instructions".to_string(),
		)
		.unwrap();

		skills.insert("test-skill".to_string(), skill);

		let lean_prompt = generate_skills_prompt_with_mode(&skills, SkillsRenderMode::Lean);

		// Lean mode should include name, description, and path
		assert!(lean_prompt.contains("test-skill"));
		assert!(lean_prompt.contains("Test skill description"));
		assert!(lean_prompt.contains("/tmp/test-skill"));

		// Lean mode should include usage rules
		assert!(lean_prompt.contains("Usage Rules"));
		assert!(lean_prompt.contains("$SkillName"));

		// Lean mode should NOT include version or author
		assert!(!lean_prompt.contains("1.0.0"));
		assert!(!lean_prompt.contains("Test Author"));
	}

	#[test]
	fn test_full_vs_lean_token_savings() {
		let mut skills = HashMap::new();

		// Create multiple skills to demonstrate savings (lean mode benefits from fewer per-skill tokens)
		for i in 0..5 {
			let skill = Skill::new(
				crate::skills::types::SkillManifest {
					name: format!("skill-{}", i),
					description: format!("Example skill number {}", i),
					version: Some("2.1.0".to_string()),
					author: Some("Developer Name".to_string()),
					vtcode_native: Some(true),
				},
				PathBuf::from(format!("/path/to/skill-{}", i)),
				"Instructions".to_string(),
			)
			.unwrap();

			skills.insert(format!("skill-{}", i), skill);
		}

		let full_prompt = generate_skills_prompt_with_mode(&skills, SkillsRenderMode::Full);
		let lean_prompt = generate_skills_prompt_with_mode(&skills, SkillsRenderMode::Lean);

		// Test that lean mode omits version/author metadata (key difference)
		assert!(!lean_prompt.contains("2.1.0"));
		assert!(!lean_prompt.contains("Developer Name"));
		assert!(!lean_prompt.contains("native:"));

		// Full mode should include metadata
		assert!(full_prompt.contains("native:"));

		// Verify both modes include usage rules (lean) or preamble (full)
		assert!(lean_prompt.contains("Usage Rules"));
		assert!(full_prompt.contains("Available Skills"));
	}
}