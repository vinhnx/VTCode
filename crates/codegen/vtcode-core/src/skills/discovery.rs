//! Dynamic Skill Discovery System
//!
//! Implements filesystem-based skill discovery with support for:
//! - Traditional VT Code skills (SKILL.md files)
//! - CLI tool skills (executable + README.md)
//! - Auto-discovery of tools in standard locations
//! - Progressive metadata loading

use crate::skills::cli_bridge::{CliToolBridge, CliToolConfig, discover_cli_tools};
use crate::skills::manifest::parse_skill_file;
use crate::skills::types::{SkillContext, SkillManifest, SkillVariety};
use crate::tools::error_messages::skill_ops;
use anyhow::{Context, Result};
use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};
use vtcode_commons::VtCodePaths;

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
        let mut tool_paths = vec![PathBuf::from("./tools"), PathBuf::from("./vendor/tools")];
        if let Ok(paths) = VtCodePaths::resolve() {
            if let Ok(user_tool_path) = paths.data_path("tools") {
                tool_paths.push(user_tool_path);
            }
            tool_paths.push(paths.legacy_dir().join("tools"));
        }
        Self {
            skill_paths: Vec::new(),
            tool_paths,
            auto_discover_system_tools: false,
            max_depth: 3,
            skill_patterns: vec!["SKILL.md".to_string()],
            tool_patterns: vec!["*.exe".to_string(), "*.sh".to_string(), "*.py".to_string()],
        }
    }
}

/// Discovery result containing both traditional skills and CLI tools
#[derive(Debug, Clone)]
pub struct DiscoveryResult {
    /// Traditional VT Code skills
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
        Self { config, cache: HashMap::new() }
    }
}

impl Default for SkillDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillDiscovery {
    /// Discover all available skills and tools
    pub async fn discover_all(&mut self, workspace_root: &Path) -> Result<DiscoveryResult> {
        let start_time = std::time::Instant::now();
        info!("Starting skill discovery in: {}", workspace_root.display());

        let config = self.config.clone();
        let workspace_root = workspace_root.to_path_buf();
        let (skills, tools, mut stats) = tokio::task::spawn_blocking(move || {
            let mut worker = Self::with_config(config);
            let mut stats = DiscoveryStats::default();
            let skills = worker.discover_traditional_skills(&workspace_root, &mut stats)?;
            let mut tools = worker.discover_cli_tools(&workspace_root, &mut stats)?;
            if worker.config.auto_discover_system_tools {
                tools.extend(worker.discover_system_tools(&mut stats)?);
            }
            Ok::<_, anyhow::Error>((skills, tools, stats))
        })
        .await
        .context("skill discovery task panicked")??;

        stats.discovery_time_ms = start_time.elapsed().as_millis() as u64;
        Ok(DiscoveryResult { skills, tools, stats })
    }

    /// Discover traditional VT Code skills
    fn discover_traditional_skills(
        &mut self,
        workspace_root: &Path,
        stats: &mut DiscoveryStats,
    ) -> Result<Vec<SkillContext>> {
        let mut skills = vec![];
        let skill_paths = if self.config.skill_paths.is_empty() {
            default_skill_paths(workspace_root)
        } else {
            self.config.skill_paths.clone()
        };

        for skill_path in &skill_paths {
            let full_path = self.expand_path(skill_path, workspace_root);

            if !full_path.exists() {
                debug!("Skill path does not exist: {}", full_path.display());
                continue;
            }

            stats.directories_scanned += 1;

            // Scan for skill directories
            match self.scan_for_skills(&full_path, stats) {
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
    fn scan_for_skills(&self, dir: &Path, stats: &mut DiscoveryStats) -> Result<Vec<SkillContext>> {
        self.scan_for_skills_recursive(dir, stats, 0)
    }

    fn scan_for_skills_recursive(
        &self,
        dir: &Path,
        stats: &mut DiscoveryStats,
        depth: usize,
    ) -> Result<Vec<SkillContext>> {
        let mut skills = vec![];

        if depth > self.config.max_depth {
            return Ok(skills);
        }

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
                            skills.push(SkillContext::MetadataOnly(manifest, path.to_path_buf()));
                            stats.skills_found += 1;
                            let skill_name = skills
                                .last()
                                .map(|ctx| ctx.manifest().name.clone())
                                .unwrap_or_else(|| "<unknown>".to_string());
                            info!("Discovered skill: {} from {}", skill_name, path.display());
                        }
                        Err(e) => {
                            warn!("Failed to parse skill from {}: {}", path.display(), e);
                            stats.errors_encountered += 1;
                        }
                    }
                }

                if depth < self.config.max_depth {
                    skills.extend(self.scan_for_skills_recursive(&path, stats, depth + 1)?);
                }
            }
        }

        Ok(skills)
    }

    /// Discover CLI tools in workspace
    fn discover_cli_tools(&mut self, workspace_root: &Path, stats: &mut DiscoveryStats) -> Result<Vec<CliToolConfig>> {
        let mut tools = vec![];

        for tool_path in &self.config.tool_paths {
            let full_path = self.expand_path(tool_path, workspace_root);

            if !full_path.exists() {
                debug!("Tool path does not exist: {}", full_path.display());
                continue;
            }

            stats.directories_scanned += 1;

            match self.scan_for_tools(&full_path, stats) {
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
    fn scan_for_tools(&self, dir: &Path, stats: &mut DiscoveryStats) -> Result<Vec<CliToolConfig>> {
        let mut tools = vec![];

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                stats.files_checked += 1;

                // Single shared predicate lives in `cli_bridge` so the
                // "executable + explicit docs" rule cannot drift between
                // scanners.
                if let Some(config) = crate::skills::cli_bridge::cli_tool_config_from_executable(dir, &path) {
                    debug!("Discovered CLI tool: {} from {}", config.name, path.display());
                    tools.push(config);
                    stats.tools_found += 1;
                }
            }
        }

        Ok(tools)
    }

    /// Discover system-wide CLI tools
    fn discover_system_tools(&self, stats: &mut DiscoveryStats) -> Result<Vec<CliToolConfig>> {
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

    /// Expand path with workspace root and home directory
    fn expand_path(&self, path: &Path, workspace_root: &Path) -> PathBuf {
        if path.starts_with("~") {
            // Expand home directory
            if let Ok(home) = std::env::var("HOME") {
                let stripped = path.strip_prefix("~").unwrap_or(path);
                return PathBuf::from(home).join(stripped);
            }
        }

        if path.is_relative() {
            // Make relative to workspace root
            workspace_root.join(path)
        } else {
            path.to_path_buf()
        }
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
            skills_found: self.cache.values().map(|entry| entry.skills.len()).sum(),
            tools_found: self.cache.values().map(|entry| entry.tools.len()).sum(),
            errors_encountered: 0,
            discovery_time_ms: 0,
        }
    }
}

