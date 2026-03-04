use crate::skills::cli_bridge::{CliToolBridge, CliToolConfig, discover_cli_tools};
use crate::skills::container_validation::{
    ContainerSkillsValidator, ContainerValidationReport, ContainerValidationResult,
};
use crate::skills::discovery::{DiscoveryResult, SkillDiscovery};
use crate::skills::model::{SkillErrorInfo, SkillLoadOutcome, SkillMetadata, SkillScope};
use crate::skills::system::system_cache_root_dir;
use crate::skills::types::{Skill, SkillContext, SkillManifest};
use anyhow::{Context, Result};
use dunce::canonicalize as normalize_path;
use hashbrown::HashSet;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::error;

// Config for loader
#[derive(Debug, Clone)]
pub struct SkillLoaderConfig {
    pub codex_home: PathBuf,
    pub cwd: PathBuf,
    pub project_root: Option<PathBuf>,
}

pub struct SkillRoot {
    pub path: PathBuf,
    pub scope: SkillScope,
    pub is_tool_root: bool,
    pub is_plugin_root: bool,
}

pub fn load_skills(config: &SkillLoaderConfig) -> SkillLoadOutcome {
    load_skills_with_home_dir(config, Some(&config.codex_home))
}

/// Lightweight metadata discovery that avoids parsing SKILL.md files.
/// Returns skill stubs with only name, description, and path (no manifest parsing).
/// This is much faster for listing available skills.
pub fn discover_skill_metadata_lightweight(config: &SkillLoaderConfig) -> SkillLoadOutcome {
    discover_skill_metadata_lightweight_with_home_dir(config, Some(&config.codex_home))
}

/// Internal helper that allows specifying an explicit home directory.
/// This is useful for testing to avoid picking up real user skills from ~/.agents/skills.
fn load_skills_with_home_dir(
    config: &SkillLoaderConfig,
    home_dir: Option<&Path>,
) -> SkillLoadOutcome {
    let mut outcome = SkillLoadOutcome::default();
    let roots = skill_roots_with_home_dir(config, home_dir);

    for root in roots {
        discover_skills_under_root(&root, &mut outcome);
    }

    add_system_cli_tools(&mut outcome);
    dedup_and_sort(&mut outcome);
    outcome
}

/// Internal helper for lightweight discovery with explicit home directory.
/// Useful for hermetic tests.
fn discover_skill_metadata_lightweight_with_home_dir(
    config: &SkillLoaderConfig,
    home_dir: Option<&Path>,
) -> SkillLoadOutcome {
    let mut outcome = SkillLoadOutcome::default();
    let roots = skill_roots_with_home_dir(config, home_dir);

    for root in roots {
        discover_metadata_under_root(&root, &mut outcome);
    }

    add_system_cli_tools(&mut outcome);
    dedup_and_sort(&mut outcome);
    outcome
}

fn add_system_cli_tools(outcome: &mut SkillLoadOutcome) {
    if let Ok(system_tools) = discover_cli_tools() {
        for tool in system_tools {
            if let Ok(skill) = tool_config_to_metadata(&tool, SkillScope::System) {
                outcome.skills.push(skill);
            }
        }
    }
}

fn dedup_and_sort(outcome: &mut SkillLoadOutcome) {
    let mut seen: HashSet<String> = HashSet::new();
    outcome
        .skills
        .retain(|skill| seen.insert(skill.name.clone()));
    outcome.skills.sort_by(|a, b| a.name.cmp(&b.name));
}

