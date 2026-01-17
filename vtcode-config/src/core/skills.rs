//! Skills configuration
//!
//! Configuration for VT Code skills system, including rendering modes
//! and discovery settings.
//!
//! ## Current Implementation Note
//!
//! As of v0.50.7, VT Code implements skills as **callable tools** (via Tool trait),
//! not as prompt text in the system prompt. Skills are loaded on-demand via
//! `/skills load <name>` commands and registered in the tool registry.
//!
//! The `prompt_format` and `render_mode` configs are currently **unused** but
//! available for future features such as:
//! - Optional skills summary in system prompt (opt-in via config flag)
//! - Rich formatting for `/skills list` command output
//! - Documentation generation
//!
//! Per Agent Skills specification: Skills are loaded on-demand, not auto-loaded.

use serde::{Deserialize, Serialize};

/// Skills system configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub struct SkillsConfig {
    /// Rendering mode for skills in system prompt
    /// - "lean": Codex-style minimal (name + description + path only, 40-60% token savings)
    /// - "full": Full metadata with version, author, native flags
    #[serde(default = "default_render_mode")]
    pub render_mode: SkillsRenderMode,

    /// Prompt format for skills section (Agent Skills spec)
    /// - "xml": XML wrapping for safety (Claude models default)
    /// - "markdown": Plain markdown sections
    #[serde(default = "default_prompt_format")]
    pub prompt_format: PromptFormat,

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
            prompt_format: default_prompt_format(),
            max_skills_in_prompt: default_max_skills_in_prompt(),
            enable_auto_trigger: default_enable_auto_trigger(),
            enable_description_matching: default_enable_description_matching(),
            min_keyword_matches: default_min_keyword_matches(),
        }
    }
}

/// Skills rendering mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum SkillsRenderMode {
    /// Lean mode (Codex-style): name + description + path only
    #[default]
    Lean,
    /// Full mode: all metadata including version, author, native flags
    Full,
}

/// Prompt format for skills section (Agent Skills spec)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum PromptFormat {
    /// XML wrapping for safety (Claude models default, per Agent Skills spec)
    #[default]
    Xml,
    /// Plain markdown sections
    Markdown,
}

fn default_render_mode() -> SkillsRenderMode {
    SkillsRenderMode::Lean
}

fn default_prompt_format() -> PromptFormat {
    PromptFormat::Xml
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
        assert_eq!(config.prompt_format, PromptFormat::Xml);
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
    fn test_prompt_format_serde() {
        // Test serialization
        let xml = PromptFormat::Xml;
        let xml_json = serde_json::to_string(&xml).unwrap();
        assert_eq!(xml_json, r#""xml""#);

        let markdown = PromptFormat::Markdown;
        let markdown_json = serde_json::to_string(&markdown).unwrap();
        assert_eq!(markdown_json, r#""markdown""#);

        // Test deserialization
        let xml_de: PromptFormat = serde_json::from_str(r#""xml""#).unwrap();
        assert_eq!(xml_de, PromptFormat::Xml);

        let markdown_de: PromptFormat = serde_json::from_str(r#""markdown""#).unwrap();
        assert_eq!(markdown_de, PromptFormat::Markdown);
    }

    #[test]
    fn test_skills_config_serde() {
        let config = SkillsConfig {
            render_mode: SkillsRenderMode::Full,
            prompt_format: PromptFormat::Markdown,
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
