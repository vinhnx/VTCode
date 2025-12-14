//! Agent Skills type definitions
//!
//! Defines core types for Anthropic Agent Skills integration, including
//! skill metadata, manifest parsing, and resource management.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Skill scope indicating where the skill is defined
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
	/// User-level skill (~/.vtcode/skills or ~/.claude/skills)
	User,
	/// Repository-level skill (.vtcode/skills or .codex/skills in project root)
	Repo,
}

impl Default for SkillScope {
	fn default() -> Self {
		Self::User
	}
}

/// Skill metadata for protocol/API responses (matches Codex protocol)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetadata {
	pub name: String,
	pub description: String,
	pub path: PathBuf,
	pub scope: SkillScope,
}

/// Skill error information (matches Codex protocol)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillErrorInfo {
	pub path: PathBuf,
	pub message: String,
}

/// Skill manifest metadata from SKILL.md frontmatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    /// Unique identifier (lowercase, hyphens, max 64 chars)
    pub name: String,
    /// What the skill does and when to use it (max 1024 chars)
    pub description: String,
    /// Optional version string
    pub version: Option<String>,
    /// Optional author name
    pub author: Option<String>,
    /// Indicates if skill uses VT Code native features (not container skills)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "vtcode-native")]
    #[serde(alias = "vtcode_native")]
    pub vtcode_native: Option<bool>,
}

impl SkillManifest {
    /// Validate manifest against Anthropic spec
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.name.is_empty() || self.name.len() > 64 {
            anyhow::bail!("Skill name must be 1-64 characters");
        }

        if !self.name.chars().all(|c| c.is_lowercase() || c.is_numeric() || c == '-') {
            anyhow::bail!("Skill name must contain only lowercase letters, numbers, and hyphens");
        }

        if self.name.contains("anthropic") || self.name.contains("claude") {
            anyhow::bail!("Skill name cannot contain 'anthropic' or 'claude'");
        }

        if self.description.is_empty() || self.description.len() > 1024 {
            anyhow::bail!("Skill description must be 1-1024 characters");
        }

        Ok(())
    }
}

/// Resource types bundled with a skill
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceType {
    /// Markdown documentation or additional instructions
    Markdown,
    /// Executable script (Python, Shell, etc.)
    Script,
    /// Reference data (JSON, schemas, templates, etc.)
    Reference,
    /// Other resource types
    Other(String),
}

/// A skill resource (Level 3: on-demand loading)
#[derive(Debug, Clone)]
pub struct SkillResource {
    /// Relative path within skill directory
    pub path: String,
    /// Content type
    pub resource_type: ResourceType,
    /// Cached content (None until loaded)
    pub content: Option<Vec<u8>>,
}

/// Complete skill definition with progressive disclosure levels
#[derive(Debug, Clone)]
pub struct Skill {
    /// Level 1: Metadata (~100 tokens, always loaded)
    pub manifest: SkillManifest,

    /// Absolute path to skill directory
    pub path: PathBuf,

	/// Skill scope (user-level or repo-level)
	pub scope: SkillScope,

    /// Level 2: Instructions from SKILL.md body (<5K tokens, loaded when triggered)
    pub instructions: String,

    /// Level 3: Bundled resources (lazy-loaded on demand)
    pub resources: HashMap<String, SkillResource>,
}

impl Skill {
    /// Create a new skill
    pub fn new(
        manifest: SkillManifest,
        path: PathBuf,
        instructions: String,
    ) -> anyhow::Result<Self> {
        manifest.validate()?;
		// Determine scope based on path
		let scope = if path.to_string_lossy().contains(".vtcode/skills") 
			|| path.to_string_lossy().contains(".codex/skills") {
			SkillScope::Repo
		} else {
			SkillScope::User
		};
        Ok(Skill {
            manifest,
            path,
			scope,
            instructions,
            resources: HashMap::new(),
        })
    }

