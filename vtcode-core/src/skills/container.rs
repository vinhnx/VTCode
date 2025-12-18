//! Skill container for multi-skill execution
//!
//! Implements Claude API container model for managing multiple skills
//! in a single request with version support and container reuse.
//!
//! Up to 8 skills per container. Container IDs can be reused across
//! multiple turns for state preservation.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Skill source type (Anthropic-managed or custom)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillType {
    /// Pre-built Anthropic skills (pptx, xlsx, docx, pdf, etc.)
    #[serde(rename = "anthropic")]
    Anthropic,
    /// User-uploaded custom skills
    #[serde(rename = "custom")]
    Custom,
}

/// Skill version specification
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum SkillVersion {
    /// Always use latest version
    #[serde(rename = "latest")]
    #[default]
    Latest,
    /// Specific version by ID (epoch timestamp for custom skills, date for Anthropic)
    Specific(String),
}

impl SkillVersion {
    pub fn as_str(&self) -> &str {
        match self {
            SkillVersion::Latest => "latest",
            SkillVersion::Specific(v) => v,
        }
    }

    pub fn is_latest(&self) -> bool {
        matches!(self, SkillVersion::Latest)
    }
}

/// Specification for a single skill in a container
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillSpec {
    /// Type of skill (anthropic or custom)
    #[serde(rename = "type")]
    pub skill_type: SkillType,
    /// Skill identifier (short name for Anthropic, UUID for custom)
    pub skill_id: String,
    /// Version to use (latest or specific epoch timestamp)
    #[serde(default)]
    pub version: SkillVersion,
}

impl SkillSpec {
    /// Create a new skill specification
    pub fn new(skill_type: SkillType, skill_id: impl Into<String>) -> Self {
        Self {
            skill_type,
            skill_id: skill_id.into(),
            version: SkillVersion::Latest,
        }
    }

    /// Create with specific version
    pub fn with_version(mut self, version: SkillVersion) -> Self {
        self.version = version;
        self
    }

    /// Create Anthropic skill (predefined by Anthropic)
    pub fn anthropic(skill_id: impl Into<String>) -> Self {
        Self::new(SkillType::Anthropic, skill_id)
    }

    /// Create custom skill (user-uploaded)
    pub fn custom(skill_id: impl Into<String>) -> Self {
        Self::new(SkillType::Custom, skill_id)
    }
}

/// Container for managing multiple skills in a request
///
/// Implements Claude's container model for multi-skill execution.
/// - Maximum 8 skills per container
/// - Container ID can be reused across multiple turns
/// - Each skill can have independent version pinning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillContainer {
    /// Optional container ID for reuse across turns
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Skills to load in this container (max 8)
    pub skills: Vec<SkillSpec>,
}

impl SkillContainer {
    /// Create a new skill container
    pub fn new() -> Self {
        Self {
            id: None,
            skills: Vec::with_capacity(8), // Pre-allocate for max capacity
        }
    }

    /// Create with single skill
    pub fn single(spec: SkillSpec) -> Self {
        Self {
            id: None,
            skills: vec![spec],
        }
    }

    /// Create with container ID (for reuse)
    pub fn with_id(id: impl Into<String>) -> Self {
        Self {
            id: Some(id.into()),
            skills: Vec::with_capacity(8), // Pre-allocate for max capacity
        }
    }

    /// Add a skill to the container
    ///
    /// # Errors
    /// Returns error if adding skill would exceed maximum of 8 skills
    pub fn add_skill(&mut self, spec: SkillSpec) -> anyhow::Result<()> {
        if self.skills.len() >= 8 {
            anyhow::bail!(
                "Container already has maximum skills (8), cannot add '{}'",
                spec.skill_id
            );
        }
        self.skills.push(spec);
        Ok(())
    }

    /// Add multiple skills
    ///
    /// # Errors
    /// Returns error if total would exceed 8 skills
    pub fn add_skills(&mut self, mut specs: Vec<SkillSpec>) -> anyhow::Result<()> {
        let current_len = self.skills.len();
        let new_len = current_len + specs.len();
        if new_len > 8 {
            anyhow::bail!(
                "Adding {} skills would exceed maximum (8). Current: {}, requested: {}",
                specs.len(),
                current_len,
                specs.len()
            );
        }
        // Reserve capacity to avoid reallocations
        if new_len > self.skills.capacity() {
            self.skills.reserve(new_len - current_len);
        }
        self.skills.append(&mut specs);
        Ok(())
    }

