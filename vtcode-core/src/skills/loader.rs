//! Enhanced Skill Discovery and Loading
//!
//! Integrates traditional VTCode skills with CLI tool skills and progressive
//! context management for efficient memory usage and comprehensive skill discovery.

use crate::skills::cli_bridge::{CliToolBridge, CliToolConfig};
use crate::skills::context_manager::{ContextManager, ContextConfig, ContextLevel};
use crate::skills::discovery::{DiscoveryConfig, SkillDiscovery};
use crate::skills::locations::{SkillLocations, DiscoveredSkill};
use crate::skills::manifest::{parse_skill_file};
use crate::skills::types::{Skill, SkillContext};
use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Enhanced skill search paths including CLI tools
const ENHANCED_SKILL_SEARCH_PATHS: &[&str] = &[
    ".claude/skills",     // Project-local skills (traditional)
    "./skills",           // Workspace skills (traditional)
    "~/.vtcode/skills",   // User global skills (traditional)
    "./tools",            // Project-local CLI tools
    "./vendor/tools",     // Vendor CLI tools
    "~/.vtcode/tools",    // User global CLI tools
];

/// Enhanced skill loader with CLI tool integration and context management
pub struct EnhancedSkillLoader {
    /// Skill locations manager
    skill_locations: SkillLocations,
    
    /// Context manager for progressive loading
    context_manager: ContextManager,
    
    /// Discovery engine for traditional skills and CLI tools
    discovery: SkillDiscovery,
    
    /// Workspace root for relative path resolution
    workspace_root: PathBuf,
    
    /// Cache of discovered skills
    discovered_skills: Option<Vec<DiscoveredSkill>>,
}

impl EnhancedSkillLoader {
    /// Create a new enhanced skill loader with default locations
    pub fn new(workspace_root: PathBuf) -> Self {
        Self::with_locations(SkillLocations::new(), workspace_root)
    }
    
    /// Create with custom skill locations
    pub fn with_locations(skill_locations: SkillLocations, workspace_root: PathBuf) -> Self {
        // Create discovery config from skill locations
        let mut discovery_config = DiscoveryConfig::default();
        
        // Extract paths from locations for backward compatibility
        for location_type in skill_locations.get_location_types() {
            if let Some(loc) = skill_locations.get_location(location_type) {
                match location_type {
                    crate::skills::locations::SkillLocationType::VtcodeUser |
                    crate::skills::locations::SkillLocationType::VtcodeProject |
                    crate::skills::locations::SkillLocationType::ClaudeUser |
                    crate::skills::locations::SkillLocationType::ClaudeProject => {
                        discovery_config.skill_paths.push(loc.base_path.clone());
                    }
                    _ => {
                        // For other locations, add to skill paths for now
                        discovery_config.skill_paths.push(loc.base_path.clone());
                    }
                }
            }
        }
        
        Self {
            skill_locations,
            context_manager: ContextManager::new(),
            discovery: SkillDiscovery::with_config(discovery_config),
            workspace_root,
            discovered_skills: None,
        }
    }
    
    /// Create loader with custom context configuration
    pub fn with_context_config(workspace_root: PathBuf, context_config: ContextConfig) -> Self {
        let mut loader = Self::new(workspace_root);
        loader.context_manager = ContextManager::with_config(context_config);
        loader
    }
    
    /// Add custom search path (backward compatibility)
    pub fn add_search_path(&mut self, path: PathBuf, path_type: SearchPathType) {
        // For backward compatibility, add to discovery config
        let mut discovery_config = DiscoveryConfig::default();
        
        match path_type {
            SearchPathType::Traditional => {
                discovery_config.skill_paths.push(path.clone());
            }
            SearchPathType::Tool => {
                discovery_config.tool_paths.push(path.clone());
            }
            SearchPathType::Both => {
                discovery_config.skill_paths.push(path.clone());
                discovery_config.tool_paths.push(path);
            }
        }
        
        self.discovery = SkillDiscovery::with_config(discovery_config);
    }
    
