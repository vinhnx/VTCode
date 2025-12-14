//! Dynamic Skill Discovery System
//!
//! Implements filesystem-based skill discovery with support for:
//! - Traditional VTCode skills (SKILL.md files)
//! - CLI tool skills (executable + README.md)
//! - Auto-discovery of tools in standard locations
//! - Progressive metadata loading

use crate::skills::cli_bridge::{CliToolBridge, CliToolConfig, discover_cli_tools};
use crate::skills::types::{SkillContext, SkillManifest};
use crate::skills::manifest::parse_skill_file;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Enhanced skill discovery configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Search paths for traditional skills
    pub skill_paths: Vec<PathBuf>,
    
    /// Search paths for CLI tools
    pub tool_paths: Vec<PathBuf>,
    
    /// Auto-discover system tools
    pub auto_discover_system_tools: bool,
    
    /// Maximum depth for recursive directory scanning
    pub max_depth: usize,
    
    /// File patterns to consider as skills
    pub skill_patterns: Vec<String>,
    
    /// Tool file patterns
    pub tool_patterns: Vec<String>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            skill_paths: vec![
                PathBuf::from(".claude/skills"),
                PathBuf::from("./skills"),
                PathBuf::from("~/.vtcode/skills"),
            ],
            tool_paths: vec![
                PathBuf::from("./tools"),
                PathBuf::from("./vendor/tools"),
                PathBuf::from("~/.vtcode/tools"),
            ],
            auto_discover_system_tools: true,
            max_depth: 3,
            skill_patterns: vec!["SKILL.md".to_string()],
            tool_patterns: vec!["*.exe".to_string(), "*.sh".to_string(), "*.py".to_string()],
        }
    }
}

/// Discovery result containing both traditional skills and CLI tools
#[derive(Debug, Clone)]
pub struct DiscoveryResult {
    /// Traditional VTCode skills
    pub skills: Vec<SkillContext>,
    
    /// CLI tool configurations
    pub tools: Vec<CliToolConfig>,
    
    /// Discovery statistics
    pub stats: DiscoveryStats,
}

/// Discovery statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryStats {
    pub directories_scanned: usize,
    pub files_checked: usize,
    pub skills_found: usize,
    pub tools_found: usize,
    pub errors_encountered: usize,
    pub discovery_time_ms: u64,
}

/// Dynamic skill discovery engine
pub struct SkillDiscovery {
    config: DiscoveryConfig,
    cache: HashMap<PathBuf, DiscoveryCacheEntry>,
}

#[derive(Debug, Clone)]
struct DiscoveryCacheEntry {
    #[allow(dead_code)]
    timestamp: std::time::SystemTime,
    skills: Vec<SkillContext>,
    tools: Vec<CliToolConfig>,
}

impl SkillDiscovery {
    /// Create new discovery engine with default configuration
    pub fn new() -> Self {
        Self::with_config(DiscoveryConfig::default())
    }
    
    /// Create new discovery engine with custom configuration
    pub fn with_config(config: DiscoveryConfig) -> Self {
        Self {
            config,
            cache: HashMap::new(),
        }
    }
    
    /// Discover all available skills and tools
    pub async fn discover_all(&mut self, workspace_root: &Path) -> Result<DiscoveryResult> {
        let start_time = std::time::Instant::now();
        let mut stats = DiscoveryStats::default();
        
        info!("Starting skill discovery in: {}", workspace_root.display());
        
        // Discover traditional skills
        let skills = self.discover_traditional_skills(workspace_root, &mut stats).await?;
        
        // Discover CLI tools
        let tools = self.discover_cli_tools(workspace_root, &mut stats).await?;
        
        // Auto-discover system tools if enabled
        if self.config.auto_discover_system_tools {
            let system_tools = self.discover_system_tools(&mut stats).await?;
            let mut all_tools = tools;
            all_tools.extend(system_tools);
            
            stats.discovery_time_ms = start_time.elapsed().as_millis() as u64;
            
            Ok(DiscoveryResult {
                skills,
                tools: all_tools,
                stats,
            })
        } else {
            stats.discovery_time_ms = start_time.elapsed().as_millis() as u64;
            
            Ok(DiscoveryResult {
                skills,
                tools,
                stats,
            })
        }
    }
    