	/// Create a new skill with explicit scope
	pub fn with_scope(
		manifest: SkillManifest,
		path: PathBuf,
		scope: SkillScope,
		instructions: String,
	) -> anyhow::Result<Self> {
		manifest.validate()?;
		Ok(Skill {
			manifest,
			path,
			scope,
			instructions,
			resources: HashMap::new(),
		})
	}

    /// Add a resource to the skill
    pub fn add_resource(&mut self, path: String, resource: SkillResource) {
        self.resources.insert(path, resource);
    }

    /// Get skill's display name
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    /// Get skill's description
    pub fn description(&self) -> &str {
        &self.manifest.description
    }

    /// Estimate token count for instructions (approximate)
    pub fn instruction_tokens(&self) -> usize {
        // Rough estimate: ~1 token per 4 characters
        self.instructions.len() / 4
    }

    /// Check if skill has a resource
    pub fn has_resource(&self, path: &str) -> bool {
        self.resources.contains_key(path)
    }

    /// Get resource by path
    pub fn get_resource(&self, path: &str) -> Option<&SkillResource> {
        self.resources.get(path)
    }

    /// List all available resources
    pub fn list_resources(&self) -> Vec<&str> {
        self.resources.keys().map(|s| s.as_str()).collect()
    }
}

/// Progressive disclosure context for skill loading
#[derive(Debug, Clone)]
pub enum SkillContext {
    /// Level 1: Only metadata (~100 tokens)
    MetadataOnly(SkillManifest),
    /// Level 2: Metadata + instructions (<5K tokens)
    WithInstructions(Skill),
    /// Level 3: Full skill with resources loaded
    Full(Skill),
}

impl SkillContext {
    /// Get manifest from any context level
    pub fn manifest(&self) -> &SkillManifest {
        match self {
            SkillContext::MetadataOnly(m) => m,
            SkillContext::WithInstructions(s) => &s.manifest,
            SkillContext::Full(s) => &s.manifest,
        }
    }

    /// Get skill (requires Level 2+)
    pub fn skill(&self) -> Option<&Skill> {
        match self {
            SkillContext::MetadataOnly(_) => None,
            SkillContext::WithInstructions(s) => Some(s),
            SkillContext::Full(s) => Some(s),
        }
    }

    /// Estimated tokens consumed in system prompt
    pub fn tokens(&self) -> usize {
        match self {
            SkillContext::MetadataOnly(_) => 100,
            SkillContext::WithInstructions(s) => 100 + s.instruction_tokens(),
            SkillContext::Full(s) => 100 + s.instruction_tokens() + (s.resources.len() * 50),
        }
    }
}

/// Skill registry entry (loaded skill + metadata)
#[derive(Debug, Clone)]
pub struct SkillRegistryEntry {
    pub skill: Skill,
    pub enabled: bool,
    pub load_time: std::time::SystemTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_validation_valid() {
        let m = SkillManifest {
            name: "my-skill".to_string(),
            description: "A test skill".to_string(),
            version: None,
            author: None,
            vtcode_native: None,
        };
        assert!(m.validate().is_ok());
    }

    #[test]
    fn test_manifest_validation_invalid_name_length() {
        let m = SkillManifest {
            name: "a".repeat(65),
            description: "Valid description".to_string(),
            version: None,
            author: None,
            vtcode_native: None,
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_manifest_validation_reserved_word() {
        let m = SkillManifest {
            name: "anthropic-skill".to_string(),
            description: "Valid description".to_string(),
            version: None,
            author: None,
            vtcode_native: None,
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_skill_context_tokens() {
        let manifest = SkillManifest {
            name: "test".to_string(),
            description: "Test".to_string(),
            version: None,
            author: None,
            vtcode_native: None,
        };

        let meta_ctx = SkillContext::MetadataOnly(manifest.clone());
        assert_eq!(meta_ctx.tokens(), 100);
    }
}