    /// Discover all available skills using the new locations system
    pub async fn discover_all_skills(&mut self) -> Result<EnhancedDiscoveryResult> {
        info!("Discovering all skills using locations system");
        
        let start_time = std::time::Instant::now();
        
        // Use the new locations-based discovery
        let discovered_skills = self.skill_locations.discover_skills()?;
        
        // Register all discovered skills in context manager
        for discovered in &discovered_skills {
            self.context_manager.register_skill_metadata(discovered.skill_context.manifest().clone())?;
        }
        
        // Also run traditional discovery for CLI tools and backward compatibility
        let discovery_result = self.discovery.discover_all(&self.workspace_root).await?;
        
        // Register CLI tools
        for tool_config in &discovery_result.tools {
            let skill_context = crate::skills::discovery::tool_config_to_skill_context(tool_config)?;
            self.context_manager.register_skill_metadata(skill_context.manifest().clone())?;
        }
        
        let discovery_time = start_time.elapsed();
        let traditional_count = discovered_skills.len();
        let cli_count = discovery_result.tools.len();
        
        info!(
            "Discovery complete: {} skills from locations, {} CLI tools from traditional discovery in {:?}",
            traditional_count,
            cli_count,
            discovery_time
        );
        
        // Cache the discovered skills
        self.discovered_skills = Some(discovered_skills.clone());
        
        Ok(EnhancedDiscoveryResult {
            traditional_skills: discovered_skills.into_iter().map(|d| d.skill_context).collect(),
            cli_tools: discovery_result.tools,
            stats: EnhancedDiscoveryStats {
                discovery_time_ms: discovery_time.as_millis() as u64,
                traditional_skills_found: traditional_count,
                cli_tools_found: cli_count,
                total_skills_found: traditional_count + cli_count,
                context_token_usage: self.context_manager.get_token_usage(),
            },
        })
    }
    
    /// Get skill by name (with automatic loading)
    pub async fn get_skill(&mut self, name: &str) -> Result<EnhancedSkill> {
        // Try to get from context manager first
        if let Some(context_entry) = self.context_manager.get_skill_context(name) {
            match context_entry.level {
                ContextLevel::Metadata => {
                    // Need to load full skill
                    self.load_full_skill(name).await
                }
                ContextLevel::Instructions => {
                    // Already has instructions, create skill from context
                    self.create_skill_from_context(&context_entry)
                }
                ContextLevel::Full => {
                    // Already fully loaded
                    if let Some(skill) = &context_entry.skill {
                        Ok(EnhancedSkill::Traditional(skill.clone()))
                    } else {
                        Err(anyhow!("Skill '{}' context indicates full loading but no skill object found", name))
                    }
                }
            }
        } else {
            Err(anyhow!("Skill '{}' not found in context manager", name))
        }
    }
    
    /// Load full skill details using the locations system
    async fn load_full_skill(&mut self, name: &str) -> Result<EnhancedSkill> {
        info!("Loading full skill: {}", name);
        
        // First, ensure we have discovered skills
        if self.discovered_skills.is_none() {
            // Discover skills if not already done
            match self.discover_all_skills().await {
                Ok(_) => {
                    // Continue with the discovered skills
                }
                Err(e) => {
                    warn!("Failed to discover skills during loading: {}", e);
                    return Err(anyhow!("Skill '{}' not found - discovery failed", name));
                }
            }
        }
        
        // Find the skill in the discovered cache
        let skill_path = if let Some(discovered_skills) = &self.discovered_skills {
            discovered_skills
                .iter()
                .find(|d| d.skill_context.manifest().name == name)
                .map(|d| d.skill_path.clone())
        } else {
            None
        };
        
        if let Some(skill_path) = skill_path {
            // Check if it's a traditional skill by looking for SKILL.md
            if skill_path.join("SKILL.md").exists() {
                return self.load_traditional_skill(&skill_path).await;
            } else {
                // Assume it's a CLI tool
                return self.load_cli_tool_skill(&skill_path).await;
            }
        }
        
        Err(anyhow!("Skill '{}' not found in any location", name))
    }
    
    /// Load traditional skill from directory
    async fn load_traditional_skill(&mut self, skill_path: &Path) -> Result<EnhancedSkill> {
        let (manifest, instructions) = parse_skill_file(skill_path)?;
        
        let mut skill = Skill::new(manifest, skill_path.to_path_buf(), instructions)?;
        
        // Load resources
        self.load_skill_resources(&mut skill)?;
        
        // Register in context manager
        self.context_manager.load_full_skill(skill.clone()).await?;
        
        Ok(EnhancedSkill::Traditional(skill))
    }
    