    /// Discover traditional VTCode skills
    async fn discover_traditional_skills(
        &mut self,
        workspace_root: &Path,
        stats: &mut DiscoveryStats,
    ) -> Result<Vec<SkillContext>> {
        let mut skills = vec![];
        
        for skill_path in &self.config.skill_paths {
            let full_path = self.expand_path(skill_path, workspace_root);
            
            if !full_path.exists() {
                debug!("Skill path does not exist: {}", full_path.display());
                continue;
            }
            
            stats.directories_scanned += 1;
            
            // Scan for skill directories
            match self.scan_for_skills(&full_path, stats).await {
                Ok(found_skills) => {
                    info!("Found {} skills in {}", found_skills.len(), full_path.display());
                    skills.extend(found_skills);
                }
                Err(e) => {
                    warn!("Failed to scan {}: {}", full_path.display(), e);
                    stats.errors_encountered += 1;
                }
            }
        }
        
        Ok(skills)
    }
    
    /// Scan directory for traditional skills
    async fn scan_for_skills(
        &self,
        dir: &Path,
        stats: &mut DiscoveryStats,
    ) -> Result<Vec<SkillContext>> {
        let mut skills = vec![];
        
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_dir() {
                stats.directories_scanned += 1;
                
                // Check for SKILL.md file
                let skill_file = path.join("SKILL.md");
                if skill_file.exists() {
                    stats.files_checked += 1;
                    
                    match parse_skill_file(&path) {
                        Ok((manifest, _instructions)) => {
                            skills.push(SkillContext::MetadataOnly(manifest));
                            stats.skills_found += 1;
                            info!("Discovered skill: {} from {}", 
                                skills.last().unwrap().manifest().name, 
                                path.display()
                            );
                        }
                        Err(e) => {
                            warn!("Failed to parse skill from {}: {}", path.display(), e);
                            stats.errors_encountered += 1;
                        }
                    }
                }
            }
        }
        