fn skill_roots_with_home_dir(
    config: &SkillLoaderConfig,
    home_dir: Option<&Path>,
) -> Vec<SkillRoot> {
    let mut roots = Vec::new();

    // 1. Repo/Project roots (highest priority)
    // We check for .agents/skills, .codex/skills, .vtcode/skills, etc.
    if let Some(project_root) = &config.project_root {
        // Traditional skills
        roots.push(SkillRoot {
            path: project_root.join(".agents/skills"),
            scope: SkillScope::Repo,
            is_tool_root: false,
            is_plugin_root: false,
        });
        roots.push(SkillRoot {
            path: project_root.join(".codex/skills"),
            scope: SkillScope::Repo,
            is_tool_root: false,
            is_plugin_root: false,
        });
        roots.push(SkillRoot {
            path: project_root.join(".vtcode/skills"),
            scope: SkillScope::Repo,
            is_tool_root: false,
            is_plugin_root: false,
        });
        roots.push(SkillRoot {
            path: project_root.join("skills"),
            scope: SkillScope::Repo,
            is_tool_root: false,
            is_plugin_root: false,
        });

        // Plugin roots (native code plugins)
        roots.push(SkillRoot {
            path: project_root.join(".agents/plugins"),
            scope: SkillScope::Repo,
            is_tool_root: false,
            is_plugin_root: true,
        });
        roots.push(SkillRoot {
            path: project_root.join(".vtcode/plugins"),
            scope: SkillScope::Repo,
            is_tool_root: false,
            is_plugin_root: true,
        });

        // Tool roots
        roots.push(SkillRoot {
            path: project_root.join("tools"),
            scope: SkillScope::Repo,
            is_tool_root: true,
            is_plugin_root: false,
        });
        roots.push(SkillRoot {
            path: project_root.join("vendor/tools"),
            scope: SkillScope::Repo,
            is_tool_root: true,
            is_plugin_root: false,
        });
    }

    // 2. User roots (only if home_dir is provided)
    if let Some(home) = home_dir {
        roots.push(SkillRoot {
            path: home.join("skills"),
            scope: SkillScope::User,
            is_tool_root: false,
            is_plugin_root: false,
        });
        roots.push(SkillRoot {
            path: home.join("tools"),
            scope: SkillScope::User,
            is_tool_root: true,
            is_plugin_root: false,
        });
        // User plugins
        roots.push(SkillRoot {
            path: home.join(".vtcode/plugins"),
            scope: SkillScope::User,
            is_tool_root: false,
            is_plugin_root: true,
        });
    }

    // 3. System roots
    roots.push(SkillRoot {
        path: system_cache_root_dir(&config.codex_home),
        scope: SkillScope::System,
        is_tool_root: false,
        is_plugin_root: false,
    });

    roots
}

fn discover_skills_under_root(root: &SkillRoot, outcome: &mut SkillLoadOutcome) {
    let Ok(root_path) = normalize_path(&root.path) else {
        return;
    };

    if !root_path.is_dir() {
        return;
    }

    let mut queue: VecDeque<PathBuf> = VecDeque::from([root_path]);
    while let Some(dir) = queue.pop_front() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                error!("failed to read skills dir {}: {e:#}", dir.display());
                continue;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = match path.file_name().and_then(|f| f.to_str()) {
                Some(name) => name,
                None => continue,
            };

            if file_name.starts_with('.') {
                continue;
            }

            if path.is_dir() {
                queue.push_back(path.clone());

                // If this is a tool root or we are in a generic scan, check for tool directory structure
                // Assuming tool dir has tool.json or executable
                if root.is_tool_root
                    && let Ok(Some(tool_meta)) = try_load_tool_from_dir(&path, root.scope)
                {
                    outcome.skills.push(tool_meta);
                }

                // If this is a plugin root, check for native plugin directory structure
                if root.is_plugin_root
                    && let Ok(Some(plugin_meta)) = try_load_plugin_from_dir(&path, root.scope)
                {
                    outcome.skills.push(plugin_meta);
                }
                continue;
            }

            // Check for traditional skill
            if file_name == "SKILL.md" {
                match crate::skills::manifest::parse_skill_file(&path) {
                    Ok((manifest, _)) => {
                        outcome.skills.push(SkillMetadata {
                            name: manifest.name.clone(),
                            description: manifest.description.clone(),
                            short_description: None,
                            path: path.clone(),
                            scope: root.scope,
                            manifest: Some(manifest),
                        });
                    }
                    Err(err) => {
                        if root.scope != SkillScope::System {
                            outcome.errors.push(SkillErrorInfo {
                                path: path.clone(),
                                message: err.to_string(),
                            });
                        }
                    }
                }
            } else if root.is_tool_root && is_executable_file(&path) {
                // Standalone executable tool?
                // We typically look for directories, but maybe standalone files too.
                // For now, let's stick to directory-based tools or tools with README.
            }
        }
    }
}