    /// Load CLI tool as skill
    async fn load_cli_tool_skill(&mut self, tool_dir: &Path) -> Result<EnhancedSkill> {
        let bridge = CliToolBridge::from_directory(tool_dir)?;
        let skill = bridge.to_skill()?;
        
        // Register in context manager
        self.context_manager.load_full_skill(skill.clone()).await?;
        
        Ok(EnhancedSkill::CliTool(bridge))
    }
    
    /// Create skill from context entry
    fn create_skill_from_context(&self, context_entry: &crate::skills::context_manager::SkillContextEntry) -> Result<EnhancedSkill> {
        if let Some(skill) = &context_entry.skill {
            return Ok(EnhancedSkill::Traditional(skill.clone()));
        }
        
        // Try to load as CLI tool if not a traditional skill
        // Use a simple heuristic: if the skill path exists and contains tool.json, it's a CLI tool
        let skill_name = &context_entry.name;
        
        // Try common locations for CLI tools
        let possible_paths = vec![
            self.workspace_root.join("tools").join(skill_name),
            self.workspace_root.join("vendor/tools").join(skill_name),
            PathBuf::from("~/.vtcode/tools").join(skill_name),
        ];
        
        for tool_dir in &possible_paths {
            let expanded_path = self.expand_path(tool_dir);
            if expanded_path.exists() && expanded_path.join("tool.json").exists() {
                let bridge = CliToolBridge::from_directory(&expanded_path)?;
                return Ok(EnhancedSkill::CliTool(bridge));
            }
        }
        
        Err(anyhow!("Cannot create skill from context for '{}'", context_entry.name))
    }
    
    /// Expand path with home directory and workspace root
    fn expand_path(&self, path: &Path) -> PathBuf {
        if path.starts_with("~") {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home).join(path.strip_prefix("~").unwrap_or(path));
            }
        }
        
        if path.is_relative() {
            self.workspace_root.join(path)
        } else {
            path.to_path_buf()
        }
    }
    
    /// Load skill resources (Level 3)
    fn load_skill_resources(&self, skill: &mut Skill) -> Result<()> {
        let mut resource_dir = skill.path.clone();
        resource_dir.push("scripts");
        
        if resource_dir.exists() {
            for entry in std::fs::read_dir(&resource_dir)? {
                let entry = entry?;
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
        
        Ok(())
    }
    
    /// Get all available skill names
    pub fn get_available_skills(&self) -> Vec<String> {
        self.context_manager.get_active_skills()
    }
    
    /// Get context manager for advanced usage
    pub fn context_manager(&self) -> &ContextManager {
        &self.context_manager
    }
    
    /// Get mutable context manager
    pub fn context_manager_mut(&mut self) -> &mut ContextManager {
        &mut self.context_manager
    }
    
    /// Get discovery statistics
    pub fn get_discovery_stats(&self) -> crate::skills::discovery::DiscoveryStats {
        self.discovery.get_stats()
    }
    
    /// Get context statistics
    pub fn get_context_stats(&self) -> crate::skills::context_manager::ContextStats {
        self.context_manager.get_stats()
    }
    
    /// Clear context cache
    pub fn clear_context_cache(&mut self) {
        self.context_manager.clear_loaded_skills();
    }
    
    /// Refresh discovery (clear caches and re-scan)
    pub async fn refresh_discovery(&mut self) -> Result<EnhancedDiscoveryResult> {
        self.discovery.clear_cache();
        self.context_manager.clear_loaded_skills();
        self.discover_all_skills().await
    }
}

/// Enhanced skill discovery result
#[derive(Debug, Clone)]
pub struct EnhancedDiscoveryResult {
    /// Traditional VTCode skills
    pub traditional_skills: Vec<SkillContext>,
    
    /// CLI tool configurations
    pub cli_tools: Vec<CliToolConfig>,
    
    /// Discovery statistics
    pub stats: EnhancedDiscoveryStats,
}

/// Enhanced discovery statistics
#[derive(Debug, Clone, Default)]
pub struct EnhancedDiscoveryStats {
    /// Discovery time in milliseconds
    pub discovery_time_ms: u64,
    
    /// Number of traditional skills found
    pub traditional_skills_found: usize,
    
    /// Number of CLI tools found
    pub cli_tools_found: usize,
    
    /// Total number of skills found
    pub total_skills_found: usize,
    
    /// Context token usage after discovery
    pub context_token_usage: usize,
}

/// Enhanced skill types
#[derive(Debug, Clone)]
pub enum EnhancedSkill {
    /// Traditional VTCode skill
    Traditional(Skill),
    
    /// CLI tool skill
    CliTool(CliToolBridge),
}

impl EnhancedSkill {
    /// Get skill name
    pub fn name(&self) -> &str {
        match self {
            EnhancedSkill::Traditional(skill) => skill.name(),
            EnhancedSkill::CliTool(bridge) => bridge.config.name.as_str(),
        }
    }
    
    /// Get skill description
    pub fn description(&self) -> &str {
        match self {
            EnhancedSkill::Traditional(skill) => skill.description(),
            EnhancedSkill::CliTool(bridge) => bridge.config.description.as_str(),
        }
    }
    
    /// Check if this is a CLI tool skill
    pub fn is_cli_tool(&self) -> bool {
        matches!(self, EnhancedSkill::CliTool(_))
    }
    
    /// Get as traditional skill (if applicable)
    pub fn as_traditional(&self) -> Option<&Skill> {
        match self {
            EnhancedSkill::Traditional(skill) => Some(skill),
            EnhancedSkill::CliTool(_) => None,
        }
    }
    
    /// Get as CLI tool (if applicable)
    pub fn as_cli_tool(&self) -> Option<&CliToolBridge> {
        match self {
            EnhancedSkill::Traditional(_) => None,
            EnhancedSkill::CliTool(bridge) => Some(bridge),
        }
    }
}

/// Search path types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchPathType {
    /// Traditional skills only
    Traditional,
    /// CLI tools only
    Tool,
    /// Both types
    Both,
}