    /// Add Anthropic skill
    pub fn add_anthropic(&mut self, skill_id: impl Into<String>) -> anyhow::Result<()> {
        self.add_skill(SkillSpec::anthropic(skill_id))
    }

    /// Add custom skill
    pub fn add_custom(&mut self, skill_id: impl Into<String>) -> anyhow::Result<()> {
        self.add_skill(SkillSpec::custom(skill_id))
    }

    /// Get number of skills in container
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Check if container is empty
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Check if container has a specific skill
    pub fn has_skill(&self, skill_id: &str) -> bool {
        self.skills.iter().any(|s| s.skill_id == skill_id)
    }

    /// Get skill by ID
    pub fn get_skill(&self, skill_id: &str) -> Option<&SkillSpec> {
        self.skills.iter().find(|s| s.skill_id == skill_id)
    }

    /// Validate container
    ///
    /// Checks:
    /// - No more than 8 skills
    /// - No duplicate skill IDs
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.skills.len() > 8 {
            anyhow::bail!("Container has {} skills, maximum is 8", self.skills.len());
        }

        let mut seen_ids = HashSet::new();
        for spec in &self.skills {
            if !seen_ids.insert(&spec.skill_id) {
                anyhow::bail!("Duplicate skill ID in container: '{}'", spec.skill_id);
            }
        }

        Ok(())
    }

    /// Set container ID for reuse
    pub fn set_id(&mut self, id: impl Into<String>) {
        self.id = Some(id.into());
    }

    /// Clear container ID
    pub fn clear_id(&mut self) {
        self.id = None;
    }

    /// Get all skill IDs in container
    pub fn skill_ids(&self) -> Vec<&str> {
        self.skills.iter().map(|s| s.skill_id.as_str()).collect()
    }

    /// Get all skills of a specific type
    pub fn skills_by_type(&self, skill_type: SkillType) -> Vec<&SkillSpec> {
        self.skills
            .iter()
            .filter(|s| s.skill_type == skill_type)
            .collect()
    }

    /// Count anthropic skills
    pub fn anthropic_count(&self) -> usize {
        self.skills_by_type(SkillType::Anthropic).len()
    }

    /// Count custom skills
    pub fn custom_count(&self) -> usize {
        self.skills_by_type(SkillType::Custom).len()
    }
}