/// Lightweight metadata discovery without parsing SKILL.md files.
/// Extracts skill name and description from filesystem structure only.
/// Much faster than full discovery, suitable for quick skill listing.
fn discover_metadata_under_root(root: &SkillRoot, outcome: &mut SkillLoadOutcome) {
    let Ok(root_path) = normalize_path(&root.path) else {
        return;
    };

    if !root_path.is_dir() {
        return;
    }

    let mut queue: VecDeque<PathBuf> = VecDeque::from([root_path]);
    while let Some(dir) = queue.pop_front() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::debug!("failed to read skills dir {}: {e:#}", dir.display());
                continue;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = match path.file_name().and_then(|f| f.to_str()) {
                Some(name) => name,
                None => continue,
            };

            if file_name.starts_with('.') {
                continue;
            }

            if path.is_dir() {
                queue.push_back(path.clone());

                // For tools, try to extract metadata without full parsing
                if root.is_tool_root
                    && let Ok(Some(tool_meta)) = try_load_tool_from_dir(&path, root.scope)
                {
                    outcome.skills.push(tool_meta);
                }
                continue;
            }

            // Check for SKILL.md but only extract stub metadata
            // Extract skill name from parent directory name as fallback
            if file_name == "SKILL.md"
                && let Some(skill_dir_name) = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
            {
                // Create minimal metadata stub without parsing
                // This allows quick skill listing with ~50-100 tokens instead of full manifest parsing
                outcome.skills.push(SkillMetadata {
                    name: skill_dir_name.to_string(),
                    description: format!("Skill from {}", skill_dir_name), // Placeholder
                    short_description: None,
                    path: path.clone(),
                    scope: root.scope,
                    manifest: None, // Important: Don't parse manifest
                });
            }
        }
    }
}

fn try_load_tool_from_dir(path: &Path, scope: SkillScope) -> Result<Option<SkillMetadata>> {
    // Check if it's a CLI tool directory (has tool.json or is executable inside)
    // Simplified: check for tool.json
    let tool_bridge = if path.join("tool.json").exists() {
        CliToolBridge::from_directory(path)?
    } else {
        // Heuristic: check for executable with same name as dir?
        // This is complex to reproduce exactly "discovery.rs" logic without code dupe.
        // I'll be conservative and require tool.json OR evident executable.
        match CliToolBridge::from_directory(path) {
            Ok(b) => b,
            Err(_) => return Ok(None),
        }
    };

    tool_config_to_metadata(&tool_bridge.config, scope).map(Some)
}

fn tool_config_to_metadata(config: &CliToolConfig, scope: SkillScope) -> Result<SkillMetadata> {
    Ok(SkillMetadata {
        name: config.name.clone(),
        description: config.description.clone(),
        short_description: None,
        path: config.executable_path.clone(), // Path to executable is the "path" of the skill?
        // Or path to directory? Reference uses SKILL.md path.
        // Here we use executable path or tool directory.
        scope,
        manifest: None, // CLI tools don't have a manifest in the same sense, or we could synthesize one
    })
}

fn try_load_plugin_from_dir(path: &Path, scope: SkillScope) -> Result<Option<SkillMetadata>> {
    // Check if it's a native plugin directory (has plugin.json)
    let plugin_json_path = path.join("plugin.json");
    if !plugin_json_path.exists() {
        return Ok(None);
    }

    // Read and parse plugin metadata
    let plugin_json_content =
        fs::read_to_string(&plugin_json_path).context("Failed to read plugin.json")?;

    let plugin_metadata: crate::skills::native_plugin::PluginMetadata =
        serde_json::from_str(&plugin_json_content).context("Invalid plugin.json format")?;

    // Validate that the plugin has a corresponding dynamic library
    let lib_name =
        crate::skills::native_plugin::PluginLoader::new().library_filename(&plugin_metadata.name);

    if !path.join(&lib_name).exists() {
        // Try alternative library names
        let alternatives = [
            format!("lib{}.dylib", plugin_metadata.name),
            format!("{}.dylib", plugin_metadata.name),
            format!("lib{}.so", plugin_metadata.name),
            format!("{}.so", plugin_metadata.name),
            format!("{}.dll", plugin_metadata.name),
        ];

        let has_lib = alternatives.iter().any(|alt| path.join(alt).exists());
        if !has_lib {
            return Ok(None); // No library found, skip this plugin
        }
    }

    Ok(Some(SkillMetadata {
        name: plugin_metadata.name.clone(),
        description: plugin_metadata.description.clone(),
        short_description: None,
        path: path.to_path_buf(),
        scope,
        manifest: None, // Native plugins don't have SKILL.md manifest
    }))
}