        Ok(skills)
    }
    
    /// Discover CLI tools in workspace
    async fn discover_cli_tools(
        &mut self,
        workspace_root: &Path,
        stats: &mut DiscoveryStats,
    ) -> Result<Vec<CliToolConfig>> {
        let mut tools = vec![];
        
        for tool_path in &self.config.tool_paths {
            let full_path = self.expand_path(tool_path, workspace_root);
            
            if !full_path.exists() {
                debug!("Tool path does not exist: {}", full_path.display());
                continue;
            }
            
            stats.directories_scanned += 1;
            
            match self.scan_for_tools(&full_path, stats).await {
                Ok(found_tools) => {
                    info!("Found {} tools in {}", found_tools.len(), full_path.display());
                    tools.extend(found_tools);
                }
                Err(e) => {
                    warn!("Failed to scan {}: {}", full_path.display(), e);
                    stats.errors_encountered += 1;
                }
            }
        }
        
        Ok(tools)
    }
    
    /// Scan directory for CLI tools
    async fn scan_for_tools(
        &self,
        dir: &Path,
        stats: &mut DiscoveryStats,
    ) -> Result<Vec<CliToolConfig>> {
        let mut tools = vec![];
        
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_file() {
                stats.files_checked += 1;
                
                // Check if it's an executable
                if self.is_executable(&entry)? {
                    // Look for accompanying documentation
                    let readme_path = self.find_tool_readme(&path);
                    let schema_path = self.find_tool_schema(&path);
                    
                    let tool_name = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    
                    let config = CliToolConfig {
                        name: tool_name.clone(),
                        description: format!("CLI tool: {}", tool_name),
                        executable_path: path.clone(),
                        readme_path,
                        schema_path,
                        timeout_seconds: Some(30),
                        supports_json: false,
                        environment: None,
                        working_dir: Some(dir.to_path_buf()),
                    };
                    
                    tools.push(config);
                    stats.tools_found += 1;
                    debug!("Discovered CLI tool: {} from {}", tool_name, path.display());
                }
            }
        }
        
        Ok(tools)
    }
    
    /// Discover system-wide CLI tools
    async fn discover_system_tools(&self, stats: &mut DiscoveryStats) -> Result<Vec<CliToolConfig>> {
        info!("Auto-discovering system CLI tools");
        
        match discover_cli_tools() {
            Ok(tools) => {
                stats.tools_found += tools.len();
                Ok(tools)
            }
            Err(e) => {
                warn!("Failed to auto-discover system tools: {}", e);
                stats.errors_encountered += 1;
                Ok(vec![])
            }
        }
    }
    
    /// Check if file is executable
    fn is_executable(&self, entry: &std::fs::DirEntry) -> Result<bool> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = entry.metadata()?;
            let permissions = metadata.permissions();
            Ok(permissions.mode() & 0o111 != 0)
        }
        
        #[cfg(windows)]
        {
            if let Some(ext) = entry.path().extension() {
                Ok(ext == "exe" || ext == "bat" || ext == "cmd")
            } else {
                Ok(false)
            }
        }
    }
    
    /// Find README file for tool
    fn find_tool_readme(&self, tool_path: &Path) -> Option<PathBuf> {
        let tool_name = tool_path.file_stem()?;
        let readme_name = format!("{}.md", tool_name.to_str()?);
        let readme_path = tool_path.with_file_name(&readme_name);
        
        if readme_path.exists() {
            Some(readme_path)
        } else {
            // Try generic README.md
            let generic_readme = tool_path.parent()?.join("README.md");
            if generic_readme.exists() {
                Some(generic_readme)
            } else {
                None
            }
        }
    }
    
    /// Find JSON schema file for tool
    fn find_tool_schema(&self, tool_path: &Path) -> Option<PathBuf> {
        let tool_name = tool_path.file_stem()?;
        let schema_name = format!("{}.json", tool_name.to_str()?);
        let schema_path = tool_path.with_file_name(&schema_name);
        
        if schema_path.exists() {
            Some(schema_path)
        } else {
            // Try tool.json
            let tool_json = tool_path.parent()?.join("tool.json");
            if tool_json.exists() {
                Some(tool_json)
            } else {
                None
            }
        }
    }
    
    /// Expand path with workspace root and home directory
    fn expand_path(&self, path: &Path, workspace_root: &Path) -> PathBuf {
        if path.starts_with("~") {
            // Expand home directory
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home).join(path.strip_prefix("~").unwrap());
            }
        }
        
        if path.is_relative() {
            // Make relative to workspace root
            workspace_root.join(path)
        } else {
            path.to_path_buf()
        }
    }
    
    /// Get cached discovery result for path
    #[allow(dead_code)]
    fn get_cached(&self, path: &Path) -> Option<&DiscoveryCacheEntry> {
        self.cache.get(path).and_then(|entry| {
            // Check if cache is still valid (5 minutes)
            let elapsed = entry.timestamp.elapsed().ok()?;
            if elapsed.as_secs() < 300 {
                Some(entry)
            } else {
                None
            }
        })
    }
    
    /// Cache discovery result
    #[allow(dead_code)]
    fn cache_result(&mut self, path: PathBuf, skills: Vec<SkillContext>, tools: Vec<CliToolConfig>) {
        self.cache.insert(path, DiscoveryCacheEntry {
            timestamp: std::time::SystemTime::now(),
            skills,
            tools,
        });
    }
    
    /// Clear discovery cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        info!("Discovery cache cleared");
    }
    
    /// Get discovery statistics
    pub fn get_stats(&self) -> DiscoveryStats {
        DiscoveryStats {
            directories_scanned: 0,
            files_checked: 0,
            skills_found: self.cache.values()
                .map(|entry| entry.skills.len())
                .sum(),
            tools_found: self.cache.values()
                .map(|entry| entry.tools.len())
                .sum(),
            errors_encountered: 0,
            discovery_time_ms: 0,
        }
    }
}

