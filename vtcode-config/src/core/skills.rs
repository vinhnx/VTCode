//! Skills configuration
//!
//! Configuration for VT Code skills system, including rendering modes
//! and discovery settings.

use serde::{Deserialize, Serialize};

/// Skills system configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct SkillsConfig {
	/// Rendering mode for skills in system prompt
	/// - "lean": Codex-style minimal (name + description + path only, 40-60% token savings)
	/// - "full": Full metadata with version, author, native flags
	#[serde(default = "default_render_mode")]
	pub render_mode: SkillsRenderMode,

	/// Maximum number of skills to show in system prompt
	#[serde(default = "default_max_skills_in_prompt")]
	pub max_skills_in_prompt: usize,

	/// Enable auto-trigger on $skill-name mentions
	#[serde(default = "default_enable_auto_trigger")]
	pub enable_auto_trigger: bool,

	/// Enable description-based keyword matching for auto-trigger
	#[serde(default = "default_enable_description_matching")]
	pub enable_description_matching: bool,

	/// Minimum keyword matches required for description-based trigger
	#[serde(default = "default_min_keyword_matches")]
	pub min_keyword_matches: usize,
}

impl Default for SkillsConfig {
	fn default() -> Self {
		Self {
			render_mode: default_render_mode(),
			max_skills_in_prompt: default_max_skills_in_prompt(),
			enable_auto_trigger: default_enable_auto_trigger(),
			enable_description_matching: default_enable_description_matching(),
			min_keyword_matches: default_min_keyword_matches(),
		}
	}
}

/// Skills rendering mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SkillsRenderMode {
	/// Lean mode (Codex-style): name + description + path only
	Lean,
	/// Full mode: all metadata including version, author, native flags
	Full,
}

impl Default for SkillsRenderMode {
	fn default() -> Self {
		Self::Lean
	}
}

fn default_render_mode() -> SkillsRenderMode {
	SkillsRenderMode::Lean
}

fn default_max_skills_in_prompt() -> usize {
	10
}

fn default_enable_auto_trigger() -> bool {
	true
}

fn default_enable_description_matching() -> bool {
	true
}

fn default_min_keyword_matches() -> usize {
	2
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_default_skills_config() {
		let config = SkillsConfig::default();
		assert_eq!(config.render_mode, SkillsRenderMode::Lean);
		assert_eq!(config.max_skills_in_prompt, 10);
		assert!(config.enable_auto_trigger);
		assert!(config.enable_description_matching);
		assert_eq!(config.min_keyword_matches, 2);
	}

	#[test]
	fn test_skills_render_mode_serde() {
		// Test serialization
		let lean = SkillsRenderMode::Lean;
		let lean_json = serde_json::to_string(&lean).unwrap();
		assert_eq!(lean_json, r#""lean""#);

		let full = SkillsRenderMode::Full;
		let full_json = serde_json::to_string(&full).unwrap();
		assert_eq!(full_json, r#""full""#);

		// Test deserialization
		let lean_de: SkillsRenderMode = serde_json::from_str(r#""lean""#).unwrap();
		assert_eq!(lean_de, SkillsRenderMode::Lean);

		let full_de: SkillsRenderMode = serde_json::from_str(r#""full""#).unwrap();
		assert_eq!(full_de, SkillsRenderMode::Full);
	}

	#[test]
	fn test_skills_config_serde() {
		let config = SkillsConfig {
			render_mode: SkillsRenderMode::Full,
			max_skills_in_prompt: 15,
			enable_auto_trigger: false,
			enable_description_matching: false,
			min_keyword_matches: 3,
		};

		let json = serde_json::to_string_pretty(&config).unwrap();
		let deserialized: SkillsConfig = serde_json::from_str(&json).unwrap();
		assert_eq!(config, deserialized);
	}
}