pub fn load_skill_resources(skill_path: &Path) -> Result<Vec<crate::skills::types::SkillResource>> {
    let mut resources = Vec::new();
    let resource_dir = skill_path.join("scripts");

    if resource_dir.exists() {
        for entry in fs::read_dir(&resource_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let rel_path = path
                    .strip_prefix(skill_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let resource_type = match path.extension().and_then(|e| e.to_str()) {
                    Some("py") | Some("sh") | Some("bash") => {
                        crate::skills::types::ResourceType::Script
                    }
                    Some("md") => crate::skills::types::ResourceType::Markdown,
                    Some("json") | Some("yaml") | Some("yml") => {
                        crate::skills::types::ResourceType::Reference
                    }
                    _ => {
                        crate::skills::types::ResourceType::Other(format!("{:?}", path.extension()))
                    }
                };

                resources.push(crate::skills::types::SkillResource {
                    path: rel_path,
                    resource_type,
                    content: None,
                });
            }
        }
    }

    // Check for references/ directory
    let references_dir = skill_path.join("references");
    if references_dir.exists() {
        for entry in fs::read_dir(&references_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let rel_path = path
                    .strip_prefix(skill_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let resource_type = match path.extension().and_then(|e| e.to_str()) {
                    Some("md") => crate::skills::types::ResourceType::Reference,
                    Some("json") | Some("yaml") | Some("yml") | Some("txt") | Some("csv") => {
                        crate::skills::types::ResourceType::Reference
                    }
                    _ => {
                        crate::skills::types::ResourceType::Other(format!("{:?}", path.extension()))
                    }
                };

                resources.push(crate::skills::types::SkillResource {
                    path: rel_path,
                    resource_type,
                    content: None,
                });
            }
        }
    }

    // Check for assets/ directory
    let assets_dir = skill_path.join("assets");
    if assets_dir.exists() {
        for entry in fs::read_dir(&assets_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                let rel_path = path
                    .strip_prefix(skill_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                let resource_type = match path.extension().and_then(|e| e.to_str()) {
                    Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("svg") => {
                        crate::skills::types::ResourceType::Asset
                    }
                    Some("json") | Some("yaml") | Some("yml") | Some("txt") | Some("csv") => {
                        crate::skills::types::ResourceType::Asset
                    }
                    _ => crate::skills::types::ResourceType::Asset,
                };

                resources.push(crate::skills::types::SkillResource {
                    path: rel_path,
                    resource_type,
                    content: None,
                });
            }
        }
    }

    Ok(resources)
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = path.metadata() {
            return meta.permissions().mode() & 0o111 != 0;
        }
    }
    #[cfg(windows)]
    {
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            return matches!(ext.to_lowercase().as_str(), "exe" | "bat" | "cmd");
        }
    }
    false
}

/// Enhanced skill variant for unified handling
pub enum EnhancedSkill {
    /// Traditional instruction-based skill
    Traditional(Box<Skill>),
    /// CLI-based tool skill
    CliTool(Box<CliToolBridge>),
    /// Native code plugin skill
    NativePlugin(Box<dyn crate::skills::native_plugin::NativePluginTrait>),
}

impl std::fmt::Debug for EnhancedSkill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Traditional(skill) => f.debug_tuple("Traditional").field(skill).finish(),
            Self::CliTool(tool) => f.debug_tuple("CliTool").field(tool).finish(),
            Self::NativePlugin(plugin) => f.debug_tuple("NativePlugin").field(plugin).finish(),
        }
    }
}

/// High-level loader that provides discovery and validation features
pub struct EnhancedSkillLoader {
    workspace_root: PathBuf,
    discovery: SkillDiscovery,
    plugin_loader: crate::skills::native_plugin::PluginLoader,
}

impl EnhancedSkillLoader {
    /// Create a new enhanced loader for workspace
    pub fn new(workspace_root: PathBuf) -> Self {
        let mut plugin_loader = crate::skills::native_plugin::PluginLoader::new();

        // Add trusted plugin directories
        plugin_loader
            // User plugins
            .add_trusted_dir(dirs::home_dir().unwrap_or_default().join(".vtcode/plugins"))
            // Project plugins
            .add_trusted_dir(workspace_root.join(".vtcode/plugins"))
            .add_trusted_dir(workspace_root.join(".agents/plugins"));

        Self {
            workspace_root,
            discovery: SkillDiscovery::new(),
            plugin_loader,
        }
    }

    /// Discover all available skills and tools
    pub async fn discover_all_skills(&mut self) -> Result<DiscoveryResult> {
        self.discovery.discover_all(&self.workspace_root).await
    }