fn default_codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn default_skill_paths(workspace_root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let stop = find_git_root(workspace_root).unwrap_or_else(|| workspace_root.to_path_buf());
    let mut current = workspace_root.to_path_buf();

    loop {
        paths.push(current.join(".agents/skills"));
        if current == stop {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }

    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".agents/skills"));
    }
    if let Ok(vtcode_paths) = VtCodePaths::resolve() {
        paths.push(vtcode_paths.data_dir().join("skills"));
        paths.push(vtcode_paths.legacy_dir().join("skills"));
        paths.extend(vtcode_paths.system_data_paths("skills").unwrap_or_default());
    }
    #[cfg(unix)]
    paths.push(PathBuf::from("/etc/codex/skills"));
    paths.push(crate::skills::system::system_cache_root_dir(&default_codex_home()));
    paths
}

fn find_git_root(path: &Path) -> Option<PathBuf> {
    let mut current = Some(path);
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

/// Convert CLI tool configuration to SkillContext
pub fn tool_config_to_skill_context(config: &CliToolConfig) -> Result<SkillContext> {
    let manifest = SkillManifest {
        name: config.name.clone(),
        description: config.description.clone(),
        version: Some("1.0.0".to_string()),
        author: Some("VT Code CLI Discovery".to_string()),
        variety: SkillVariety::SystemUtility,
        ..Default::default()
    };

    Ok(SkillContext::MetadataOnly(manifest, config.executable_path.clone()))
}

/// Progressive skill loader that can load full skill details on demand
pub struct ProgressiveSkillLoader {
    discovery: SkillDiscovery,
    skill_cache: HashMap<String, crate::skills::types::Skill>,
}

impl ProgressiveSkillLoader {
    pub fn new(config: DiscoveryConfig) -> Self {
        Self {
            discovery: SkillDiscovery::with_config(config),
            skill_cache: HashMap::new(),
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
                return tool_config_to_skill_context(tool);
            }
        }

        Err(skill_ops::skill_not_found_error(name))
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

        Err(skill_ops::skill_not_found_error(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_discovery_config_default() {
        let config = DiscoveryConfig::default();
        assert!(config.skill_paths.is_empty());
        assert!(!config.tool_paths.is_empty());
        assert!(!config.auto_discover_system_tools); // Disabled by default for security
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
        result.unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn scan_for_tools_shares_the_documented_executable_rule() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let write_executable = |name: &str| {
            let path = temp_dir.path().join(name);
            std::fs::write(&path, "#!/bin/sh\necho hi\n").unwrap();
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        };
        // Asymmetric pair: same executable bit, docs present vs absent.
        write_executable("kept-tool");
        std::fs::write(temp_dir.path().join("kept-tool.md"), "# kept\n").unwrap();
        write_executable("dropped-tool");

        let worker = SkillDiscovery::new();
        let mut stats = DiscoveryStats::default();
        let tools = worker.scan_for_tools(temp_dir.path(), &mut stats).unwrap();
        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(names, vec!["kept-tool"], "only the documented executable is a skill");
        assert_eq!(stats.tools_found, 1);
    }
}