/// Convert CLI tool configuration to SkillContext
pub fn tool_config_to_skill_context(config: &CliToolConfig) -> Result<SkillContext> {
    let manifest = SkillManifest {
        name: config.name.clone(),
        description: config.description.clone(),
        version: Some("1.0.0".to_string()),
        author: Some("VTCode CLI Discovery".to_string()),
    };
    
    Ok(SkillContext::MetadataOnly(manifest))
}

/// Progressive skill loader that can load full skill details on demand
pub struct ProgressiveSkillLoader {
    discovery: SkillDiscovery,
    skill_cache: HashMap<String, crate::skills::types::Skill>,
    #[allow(dead_code)]
    tool_cache: HashMap<String, CliToolBridge>,
}

impl ProgressiveSkillLoader {
    pub fn new(config: DiscoveryConfig) -> Self {
        Self {
            discovery: SkillDiscovery::with_config(config),
            skill_cache: HashMap::new(),
            tool_cache: HashMap::new(),
        }
    }
    
    /// Get skill metadata (lightweight)
    pub async fn get_skill_metadata(&mut self, workspace_root: &Path, name: &str) -> Result<SkillContext> {
        let result = self.discovery.discover_all(workspace_root).await?;
        
        // Check traditional skills
        for skill in &result.skills {
            if skill.manifest().name == name {
                return Ok(skill.clone());
            }
        }
        
        // Check CLI tools
        for tool in &result.tools {
            if tool.name == name {
                return Ok(tool_config_to_skill_context(tool)?);
            }
        }
        
        Err(anyhow::anyhow!("Skill '{}' not found", name))
    }
    
    /// Load full skill with instructions and resources
    pub async fn load_full_skill(&mut self, workspace_root: &Path, name: &str) -> Result<crate::skills::types::Skill> {
        // Check cache first
        if let Some(skill) = self.skill_cache.get(name) {
            return Ok(skill.clone());
        }
        
        let result = self.discovery.discover_all(workspace_root).await?;
        
        // Try traditional skills first
        for skill_ctx in &result.skills {
            if skill_ctx.manifest().name == name {
                // Load full skill details
                // This would require path information - simplified for now
                let manifest = skill_ctx.manifest().clone();
                let skill = crate::skills::types::Skill::new(
                    manifest,
                    workspace_root.to_path_buf(),
                    "# Full instructions would be loaded here".to_string(),
                )?;
                
                self.skill_cache.insert(name.to_string(), skill.clone());
                return Ok(skill);
            }
        }
        
        // Try CLI tools
        for tool_config in &result.tools {
            if tool_config.name == name {
                let bridge = CliToolBridge::new(tool_config.clone())?;
                let skill = bridge.to_skill()?;
                
                self.skill_cache.insert(name.to_string(), skill.clone());
                return Ok(skill);
            }
        }
        
        Err(anyhow::anyhow!("Skill '{}' not found", name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_discovery_config_default() {
        let config = DiscoveryConfig::default();
        assert!(!config.skill_paths.is_empty());
        assert!(!config.tool_paths.is_empty());
        assert!(config.auto_discover_system_tools);
    }
    
    #[tokio::test]
    async fn test_discovery_engine_creation() {
        let discovery = SkillDiscovery::new();
        assert_eq!(discovery.cache.len(), 0);
    }
    
    #[tokio::test]
    async fn test_progressive_loader() {
        let temp_dir = TempDir::new().unwrap();
        let config = DiscoveryConfig::default();
        let mut loader = ProgressiveSkillLoader::new(config);
        
        // Should handle empty directory gracefully
        let result = loader.discovery.discover_all(temp_dir.path()).await;
        assert!(result.is_ok());
    }
}