impl Default for SkillContainer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_spec_new() {
        let spec = SkillSpec::new(SkillType::Custom, "my-skill");
        assert_eq!(spec.skill_id, "my-skill");
        assert_eq!(spec.skill_type, SkillType::Custom);
        assert!(spec.version.is_latest());
    }

    #[test]
    fn test_skill_spec_anthropic() {
        let spec = SkillSpec::anthropic("xlsx");
        assert_eq!(spec.skill_id, "xlsx");
        assert_eq!(spec.skill_type, SkillType::Anthropic);
    }

    #[test]
    fn test_skill_spec_with_version() {
        let spec = SkillSpec::custom("my-skill")
            .with_version(SkillVersion::Specific("1759178010641129".to_string()));
        assert_eq!(spec.version.as_str(), "1759178010641129");
        assert!(!spec.version.is_latest());
    }

    #[test]
    fn test_container_creation() {
        let container = SkillContainer::new();
        assert!(container.is_empty());
        assert!(container.id.is_none());
    }

    #[test]
    fn test_container_single_skill() {
        let spec = SkillSpec::custom("test-skill");
        let container = SkillContainer::single(spec.clone());
        assert_eq!(container.len(), 1);
        assert!(container.has_skill("test-skill"));
        assert_eq!(container.get_skill("test-skill"), Some(&spec));
    }

    #[test]
    fn test_container_add_skill() {
        let mut container = SkillContainer::new();
        let spec = SkillSpec::custom("skill1");
        assert!(container.add_skill(spec).is_ok());
        assert_eq!(container.len(), 1);
    }

    #[test]
    fn test_container_max_skills() {
        let mut container = SkillContainer::new();
        for i in 0..8 {
            let spec = SkillSpec::custom(format!("skill{}", i));
            assert!(container.add_skill(spec).is_ok());
        }
        assert_eq!(container.len(), 8);

        // Try to add 9th skill
        let spec = SkillSpec::custom("skill9");
        assert!(container.add_skill(spec).is_err());
    }

    #[test]
    fn test_container_add_skills_batch() {
        let mut container = SkillContainer::new();
        let specs = vec![
            SkillSpec::custom("skill1"),
            SkillSpec::custom("skill2"),
            SkillSpec::custom("skill3"),
        ];
        assert!(container.add_skills(specs).is_ok());
        assert_eq!(container.len(), 3);
    }

    #[test]
    fn test_container_add_skills_batch_overflow() {
        let mut container = SkillContainer::new();
        for i in 0..7 {
            let spec = SkillSpec::custom(format!("skill{}", i));
            container.add_skill(spec).ok();
        }
        assert_eq!(container.len(), 7);

        let specs = vec![SkillSpec::custom("skill7"), SkillSpec::custom("skill8")];
        assert!(container.add_skills(specs).is_err());
    }

    #[test]
    fn test_container_duplicate_skill_ids() {
        let mut container = SkillContainer::new();
        assert!(container.add_skill(SkillSpec::custom("dup")).is_ok());
        assert!(container.add_skill(SkillSpec::custom("dup")).is_ok());
        assert!(container.validate().is_err());
    }

    #[test]
    fn test_container_with_id() {
        let container = SkillContainer::with_id("container-123");
        assert_eq!(container.id, Some("container-123".to_string()));
    }

    #[test]
    fn test_container_set_id() {
        let mut container = SkillContainer::new();
        container.set_id("new-id");
        assert_eq!(container.id, Some("new-id".to_string()));
    }

    #[test]
    fn test_container_skills_by_type() {
        let mut container = SkillContainer::new();
        container.add_anthropic("xlsx").ok();
        container.add_anthropic("pptx").ok();
        container.add_custom("my-skill").ok();

        let anthropic = container.skills_by_type(SkillType::Anthropic);
        assert_eq!(anthropic.len(), 2);

        let custom = container.skills_by_type(SkillType::Custom);
        assert_eq!(custom.len(), 1);

        assert_eq!(container.anthropic_count(), 2);
        assert_eq!(container.custom_count(), 1);
    }

    #[test]
    fn test_container_skill_ids() {
        let mut container = SkillContainer::new();
        container.add_skill(SkillSpec::custom("skill1")).ok();
        container.add_skill(SkillSpec::custom("skill2")).ok();
        container.add_skill(SkillSpec::custom("skill3")).ok();

        let ids = container.skill_ids();
        assert_eq!(ids, vec!["skill1", "skill2", "skill3"]);
    }

    #[test]
    fn test_container_validation() {
        let mut container = SkillContainer::new();
        for i in 0..8 {
            container
                .add_skill(SkillSpec::custom(format!("skill{}", i)))
                .ok();
        }
        assert!(container.validate().is_ok());
    }

    #[test]
    fn test_skill_spec_roundtrip() {
        // Test serialization/deserialization roundtrip
        let spec = SkillSpec {
            skill_type: SkillType::Custom,
            skill_id: "my-skill".to_string(),
            version: SkillVersion::Specific("1759178010641129".to_string()),
        };

        let json = serde_json::to_string(&spec).unwrap();
        let deserialized: SkillSpec = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.skill_id, "my-skill");
        assert_eq!(deserialized.skill_type, SkillType::Custom);
        assert_eq!(
            deserialized.version,
            SkillVersion::Specific("1759178010641129".to_string())
        );
    }

    #[test]
    fn test_container_serialization() {
        let mut container = SkillContainer::new();
        container.add_anthropic("xlsx").ok();
        container.add_custom("my-skill").ok();

        let json = serde_json::to_string(&container).unwrap();
        let deserialized: SkillContainer = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.len(), 2);
        assert!(deserialized.has_skill("xlsx"));
        assert!(deserialized.has_skill("my-skill"));
    }
}