    /// Get a specific skill by name
    pub async fn get_skill(&mut self, name: &str) -> Result<EnhancedSkill> {
        let result = self.discovery.discover_all(&self.workspace_root).await?;

        // Try traditional skills first
        for skill_ctx in &result.skills {
            if skill_ctx.manifest().name == name {
                let path = skill_ctx.path();
                let (manifest, instructions) = crate::skills::manifest::parse_skill_file(path)?;
                let skill = Skill::new(manifest, path.clone(), instructions)?;
                return Ok(EnhancedSkill::Traditional(Box::new(skill)));
            }
        }

        // Try CLI tools
        for tool_config in &result.tools {
            if tool_config.name == name {
                let bridge = CliToolBridge::new(tool_config.clone())?;
                return Ok(EnhancedSkill::CliTool(Box::new(bridge)));
            }
        }

        // Try native plugins - discover plugin directories and load on demand
        // First, find the plugin directory by scanning trusted directories
        for plugin_dir in self.get_plugin_directories() {
            if !plugin_dir.exists() {
                continue;
            }

            // Check if this directory contains the requested plugin
            let plugin_json = plugin_dir.join("plugin.json");
            if let Ok(content) = fs::read_to_string(&plugin_json)
                && let Ok(metadata) =
                    serde_json::from_str::<crate::skills::native_plugin::PluginMetadata>(&content)
                && metadata.name == name
            {
                // Load the plugin
                let plugin = self.plugin_loader.load_plugin(&plugin_dir)?;
                return Ok(EnhancedSkill::NativePlugin(plugin));
            }
        }

        Err(anyhow::anyhow!("Skill '{}' not found", name))
    }

    /// Get all trusted plugin directories
    fn get_plugin_directories(&self) -> Vec<PathBuf> {
        self.plugin_loader.trusted_dirs().to_vec()
    }

    /// Generate a comprehensive container validation report
    pub async fn generate_validation_report(&mut self) -> Result<ContainerValidationReport> {
        let result = self.discovery.discover_all(&self.workspace_root).await?;
        let mut report = ContainerValidationReport::new();
        let validator = ContainerSkillsValidator::new();

        for skill_ctx in &result.skills {
            match self.load_full_skill_from_ctx(skill_ctx) {
                Ok(skill) => {
                    let analysis = validator.analyze_skill(&skill);
                    report.add_skill_analysis(skill.name().to_string(), analysis);
                }
                Err(e) => {
                    report.add_incompatible_skill(
                        skill_ctx.manifest().name.clone(),
                        skill_ctx.manifest().description.clone(),
                        format!("Load error: {}", e),
                    );
                }
            }
        }

        report.finalize();
        Ok(report)
    }

    /// Check container requirements for a skill
    pub fn check_container_requirements(&self, skill: &Skill) -> ContainerValidationResult {
        let validator = ContainerSkillsValidator::new();
        validator.analyze_skill(skill)
    }

