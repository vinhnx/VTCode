//! Agent Skills type definitions
//!
//! Defines core types for Anthropic Agent Skills integration, including
//! skill metadata, manifest parsing, and resource management.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Skill scope indicating where the skill is defined
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    /// User-level skill (~/.vtcode/skills or ~/.claude/skills)
    #[default]
    User,
    /// Repository-level skill (.vtcode/skills or .codex/skills in project root)
    Repo,
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
    /// Optional license string for the skill
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Optional model preference (inherits session if unset)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Marks the skill as a mode command (displayed prominently)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<bool>,
    /// Indicates if skill uses VT Code native features (not container skills)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "vtcode-native")]
    #[serde(alias = "vtcode_native")]
    pub vtcode_native: Option<bool>,
    /// Space-delimited list of pre-approved tools (Agent Skills spec)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "allowed-tools")]
    #[serde(alias = "allowed_tools")]
    pub allowed_tools: Option<String>,
    /// Optional guard to disable direct model invocations when skill is active
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "disable-model-invocation")]
    #[serde(alias = "disable_model_invocation")]
    pub disable_model_invocation: Option<bool>,
    /// Optional guidance on when to use the skill (Claude frontmatter best practice)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "when-to-use")]
    #[serde(alias = "when_to_use")]
    pub when_to_use: Option<String>,
    /// Indicates the skill explicitly requires container skills
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "requires-container")]
    #[serde(alias = "requires_container")]
    pub requires_container: Option<bool>,
    /// Indicates the skill should not be run inside a container (force VTCode-native path)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "disallow-container")]
    #[serde(alias = "disallow_container")]
    pub disallow_container: Option<bool>,
    /// Environment/platform requirements (1-500 chars, Agent Skills spec)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    /// Arbitrary key-value metadata (Agent Skills spec)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

impl SkillManifest {
    /// Validate manifest against Anthropic spec
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.name.is_empty() || self.name.len() > 64 {
            anyhow::bail!("Skill name must be 1-64 characters");
        }

        if !self
            .name
            .chars()
            .all(|c| c.is_lowercase() || c.is_numeric() || c == '-')
        {
            anyhow::bail!("Skill name must contain only lowercase letters, numbers, and hyphens");
        }

        if self.name.contains("anthropic") || self.name.contains("claude") {
            anyhow::bail!("Skill name cannot contain 'anthropic' or 'claude'");
        }

        if self.description.is_empty() || self.description.len() > 1024 {
            anyhow::bail!("Skill description must be 1-1024 characters");
        }

        if let (Some(true), Some(true)) = (self.requires_container, self.disallow_container) {
            anyhow::bail!(
                "Skill manifest cannot set both requires-container and disallow-container"
            );
        }

        if let Some(when_to_use) = &self.when_to_use
            && when_to_use.len() > 512
        {
            anyhow::bail!("when-to-use must be 0-512 characters");
        }

        if let Some(allowed_tools) = &self.allowed_tools {
            // Parse space-delimited string per Agent Skills spec
            let tools: Vec<&str> = allowed_tools.split_whitespace().collect();

            if tools.len() > 16 {
                anyhow::bail!("allowed-tools must list at most 16 tools");
            }

            if tools.is_empty() {
                anyhow::bail!("allowed-tools must not be empty if specified");
            }
        }

        if let Some(license) = &self.license
            && license.len() > 512
        {
            anyhow::bail!("license must be 0-512 characters");
        }

        if let Some(model) = &self.model
            && model.len() > 128
        {
            anyhow::bail!("model must be 0-128 characters");
        }

        if let Some(compatibility) = &self.compatibility
            && (compatibility.is_empty() || compatibility.len() > 500)
        {
            anyhow::bail!("compatibility must be 1-500 characters");
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
            || path.to_string_lossy().contains(".codex/skills")
        {
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
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
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
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
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
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
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
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
        };

        let meta_ctx = SkillContext::MetadataOnly(manifest.clone());
        assert_eq!(meta_ctx.tokens(), 100);
    }

    #[test]
    fn test_compatibility_validation() {
        // Valid compatibility
        let m = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            requires_container: None,
            disallow_container: None,
            compatibility: Some("Designed for VTCode".to_string()),
            metadata: None,
        };
        assert!(m.validate().is_ok());

        // Invalid: empty compatibility
        let m = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            requires_container: None,
            disallow_container: None,
            compatibility: Some("".to_string()),
            metadata: None,
        };
        assert!(m.validate().is_err());

        // Invalid: too long (> 500 chars)
        let m = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            requires_container: None,
            disallow_container: None,
            compatibility: Some("a".repeat(501)),
            metadata: None,
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_allowed_tools_string_format() {
        // Valid space-delimited string
        let m = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: Some("Read Write Bash".to_string()),
            disable_model_invocation: None,
            when_to_use: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
        };
        assert!(m.validate().is_ok());

        // Invalid: empty string
        let m = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: Some("".to_string()),
            disable_model_invocation: None,
            when_to_use: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
        };
        assert!(m.validate().is_err());

        // Invalid: too many tools (> 16)
        let tools = (0..17).map(|i| format!("Tool{}", i)).collect::<Vec<_>>().join(" ");
        let m = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: Some(tools),
            disable_model_invocation: None,
            when_to_use: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: None,
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_metadata_field() {
        let mut metadata = HashMap::new();
        metadata.insert("author".to_string(), "Test Author".to_string());
        metadata.insert("version".to_string(), "1.0.0".to_string());

        let m = SkillManifest {
            name: "test-skill".to_string(),
            description: "Test description".to_string(),
            version: None,
            author: None,
            license: None,
            model: None,
            mode: None,
            vtcode_native: None,
            allowed_tools: None,
            disable_model_invocation: None,
            when_to_use: None,
            requires_container: None,
            disallow_container: None,
            compatibility: None,
            metadata: Some(metadata),
        };
        assert!(m.validate().is_ok());
    }
}
