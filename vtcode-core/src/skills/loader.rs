//! Skill discovery and loading from filesystem
//!
//! Loads skills from standard directories:
//! - ~/.vtcode/skills/ (user global skills)
//! - ./.claude/skills/ (project skills)
//! - ./skills/ (workspace skills)

use crate::skills::manifest::{parse_skill_file};
use crate::skills::types::{Skill, SkillContext};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;
use tracing::{debug, warn, info};

/// Standard skill search paths
const SKILL_SEARCH_PATHS: &[&str] = &[
    ".claude/skills",     // Project-local skills
    "./skills",           // Workspace skills
    "~/.vtcode/skills",   // User global skills
];

/// Skill loader and discovery
pub struct SkillLoader {
    search_paths: Vec<PathBuf>,
}

impl SkillLoader {
    /// Create a new skill loader with default search paths
    pub fn new(workspace_root: PathBuf) -> Self {
        let mut search_paths = vec![];

        // Project-local skills
        search_paths.push(workspace_root.join(".claude/skills"));
        search_paths.push(workspace_root.join("skills"));

        // User global skills
        if let Ok(home_dir) = std::env::var("HOME") {
            search_paths.push(PathBuf::from(home_dir).join(".vtcode/skills"));
        }

        SkillLoader { search_paths }
    }

    /// Add custom search path
    pub fn add_search_path(&mut self, path: PathBuf) {
        self.search_paths.push(path);
    }

    /// Discover all available skills (Level 1: metadata only)
    pub fn discover_skills(&self) -> anyhow::Result<Vec<SkillContext>> {
        let mut skills = vec![];

        for search_path in &self.search_paths {
            if !search_path.exists() {
                debug!("Skill search path does not exist: {}", search_path.display());
                continue;
            }

            if !search_path.is_dir() {
                warn!("Skill search path is not a directory: {}", search_path.display());
                continue;
            }

            // Each subdirectory is a skill
            match fs::read_dir(search_path) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            match self.load_skill_metadata(&path) {
                                Ok(manifest) => {
                                    skills.push(SkillContext::MetadataOnly(manifest));
                                }
                                Err(e) => {
                                    warn!(
                                        "Failed to load skill metadata from {}: {}",
                                        path.display(),
                                        e
                                    );
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to read skill directory {}: {}",
                        search_path.display(),
                        e
                    );
                }
            }
        }

        Ok(skills)
    }

    /// Load a skill by name (Level 2: with instructions)
    pub fn load_skill(&self, name: &str) -> anyhow::Result<Skill> {
        for search_path in &self.search_paths {
            let skill_path = search_path.join(name);

            if skill_path.exists() && skill_path.is_dir() {
                return self.load_skill_from_path(&skill_path);
            }
        }

        anyhow::bail!("Skill '{}' not found in search paths", name);
    }

    /// Load a skill from specific path
    pub fn load_skill_from_path(&self, skill_path: &Path) -> anyhow::Result<Skill> {
        let (manifest, instructions) = parse_skill_file(skill_path)?;

        let mut skill = Skill::new(manifest, skill_path.to_path_buf(), instructions)?;

        // Discover resources (Level 3)
        self.load_skill_resources(&mut skill)?;

        info!("Loaded skill: {} from {}", skill.name(), skill_path.display());

        Ok(skill)
    }

    /// Load skill metadata only (Level 1)
    fn load_skill_metadata(&self, skill_path: &Path) -> anyhow::Result<crate::skills::types::SkillManifest> {
        let (manifest, _) = parse_skill_file(skill_path)?;
        Ok(manifest)
    }

    /// Discover and load skill resources (Level 3)
    fn load_skill_resources(&self, skill: &mut Skill) -> anyhow::Result<()> {
        let mut resource_dir = skill.path.clone();
        resource_dir.push("scripts");

        if resource_dir.exists() {
            match fs::read_dir(&resource_dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() {
                            let rel_path = path.strip_prefix(&skill.path)
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default();

                            let resource_type = match path.extension()
                                .and_then(|e| e.to_str()) {
                                Some("py") | Some("sh") | Some("bash") => {
                                    crate::skills::types::ResourceType::Script
                                }
                                Some("md") => crate::skills::types::ResourceType::Markdown,
                                Some("json") | Some("yaml") | Some("yml") => {
                                    crate::skills::types::ResourceType::Reference
                                }
                                _ => crate::skills::types::ResourceType::Other(
                                    format!("{:?}", path.extension())
                                ),
                            };

                            skill.add_resource(
                                rel_path.clone(),
                                crate::skills::types::SkillResource {
                                    path: rel_path,
                                    resource_type,
                                    content: None,
                                },
                            );
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to read skill resources from {}: {}", resource_dir.display(), e);
                }
            }
        }

        Ok(())
    }
}

/// Skill cache (Level 1 only for minimal memory usage)
pub struct SkillCache {
    metadata_cache: HashMap<String, SkillContext>,
    loader: SkillLoader,
}

impl SkillCache {
    pub fn new(workspace_root: PathBuf) -> Self {
        SkillCache {
            metadata_cache: HashMap::new(),
            loader: SkillLoader::new(workspace_root),
        }
    }

    /// Get cached metadata or discover
    pub async fn get_metadata(&mut self, name: &str) -> anyhow::Result<SkillContext> {
        if let Some(ctx) = self.metadata_cache.get(name) {
            return Ok(ctx.clone());
        }

        let metadata = self.loader.discover_skills()?
            .into_iter()
            .find(|ctx| ctx.manifest().name == name)
            .ok_or_else(|| anyhow::anyhow!("Skill '{}' not found", name))?;

        self.metadata_cache.insert(name.to_string(), metadata.clone());
        Ok(metadata)
    }

    /// Load full skill (Level 2-3)
    pub async fn load_skill(&self, name: &str) -> anyhow::Result<Skill> {
        self.loader.load_skill(name)
    }

    /// Discover all available skills
    pub async fn discover_all(&mut self) -> anyhow::Result<Vec<SkillContext>> {
        self.loader.discover_skills()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_skill_loader_creation() {
        let temp_dir = TempDir::new().unwrap();
        let loader = SkillLoader::new(temp_dir.path().to_path_buf());
        assert!(!loader.search_paths.is_empty());
    }

    #[tokio::test]
    async fn test_skill_cache_creation() {
        let temp_dir = TempDir::new().unwrap();
        let cache = SkillCache::new(temp_dir.path().to_path_buf());
        assert_eq!(cache.metadata_cache.len(), 0);
    }
}
