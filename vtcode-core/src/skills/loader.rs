//! Enhanced Skill Discovery and Loading
//!
//! Integrates traditional VTCode skills with CLI tool skills and progressive
//! context management for efficient memory usage and comprehensive skill discovery.

use crate::skills::cli_bridge::{CliToolBridge, CliToolConfig};
use crate::skills::context_manager::{ContextManager, ContextConfig, ContextLevel};
use crate::skills::discovery::{DiscoveryConfig, SkillDiscovery};
use crate::skills::locations::{SkillLocations, DiscoveredSkill, SkillLocationType};
use crate::skills::manifest::{parse_skill_file};
use crate::skills::types::{Skill, SkillContext, SkillManifest};
use crate::skills::container_validation::{ContainerSkillsValidator, ContainerValidationResult, ContainerValidationReport};
use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Enhanced skill search paths including CLI tools
#[allow(dead_code)]
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

    /// Container skills validator
    container_validator: ContainerSkillsValidator,

    /// Whether to filter incompatible skills
    filter_incompatible_skills: bool,
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
            container_validator: ContainerSkillsValidator::new(),
            filter_incompatible_skills: true, // Filter by default for better UX
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

    /// Get skill by name (with automatic loading and lazy discovery)
    pub async fn get_skill(&mut self, name: &str) -> Result<EnhancedSkill> {
        // Try to get from context manager first (optimal path)
        if let Some(context_entry) = self.context_manager.get_skill_context(name) {
            return match context_entry.level {
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
            };
        }

        // Skill not found - attempt targeted discovery instead of full scan
        debug!("Skill '{}' not found in context manager, attempting targeted discovery", name);

        // Try to discover just this skill using locations system
        match self.discover_single_skill(name).await {
            Ok(Some(skill_context)) => {
                debug!("Successfully discovered skill '{}' during get_skill", name);
                // Register this single skill in context manager
                self.context_manager.register_skill_metadata(skill_context.manifest().clone())?;

                // Also register in discovered_skills cache for load_full_skill to find it
                if let Some(skill_path) = self.find_skill_path_from_context(name) {
                    let discovered_skill = DiscoveredSkill {
                        location_type: SkillLocationType::VtcodeProject, // Default type, could be improved
                        skill_context: skill_context.clone(),
                        skill_path: skill_path.clone(),
                        skill_name: name.to_string(),
                    };

                    if self.discovered_skills.is_none() {
                        self.discovered_skills = Some(Vec::new());
                    }
                    if let Some(cache) = &mut self.discovered_skills {
                        cache.push(discovered_skill);
                    }
                    debug!("Cached discovered skill '{}' at path '{}'", name, skill_path.display());
                }

                // Now load the full skill
                self.load_full_skill(name).await
            }
            Ok(None) => {
                // Skill doesn't exist in any location
                Err(anyhow!(
                    "Skill '{}' not found. Available skills: {}. Use 'skills list' to see all available skills.",
                    name,
                    self.get_available_skills_hint()
                ))
            }
            Err(e) => {
                // Discovery failed - provide helpful error
                Err(anyhow!(
                    "Failed to discover skill '{}': {}. Ensure the skill exists in one of the skill directories.",
                    name, e
                ))
            }
        }
    }

    /// Load full skill details using the locations system
    /// Note: This method assumes the skill has already been discovered via discover_single_skill()
    async fn load_full_skill(&mut self, name: &str) -> Result<EnhancedSkill> {
        info!("Loading full skill: {}", name);

        // We assume the skill has been discovered and is in the discovered_skills cache
        // This eliminates redundant discovery calls since get_skill() now handles discovery
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

        // If we reach here, the skill was discovered but somehow lost - this shouldn't happen
        Err(anyhow!(
            "Skill '{}' was discovered but could not be loaded. This indicates an internal error.",
            name
        ))
    }

    /// Load traditional skill from directory
    async fn load_traditional_skill(&mut self, skill_path: &Path) -> Result<EnhancedSkill> {
        let (manifest, instructions) = parse_skill_file(skill_path)?;

        let mut skill = Skill::new(manifest, skill_path.to_path_buf(), instructions)?;

        // Validate container skills compatibility
        let container_analysis = self.container_validator.analyze_skill(&skill);

        if container_analysis.should_filter && self.filter_incompatible_skills {
            warn!(
                "Skill '{}' requires container skills and will be filtered out: {}",
                skill.name(),
                container_analysis.analysis
            );
            return Err(anyhow!(
                "Skill '{}' requires Anthropic container skills which are not supported in VTCode.\n{}",
                skill.name(),
                container_analysis.recommendations.join("\n")
            ));
        } else if container_analysis.requirement != crate::skills::container_validation::ContainerSkillsRequirement::NotRequired {
            info!(
                "Skill '{}' container skills analysis: {}",
                skill.name(),
                container_analysis.analysis
            );
            for recommendation in &container_analysis.recommendations {
                info!("  - {}", recommendation);
            }
        }

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

    /// Find skill path by scanning locations for a skill with the given name
    fn find_skill_path_from_context(&self, skill_name: &str) -> Option<PathBuf> {
        // Try to find the skill path by scanning the locations
        // This is used when we have the skill context but need the path for loading
        match self.skill_locations.discover_skills() {
            Ok(discovered_skills) => {
                for discovered in discovered_skills {
                    if discovered.skill_context.manifest().name == skill_name {
                        return Some(discovered.skill_path);
                    }
                }
                None
            }
            Err(e) => {
                debug!("Failed to discover skills while looking for path for '{}': {}", skill_name, e);
                None
            }
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

    /// Discover a single skill by name (targeted discovery)
    async fn discover_single_skill(&mut self, name: &str) -> Result<Option<SkillContext>> {
        debug!("Attempting targeted discovery for skill '{}'", name);

        // Use the locations system to find just this skill
        let discovered_skills = self.skill_locations.discover_skills()?;

        // Look for the specific skill
        for discovered in discovered_skills {
            if discovered.skill_context.manifest().name == name {
                debug!("Found skill '{}' during targeted discovery", name);
                return Ok(Some(discovered.skill_context));
            }
        }

        // Also check CLI tools using traditional discovery
        let discovery_result = self.discovery.discover_all(&self.workspace_root).await?;
        for tool_config in &discovery_result.tools {
            if tool_config.name == name {
                let skill_context = crate::skills::discovery::tool_config_to_skill_context(tool_config)?;
                debug!("Found CLI tool skill '{}' during targeted discovery", name);
                return Ok(Some(skill_context));
            }
        }

        debug!("Skill '{}' not found during targeted discovery", name);
        Ok(None)
    }

    /// Get a helpful hint about available skills for error messages
    fn get_available_skills_hint(&self) -> String {
        let available = self.get_available_skills();
        if available.is_empty() {
            "no skills available".to_string()
        } else if available.len() <= 5 {
            format!("{}", available.join(", "))
        } else {
            format!("{} (and {} more)", available[..5].join(", "), available.len() - 5)
        }
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

    /// Set whether to filter incompatible container skills
    pub fn set_filter_incompatible_skills(&mut self, filter: bool) {
        self.filter_incompatible_skills = filter;
    }

    /// Check if a skill requires container skills (for testing/debugging)
    pub fn check_container_requirements(&self, skill: &Skill) -> ContainerValidationResult {
        self.container_validator.analyze_skill(skill)
    }

    /// Get container skills validator for advanced usage
    pub fn container_validator(&self) -> &ContainerSkillsValidator {
        &self.container_validator
    }

    /// Generate a comprehensive validation report for all discovered skills
    pub async fn generate_validation_report(&mut self) -> Result<ContainerValidationReport> {
        info!("Generating comprehensive container skills validation report");

        // Discover all skills first
        let discovery_result = self.discover_all_skills().await?;
        let mut report = ContainerValidationReport::new();

        // Analyze each traditional skill
        for skill_context in &discovery_result.traditional_skills {
            // We need to load the full skill to analyze it
            match self.get_skill(&skill_context.manifest().name).await {
                Ok(EnhancedSkill::Traditional(skill)) => {
                    let analysis = self.container_validator.analyze_skill(&skill);
                    report.add_skill_analysis(skill_context.manifest().name.clone(), analysis);
                }
                Ok(EnhancedSkill::CliTool(_)) => {
                    // CLI tools don't need container skills validation
                    continue;
                }
                Err(e) => {
                    // If skill failed to load due to container skills, record the error
                    if e.to_string().contains("container skills") {
                        report.add_incompatible_skill(
                            skill_context.manifest().name.clone(),
                            skill_context.manifest().description.clone(),
                            e.to_string()
                        );
                    }
                }
            }
        }

        report.finalize();
        Ok(report)
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

/// Detect skill mentions in user input (Codex-style `$SkillName` syntax)
///
/// Returns list of skill names that should be triggered based on:
/// 1. Explicit `$skill-name` mention in user input
/// 2. Description keyword matches (fuzzy matching)
///
/// # Examples
/// ```
/// let input = "Use $pdf-analyzer to process the document";
/// let skills = vec![skill_manifest_with_name("pdf-analyzer")];
/// let matches = detect_skill_mentions(input, &skills);
/// assert!(matches.contains(&"pdf-analyzer".to_string()));
/// ```
pub fn detect_skill_mentions(user_input: &str, available_skills: &[SkillManifest]) -> Vec<String> {
	let mut matches = Vec::new();
	let input_lower = user_input.to_lowercase();

	for skill in available_skills {
		// Pattern 1: Explicit $skill-name mention (Codex pattern)
		if input_lower.contains(&format!("${}", skill.name)) {
			matches.push(skill.name.clone());
			continue;
		}

		// Pattern 2: Description keyword matching (fuzzy)
		// Extract key terms from description (words 4+ chars)
		let description_keywords: Vec<&str> = skill
			.description
			.split_whitespace()
			.filter(|word| word.len() >= 4)
			.map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()))
			.collect();

		// Check if multiple keywords match (requires at least 2 matches for confidence)
		let keyword_matches = description_keywords
			.iter()
			.filter(|keyword| input_lower.contains(&keyword.to_lowercase()))
			.count();

		if keyword_matches >= 2 {
			matches.push(skill.name.clone());
		}
	}

	// Deduplicate
	matches.sort();
	matches.dedup();
	matches
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

	#[test]
	fn test_detect_skill_mentions_explicit_syntax() {
		let skills = vec![SkillManifest {
			name: "pdf-analyzer".to_string(),
			description: "Extract text and tables from PDF documents".to_string(),
			version: None,
			author: None,
			vtcode_native: None,
		}];

		// Test explicit $skill-name syntax (Codex pattern)
		let input = "Use $pdf-analyzer to process the document";
		let matches = detect_skill_mentions(input, &skills);
		assert_eq!(matches.len(), 1);
		assert_eq!(matches[0], "pdf-analyzer");
	}

	#[test]
	fn test_detect_skill_mentions_description_match() {
		let skills = vec![SkillManifest {
			name: "spreadsheet-generator".to_string(),
			description: "Generate Excel spreadsheets with data analysis and charts".to_string(),
			version: None,
			author: None,
			vtcode_native: None,
		}];

		// Test description keyword matching (requires 2+ keyword matches)
		let input = "Create a spreadsheet with analysis charts for the quarterly report";
		let matches = detect_skill_mentions(input, &skills);
		assert_eq!(matches.len(), 1);
		assert_eq!(matches[0], "spreadsheet-generator");
	}

	#[test]
	fn test_detect_skill_mentions_no_match() {
		let skills = vec![SkillManifest {
			name: "pdf-analyzer".to_string(),
			description: "Extract text and tables from PDF documents".to_string(),
			version: None,
			author: None,
			vtcode_native: None,
		}];

		// No match - only 1 keyword
		let input = "Process this document";
		let matches = detect_skill_mentions(input, &skills);
		assert_eq!(matches.len(), 0);
	}

	#[test]
	fn test_detect_skill_mentions_case_insensitive() {
		let skills = vec![SkillManifest {
			name: "doc-generator".to_string(),
			description: "Generate technical documentation".to_string(),
			version: None,
			author: None,
			vtcode_native: None,
		}];

		// Case insensitive matching
		let input = "Use $DOC-GENERATOR to create the docs";
		let matches = detect_skill_mentions(input, &skills);
		assert_eq!(matches.len(), 1);
		assert_eq!(matches[0], "doc-generator");
	}

	#[test]
	fn test_detect_skill_mentions_multiple_skills() {
		let skills = vec![
			SkillManifest {
				name: "pdf-analyzer".to_string(),
				description: "Extract text from PDF documents".to_string(),
				version: None,
				author: None,
				vtcode_native: None,
			},
			SkillManifest {
				name: "spreadsheet-generator".to_string(),
				description: "Generate Excel spreadsheets with charts".to_string(),
				version: None,
				author: None,
				vtcode_native: None,
			},
		];

		// Multiple skills triggered
		let input = "Extract data from PDF and create spreadsheet with charts";
		let matches = detect_skill_mentions(input, &skills);
		assert!(matches.len() >= 1); // At least one should match
	}
}