/// Legacy skill loader for backward compatibility
pub struct LegacySkillLoader {
    enhanced: EnhancedSkillLoader,
}

impl LegacySkillLoader {
    /// Create legacy loader from enhanced loader
    pub fn from_enhanced(enhanced: EnhancedSkillLoader) -> Self {
        Self { enhanced }
    }
    
    /// Discover skills (traditional only for backward compatibility)
    pub fn discover_skills(&self) -> Result<Vec<SkillContext>> {
        // This would need to be implemented with async runtime
        // For now, return an error indicating the need to upgrade
        Err(anyhow!("Legacy skill discovery requires async runtime. Use EnhancedSkillLoader::discover_all_skills() instead."))
    }
    
    /// Load skill by name
    pub async fn load_skill(&mut self, name: &str) -> Result<Skill> {
        match self.enhanced.get_skill(name).await? {
            EnhancedSkill::Traditional(skill) => Ok(skill),
            EnhancedSkill::CliTool(_) => Err(anyhow!("CLI tool skills are not supported in legacy loader")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_enhanced_loader_creation() {
        let temp_dir = TempDir::new().unwrap();
        let loader = EnhancedSkillLoader::new(temp_dir.path().to_path_buf());
        
        // Test that loader is created successfully
        assert_eq!(loader.get_available_skills().len(), 0);
        assert!(loader.context_manager().get_stats().total_skills_loaded == 0);
    }
    
    #[tokio::test]
    async fn test_discovery_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let mut loader = EnhancedSkillLoader::new(temp_dir.path().to_path_buf());
        
        let result = loader.discover_all_skills().await.unwrap();
        assert_eq!(result.traditional_skills.len(), 0);
        assert_eq!(result.cli_tools.len(), 0);
        assert_eq!(result.stats.total_skills_found, 0);
    }
    
    #[test]
    fn test_search_path_type() {
        assert_eq!(SearchPathType::Traditional, SearchPathType::Traditional);
        assert_eq!(SearchPathType::Tool, SearchPathType::Tool);
        assert_eq!(SearchPathType::Both, SearchPathType::Both);
    }
    
    #[test]
    fn test_enhanced_skill_types() {
        // Test would need actual Skill and CliToolBridge instances
        // This is a placeholder for the test structure
    }
}