    fn load_full_skill_from_ctx(&self, ctx: &SkillContext) -> Result<Skill> {
        let path = ctx.path();
        let (manifest, instructions) = crate::skills::manifest::parse_skill_file(path)?;
        Skill::new(manifest, path.clone(), instructions)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SkillMentionDetectionOptions {
    pub enable_auto_trigger: bool,
    pub enable_description_matching: bool,
    pub min_keyword_matches: usize,
}

impl Default for SkillMentionDetectionOptions {
    fn default() -> Self {
        Self {
            enable_auto_trigger: true,
            enable_description_matching: true,
            min_keyword_matches: 2,
        }
    }
}

/// Detect skill mentions using default routing options.
pub fn detect_skill_mentions(user_input: &str, available_skills: &[SkillManifest]) -> Vec<String> {
    detect_skill_mentions_with_options(
        user_input,
        available_skills,
        &SkillMentionDetectionOptions::default(),
    )
}

/// Detect skill mentions using explicit routing options.
///
/// Routing policy:
/// - Explicit `$skill-name` mentions always win.
/// - Description/when-to-use keywords provide positive signal.
/// - when-not-to-use keywords provide negative signal and can veto weak matches.
pub fn detect_skill_mentions_with_options(
    user_input: &str,
    available_skills: &[SkillManifest],
    options: &SkillMentionDetectionOptions,
) -> Vec<String> {
    if !options.enable_auto_trigger {
        return Vec::new();
    }

    let mut mentions = Vec::new();
    let input_lower = user_input.to_lowercase();
    let input_keywords = extract_keywords(user_input);
    let min_matches = options.min_keyword_matches.max(1);

    for skill in available_skills {
        let skill_name_lower = skill.name.to_lowercase();
        let explicit_trigger = format!("${skill_name_lower}");
        if input_lower.contains(&explicit_trigger) {
            mentions.push(skill.name.clone());
            continue;
        }

        if !options.enable_description_matching {
            continue;
        }

        let description_keywords = extract_keywords(&skill.description);
        let when_to_use_keywords = skill
            .when_to_use
            .as_deref()
            .map(extract_keywords)
            .unwrap_or_default();
        let when_not_to_use_keywords = skill
            .when_not_to_use
            .as_deref()
            .map(extract_keywords)
            .unwrap_or_default();

        let description_matches = overlap_count(&input_keywords, &description_keywords);
        let use_matches = overlap_count(&input_keywords, &when_to_use_keywords);
        let avoid_matches = overlap_count(&input_keywords, &when_not_to_use_keywords);
        let positive_matches = description_matches + use_matches;

        if avoid_matches > 0 && use_matches == 0 && description_matches <= avoid_matches {
            continue;
        }

        if positive_matches >= min_matches {
            mentions.push(skill.name.clone());
        }
    }

    mentions.sort();
    mentions.dedup();
    mentions
}

fn overlap_count(input_keywords: &HashSet<String>, skill_keywords: &HashSet<String>) -> usize {
    input_keywords.intersection(skill_keywords).count()
}

fn extract_keywords(text: &str) -> HashSet<String> {
    const STOPWORDS: &[&str] = &[
        "the", "and", "with", "from", "that", "this", "when", "where", "what", "your", "for",
        "into", "onto", "than", "then", "also", "only", "should", "would", "could", "have", "has",
        "had", "use", "using", "task", "tasks", "help", "need", "want",
    ];

    text.split(|c: char| !c.is_alphanumeric())
        .map(|part| part.trim().to_lowercase())
        .filter(|part| part.len() > 2)
        .filter(|part| !STOPWORDS.contains(&part.as_str()))
        .collect()
}

/// Test helper for hermetic skill loading that does not pick up user skills.
/// Use this in tests to avoid failures when ~/.agents/skills contains skills.
#[cfg(test)]
pub fn load_skills_hermetic(config: &SkillLoaderConfig) -> SkillLoadOutcome {
    load_skills_with_home_dir(config, None)
}

/// Test helper for hermetic lightweight skill discovery.
#[cfg(test)]
pub fn discover_skill_metadata_lightweight_hermetic(
    config: &SkillLoaderConfig,
) -> SkillLoadOutcome {
    discover_skill_metadata_lightweight_with_home_dir(config, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(
        name: &str,
        description: &str,
        when_to_use: Option<&str>,
        when_not_to_use: Option<&str>,
    ) -> SkillManifest {
        SkillManifest {
            name: name.to_string(),
            description: description.to_string(),
            when_to_use: when_to_use.map(ToOwned::to_owned),
            when_not_to_use: when_not_to_use.map(ToOwned::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn detects_explicit_skill_mentions() {
        let skills = vec![manifest(
            "pdf-analyzer",
            "Analyze PDF files and extract tables",
            None,
            None,
        )];
        let mentions = detect_skill_mentions("Use $pdf-analyzer for this file", &skills);
        assert_eq!(mentions, vec!["pdf-analyzer".to_string()]);
    }

    #[test]
    fn negative_signal_blocks_weak_keyword_match() {
        let skills = vec![manifest(
            "api-fetcher",
            "Fetch data from API endpoints and summarize responses",
            Some("Use for batch API analytics and endpoint inventories"),
            Some("Do not use for local file edits or static markdown updates"),
        )];

        let mentions = detect_skill_mentions(
            "Please update this local markdown file and fix headings",
            &skills,
        );
        assert!(mentions.is_empty());
    }

    #[test]
    fn auto_trigger_can_be_disabled() {
        let skills = vec![manifest(
            "sql-checker",
            "Validate SQL migration scripts for safety",
            None,
            None,
        )];
        let options = SkillMentionDetectionOptions {
            enable_auto_trigger: false,
            ..Default::default()
        };
        let mentions = detect_skill_mentions_with_options("Use $sql-checker", &skills, &options);
        assert!(mentions.is_empty());
    }
}
