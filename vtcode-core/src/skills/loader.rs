use crate::skills::cli_bridge::{CliToolBridge, CliToolConfig, discover_cli_tools};
use crate::skills::command_skills::{
    BuiltInCommandSkill, built_in_command_skill, merge_built_in_command_skill_contexts,
};
use crate::skills::container_validation::{
    ContainerSkillsValidator, ContainerValidationReport, ContainerValidationResult,
};
use crate::skills::discovery::{DiscoveryConfig, DiscoveryResult, SkillDiscovery};
use crate::skills::model::{SkillErrorInfo, SkillLoadOutcome, SkillMetadata, SkillScope};
use crate::skills::system::{install_system_skills, system_cache_root_dir};
use crate::skills::types::{Skill, SkillContext, SkillManifest};
use anyhow::{Context, Result};
use dunce::canonicalize as normalize_path;
use hashbrown::{HashMap, HashSet};
use serde::Deserialize;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, SystemTime};
use tracing::{error, warn};

// Config for loader
#[derive(Debug, Clone)]
pub struct SkillLoaderConfig {
    pub codex_home: PathBuf,
    pub cwd: PathBuf,
    pub project_root: Option<PathBuf>,
    pub include_bundled_system_skills: bool,
}

pub struct SkillRoot {
    pub path: PathBuf,
    pub scope: SkillScope,
    pub is_tool_root: bool,
    pub is_plugin_root: bool,
}

const LIGHTWEIGHT_SKILL_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const LIGHTWEIGHT_SKILL_CACHE_MAX_ENTRIES: usize = 32;

static LIGHTWEIGHT_SKILL_METADATA_CACHE: OnceLock<
    RwLock<HashMap<LightweightSkillCacheKey, CachedLightweightSkillOutcome>>,
> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LightweightSkillCacheKey {
    codex_home: PathBuf,
    cwd: PathBuf,
    project_root: Option<PathBuf>,
    include_bundled_system_skills: bool,
    home_dir: Option<PathBuf>,
}

impl LightweightSkillCacheKey {
    fn new(config: &SkillLoaderConfig, home_dir: Option<&Path>) -> Self {
        Self {
            codex_home: normalize_cache_path(&config.codex_home),
            cwd: normalize_cache_path(&config.cwd),
            project_root: config.project_root.as_deref().map(normalize_cache_path),
            include_bundled_system_skills: config.include_bundled_system_skills,
            home_dir: home_dir.map(normalize_cache_path),
        }
    }
}

#[derive(Clone)]
struct CachedLightweightSkillOutcome {
    outcome: SkillLoadOutcome,
    timestamp: SystemTime,
}

impl CachedLightweightSkillOutcome {
    fn is_expired(&self) -> bool {
        self.timestamp
            .elapsed()
            .unwrap_or(LIGHTWEIGHT_SKILL_CACHE_TTL)
            > LIGHTWEIGHT_SKILL_CACHE_TTL
    }
}

fn normalize_cache_path(path: &Path) -> PathBuf {
    normalize_path(path).unwrap_or_else(|_| path.to_path_buf())
}

fn lightweight_skill_metadata_cache()
-> &'static RwLock<HashMap<LightweightSkillCacheKey, CachedLightweightSkillOutcome>> {
    LIGHTWEIGHT_SKILL_METADATA_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_cached_lightweight_skill_outcome(
    key: &LightweightSkillCacheKey,
) -> Option<SkillLoadOutcome> {
    match lightweight_skill_metadata_cache().read() {
        Ok(cache) => cache
            .get(key)
            .filter(|cached| !cached.is_expired())
            .map(|cached| cached.outcome.clone()),
        Err(_) => {
            warn!("lightweight skill metadata cache lock poisoned while reading cache");
            None
        }
    }
}

fn cache_lightweight_skill_outcome(key: LightweightSkillCacheKey, outcome: &SkillLoadOutcome) {
    match lightweight_skill_metadata_cache().write() {
        Ok(mut cache) => {
            if cache.len() >= LIGHTWEIGHT_SKILL_CACHE_MAX_ENTRIES && !cache.contains_key(&key) {
                let expired: Vec<_> = cache
                    .iter()
                    .filter(|(_, value)| value.is_expired())
                    .map(|(cache_key, _)| cache_key.clone())
                    .collect();

                for cache_key in expired {
                    cache.remove(&cache_key);
                }

                if cache.len() >= LIGHTWEIGHT_SKILL_CACHE_MAX_ENTRIES {
                    let oldest_key = cache
                        .iter()
                        .min_by_key(|(_, value)| value.timestamp)
                        .map(|(cache_key, _)| cache_key.clone());
                    if let Some(oldest_key) = oldest_key {
                        cache.remove(&oldest_key);
                    }
                }
            }

            cache.insert(
                key,
                CachedLightweightSkillOutcome {
                    outcome: outcome.clone(),
                    timestamp: SystemTime::now(),
                },
            );
        }
        Err(_) => warn!("lightweight skill metadata cache lock poisoned while writing cache"),
    }
}

pub(crate) fn clear_lightweight_skill_metadata_cache() {
    match lightweight_skill_metadata_cache().write() {
        Ok(mut cache) => cache.clear(),
        Err(_) => warn!("lightweight skill metadata cache lock poisoned while clearing cache"),
    }
}

pub fn load_skills(config: &SkillLoaderConfig) -> SkillLoadOutcome {
    let home_dir = dirs::home_dir();
    load_skills_with_home_dir(config, home_dir.as_deref())
}

/// Lightweight metadata discovery that avoids parsing SKILL.md files.
/// Returns skill stubs with only name, description, and path (no manifest parsing).
/// This is much faster for listing available skills.
pub fn discover_skill_metadata_lightweight(config: &SkillLoaderConfig) -> SkillLoadOutcome {
    let home_dir = dirs::home_dir();
    discover_skill_metadata_lightweight_with_home_dir(config, home_dir.as_deref())
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
    filter_disabled_skills(&mut outcome, home_dir);
    outcome
}

/// Internal helper for lightweight discovery with explicit home directory.
/// Useful for hermetic tests.
fn discover_skill_metadata_lightweight_with_home_dir(
    config: &SkillLoaderConfig,
    home_dir: Option<&Path>,
) -> SkillLoadOutcome {
    let cache_key = LightweightSkillCacheKey::new(config, home_dir);
    if let Some(cached) = get_cached_lightweight_skill_outcome(&cache_key) {
        return cached;
    }

    let outcome = discover_skill_metadata_lightweight_uncached(config, home_dir);
    cache_lightweight_skill_outcome(cache_key, &outcome);
    outcome
}

fn discover_skill_metadata_lightweight_uncached(
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
    filter_disabled_skills(&mut outcome, home_dir);
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

#[derive(Debug, Default, Deserialize)]
struct CodexConfig {
    #[serde(default)]
    skills: CodexSkillsConfig,
}

#[derive(Debug, Default, Deserialize)]
struct CodexSkillsConfig {
    #[serde(default)]
    config: Vec<CodexSkillToggle>,
}

#[derive(Debug, Deserialize)]
struct CodexSkillToggle {
    #[serde(default)]
    path: Option<PathBuf>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default = "default_skill_toggle_enabled")]
    enabled: bool,
}

#[derive(Debug, Default)]
struct DisabledSkillSelectors {
    paths: HashSet<PathBuf>,
    names: HashSet<String>,
}

fn default_skill_toggle_enabled() -> bool {
    true
}

fn filter_disabled_skills(outcome: &mut SkillLoadOutcome, home_dir: Option<&Path>) {
    let disabled = disabled_skill_selectors(home_dir);
    if disabled.paths.is_empty() && disabled.names.is_empty() {
        return;
    }

    outcome.skills.retain(|skill| {
        let canonical = normalize_path(&skill.path).unwrap_or_else(|_| skill.path.clone());
        !disabled.paths.contains(&canonical) && !disabled.names.contains(&skill.name)
    });
}

fn disabled_skill_selectors(home_dir: Option<&Path>) -> DisabledSkillSelectors {
    let Some(home_dir) = home_dir else {
        return DisabledSkillSelectors::default();
    };

    let config_path = home_dir.join(".codex").join("config.toml");
    let Ok(content) = fs::read_to_string(&config_path) else {
        return DisabledSkillSelectors::default();
    };
    let Ok(config) = toml::from_str::<CodexConfig>(&content) else {
        return DisabledSkillSelectors::default();
    };

    let mut selectors = DisabledSkillSelectors::default();
    for entry in config
        .skills
        .config
        .into_iter()
        .filter(|entry| !entry.enabled)
    {
        if let Some(path) = entry.path {
            selectors
                .paths
                .insert(normalize_path(&path).unwrap_or(path));
        }
        if let Some(name) = entry.name.map(|name| name.trim().to_string())
            && !name.is_empty()
        {
            selectors.names.insert(name);
        }
    }

    selectors
}

fn skill_roots_with_home_dir(
    config: &SkillLoaderConfig,
    home_dir: Option<&Path>,
) -> Vec<SkillRoot> {
    let mut roots = Vec::new();

    for repo_dir in repo_skill_search_dirs(config) {
        roots.push(SkillRoot {
            path: repo_dir.join(".agents/skills"),
            scope: SkillScope::Repo,
            is_tool_root: false,
            is_plugin_root: false,
        });
    }

    if let Some(project_root) = &config.project_root {
        roots.push(SkillRoot {
            path: project_root.join(".agents/plugins"),
            scope: SkillScope::Repo,
            is_tool_root: false,
            is_plugin_root: true,
        });
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

    if let Some(home) = home_dir {
        roots.push(SkillRoot {
            path: home.join(".agents/skills"),
            scope: SkillScope::User,
            is_tool_root: false,
            is_plugin_root: false,
        });
    }

    #[cfg(unix)]
    roots.push(SkillRoot {
        path: PathBuf::from("/etc/codex/skills"),
        scope: SkillScope::Admin,
        is_tool_root: false,
        is_plugin_root: false,
    });

    if config.include_bundled_system_skills {
        roots.push(SkillRoot {
            path: system_cache_root_dir(&config.codex_home),
            scope: SkillScope::System,
            is_tool_root: false,
            is_plugin_root: false,
        });
    }

    roots
}

fn repo_skill_search_dirs(config: &SkillLoaderConfig) -> Vec<PathBuf> {
    let stop = config
        .project_root
        .clone()
        .unwrap_or_else(|| config.cwd.clone());
    let mut dirs = Vec::new();
    let mut current = config.cwd.clone();

    loop {
        dirs.push(current.clone());
        if current == stop {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }

    dirs
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
                let Some(skill_dir) = path.parent() else {
                    continue;
                };
                match crate::skills::manifest::parse_skill_file(skill_dir) {
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

/// Lightweight metadata discovery that parses only SKILL.md frontmatter.
/// This preserves routing-critical fields without loading full instructions.
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

            if file_name == "SKILL.md" {
                match fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))
                {
                    Ok(contents) => match crate::skills::manifest::parse_skill_content(&contents) {
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
                    },
                    Err(err) => {
                        if root.scope != SkillScope::System {
                            outcome.errors.push(SkillErrorInfo {
                                path: path.clone(),
                                message: err.to_string(),
                            });
                        }
                    }
                }
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
    /// Built-in VT Code command skill
    BuiltInCommand(Box<BuiltInCommandSkill>),
    /// Native code plugin skill
    NativePlugin(Box<dyn crate::skills::native_plugin::NativePluginTrait>),
}

impl std::fmt::Debug for EnhancedSkill {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Traditional(skill) => f.debug_tuple("Traditional").field(skill).finish(),
            Self::CliTool(tool) => f.debug_tuple("CliTool").field(tool).finish(),
            Self::BuiltInCommand(skill) => f.debug_tuple("BuiltInCommand").field(skill).finish(),
            Self::NativePlugin(plugin) => f.debug_tuple("NativePlugin").field(plugin).finish(),
        }
    }
}

/// High-level loader that provides discovery and validation features
pub struct EnhancedSkillLoader {
    workspace_root: PathBuf,
    codex_home: PathBuf,
    discovery: SkillDiscovery,
    plugin_loader: crate::skills::native_plugin::PluginLoader,
}

fn plugin_loader_for_workspace(
    workspace_root: &Path,
    codex_home: Option<&Path>,
) -> crate::skills::native_plugin::PluginLoader {
    let mut plugin_loader = crate::skills::native_plugin::PluginLoader::new();

    if let Some(codex_home) = codex_home {
        plugin_loader.add_trusted_dir(codex_home.join("plugins"));
    } else {
        plugin_loader.add_trusted_dir(dirs::home_dir().unwrap_or_default().join(".vtcode/plugins"));
    }

    plugin_loader
        .add_trusted_dir(workspace_root.join(".vtcode/plugins"))
        .add_trusted_dir(workspace_root.join(".agents/plugins"));

    plugin_loader
}

fn default_codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
        .unwrap_or_else(|| PathBuf::from(".codex"))
}

fn discovery_config_for_codex_home(workspace_root: &Path, codex_home: &Path) -> DiscoveryConfig {
    let home_dir = dirs::home_dir();
    let loader_config = SkillLoaderConfig {
        codex_home: codex_home.to_path_buf(),
        cwd: workspace_root.to_path_buf(),
        project_root: find_git_root(workspace_root),
        include_bundled_system_skills: true,
    };
    let roots = skill_roots_with_home_dir(&loader_config, home_dir.as_deref());

    DiscoveryConfig {
        skill_paths: roots
            .iter()
            .filter(|root| !root.is_tool_root && !root.is_plugin_root)
            .map(|root| root.path.clone())
            .collect(),
        tool_paths: roots
            .iter()
            .filter(|root| root.is_tool_root)
            .map(|root| root.path.clone())
            .collect(),
        ..Default::default()
    }
}

impl EnhancedSkillLoader {
    /// Create a new enhanced loader for workspace
    pub fn new(workspace_root: PathBuf) -> Self {
        let codex_home = default_codex_home();
        let discovery = SkillDiscovery::with_config(discovery_config_for_codex_home(
            &workspace_root,
            &codex_home,
        ));
        let plugin_loader = plugin_loader_for_workspace(&workspace_root, Some(&codex_home));
        Self {
            workspace_root,
            codex_home,
            discovery,
            plugin_loader,
        }
    }

    /// Create a loader pinned to a specific VT Code home directory.
    pub fn with_codex_home(workspace_root: PathBuf, codex_home: PathBuf) -> Self {
        let discovery = SkillDiscovery::with_config(discovery_config_for_codex_home(
            &workspace_root,
            &codex_home,
        ));
        let plugin_loader = plugin_loader_for_workspace(&workspace_root, Some(&codex_home));
        Self {
            workspace_root,
            codex_home,
            discovery,
            plugin_loader,
        }
    }

    fn ensure_system_skills_installed(&self) {
        if let Err(err) = install_system_skills(&self.codex_home) {
            tracing::warn!("enhanced skill loader failed to install bundled system skills: {err}");
        }
    }

    /// Discover all available skills and tools
    pub async fn discover_all_skills(&mut self) -> Result<DiscoveryResult> {
        self.ensure_system_skills_installed();
        let mut result = self.discovery.discover_all(&self.workspace_root).await?;
        merge_built_in_command_skill_contexts(&mut result.skills);
        Ok(result)
    }

    /// Get a specific skill by name
    pub async fn get_skill(&mut self, name: &str) -> Result<EnhancedSkill> {
        self.ensure_system_skills_installed();
        let result = self.discovery.discover_all(&self.workspace_root).await?;

        // Try traditional skills first
        for skill_ctx in &result.skills {
            if skill_ctx.manifest().name == name {
                let path = skill_ctx.path();
                let (manifest, instructions) = crate::skills::manifest::parse_skill_file(path)?;
                let skill = Skill::with_scope(
                    manifest,
                    path.clone(),
                    infer_scope_from_skill_path(path, &self.workspace_root),
                    instructions,
                )?;
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

        if let Some(skill) = built_in_command_skill(name) {
            return Ok(EnhancedSkill::BuiltInCommand(Box::new(skill)));
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
        Skill::with_scope(
            manifest,
            path.clone(),
            infer_scope_from_skill_path(path, &self.workspace_root),
            instructions,
        )
    }
}

fn infer_scope_from_skill_path(path: &Path, workspace_root: &Path) -> SkillScope {
    if path.starts_with(Path::new("/etc/codex/skills")) {
        return SkillScope::Admin;
    }
    if path.starts_with(system_cache_root_dir(&default_codex_home())) {
        return SkillScope::System;
    }
    if let Some(home) = dirs::home_dir()
        && path.starts_with(home.join(".agents/skills"))
    {
        return SkillScope::User;
    }
    if path.starts_with(workspace_root)
        || path.to_string_lossy().contains("/.agents/skills/")
        || path.to_string_lossy().contains("\\.agents\\skills\\")
    {
        return SkillScope::Repo;
    }
    SkillScope::User
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
/// - Description keywords provide the only implicit signal.
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
        let description_matches = overlap_count(&input_keywords, &description_keywords);
        if description_matches >= min_matches {
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
    use crate::skills::CommandSkillBackend;
    use crate::skills::command_skills::command_skill_specs;
    use crate::skills::system::{install_system_skills, system_cache_root_dir};
    use serial_test::serial;
    use std::fs;
    use tempfile::TempDir;
    use tempfile::tempdir;

    fn manifest(name: &str, description: &str) -> SkillManifest {
        SkillManifest {
            name: name.to_string(),
            description: description.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn detects_explicit_skill_mentions() {
        let skills = vec![manifest(
            "pdf-analyzer",
            "Analyze PDF files and extract tables",
        )];
        let mentions = detect_skill_mentions("Use $pdf-analyzer for this file", &skills);
        assert_eq!(mentions, vec!["pdf-analyzer".to_string()]);
    }

    #[test]
    fn description_keywords_drive_implicit_matches() {
        let skills = vec![manifest(
            "api-fetcher",
            "Fetch data from API endpoints and summarize responses",
        )];

        let mentions = detect_skill_mentions(
            "Fetch and summarize API responses for these endpoints",
            &skills,
        );
        assert_eq!(mentions, vec!["api-fetcher".to_string()]);
    }

    #[test]
    fn unrelated_input_does_not_match_description() {
        let skills = vec![manifest(
            "api-fetcher",
            "Fetch data from API endpoints and summarize responses",
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
        )];
        let options = SkillMentionDetectionOptions {
            enable_auto_trigger: false,
            ..Default::default()
        };
        let mentions = detect_skill_mentions_with_options("Use $sql-checker", &skills, &options);
        assert!(mentions.is_empty());
    }

    #[test]
    #[serial]
    fn lightweight_metadata_discovery_reuses_process_wide_cache() {
        clear_lightweight_skill_metadata_cache();

        let codex_home = tempdir().expect("codex home");
        let workspace = tempdir().expect("workspace");
        let skill_dir = workspace
            .path()
            .join(".agents/skills/process-wide-cache-skill");
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: process-wide-cache-skill\ndescription: process-wide cache test\n---\n# Body\n",
        )
        .expect("write skill");

        let config = SkillLoaderConfig {
            codex_home: codex_home.path().to_path_buf(),
            cwd: workspace.path().to_path_buf(),
            project_root: Some(workspace.path().to_path_buf()),
            include_bundled_system_skills: false,
        };

        let first = discover_skill_metadata_lightweight_hermetic(&config);
        assert!(
            first
                .skills
                .iter()
                .any(|skill| skill.name == "process-wide-cache-skill"),
            "expected first discovery to find test skill",
        );

        fs::remove_dir_all(&skill_dir).expect("remove cached skill dir");

        let second = discover_skill_metadata_lightweight_hermetic(&config);
        assert!(
            second
                .skills
                .iter()
                .any(|skill| skill.name == "process-wide-cache-skill"),
            "expected cached discovery to preserve removed skill until cache is cleared",
        );

        clear_lightweight_skill_metadata_cache();

        let third = discover_skill_metadata_lightweight_hermetic(&config);
        assert!(
            !third
                .skills
                .iter()
                .any(|skill| skill.name == "process-wide-cache-skill"),
            "expected cleared cache to force rediscovery",
        );
    }

    #[tokio::test]
    async fn enhanced_loader_discovers_and_loads_built_in_command_skills() {
        let temp_dir = TempDir::new().expect("temp dir");
        let mut loader = EnhancedSkillLoader::new(temp_dir.path().to_path_buf());

        let discovery = loader.discover_all_skills().await.expect("discover skills");
        assert!(
            discovery
                .skills
                .iter()
                .any(|skill_ctx| skill_ctx.manifest().name == "cmd-status")
        );

        let skill = loader
            .get_skill("cmd-status")
            .await
            .expect("load cmd-status");
        assert!(matches!(skill, EnhancedSkill::BuiltInCommand(_)));
    }

    #[tokio::test]
    async fn enhanced_loader_discovers_and_loads_bundled_command_skills() {
        let workspace = TempDir::new().expect("workspace");
        let codex_home = TempDir::new().expect("codex home");
        install_system_skills(codex_home.path()).expect("install bundled system skills");
        let cmd_review_dir = system_cache_root_dir(codex_home.path()).join("cmd-review");
        assert!(
            cmd_review_dir.join("SKILL.md").exists(),
            "expected bundled cmd-review at {}",
            cmd_review_dir.display()
        );
        let (manifest, _) =
            crate::skills::manifest::parse_skill_file(&cmd_review_dir).expect("parse cmd-review");
        assert_eq!(manifest.name, "cmd-review");
        let config = discovery_config_for_codex_home(workspace.path(), codex_home.path());
        assert!(
            config
                .skill_paths
                .iter()
                .any(|path| path == &system_cache_root_dir(codex_home.path()))
        );
        let mut loader = EnhancedSkillLoader::with_codex_home(
            workspace.path().to_path_buf(),
            codex_home.path().to_path_buf(),
        );

        let discovery = loader.discover_all_skills().await.expect("discover skills");
        assert!(
            discovery
                .skills
                .iter()
                .any(|skill_ctx| skill_ctx.manifest().name == "cmd-review")
        );

        let skill = loader
            .get_skill("cmd-review")
            .await
            .expect("load cmd-review");
        assert!(matches!(skill, EnhancedSkill::Traditional(_)));
    }

    #[tokio::test]
    async fn enhanced_loader_discovers_every_command_skill() {
        let workspace = TempDir::new().expect("workspace");
        let codex_home = TempDir::new().expect("codex home");
        let mut loader = EnhancedSkillLoader::with_codex_home(
            workspace.path().to_path_buf(),
            codex_home.path().to_path_buf(),
        );

        let discovery = loader.discover_all_skills().await.expect("discover skills");
        let discovered_names = discovery
            .skills
            .iter()
            .map(|skill_ctx| skill_ctx.manifest().name.as_str())
            .collect::<std::collections::HashSet<_>>();

        for spec in command_skill_specs() {
            assert!(
                discovered_names.contains(spec.skill_name),
                "missing command skill {}",
                spec.skill_name
            );

            let skill = loader
                .get_skill(spec.skill_name)
                .await
                .unwrap_or_else(|error| panic!("failed to load {}: {error}", spec.skill_name));

            match spec.backend {
                CommandSkillBackend::TraditionalSkill { .. } => {
                    assert!(
                        matches!(skill, EnhancedSkill::Traditional(_)),
                        "{} should load as a traditional skill",
                        spec.skill_name
                    );
                }
                CommandSkillBackend::BuiltInCommand { .. } => {
                    assert!(
                        matches!(skill, EnhancedSkill::BuiltInCommand(_)),
                        "{} should load as a built-in command skill",
                        spec.skill_name
                    );
                }
            }
        }
    }

    fn write_skill(dir: &Path, name: &str, description: &str) {
        fs::create_dir_all(dir).expect("create skill dir");
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n\nUse this skill.\n"),
        )
        .expect("write SKILL.md");
    }

    fn write_codex_skill_config(home_dir: &Path, contents: &str) {
        let config_dir = home_dir.join(".codex");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(config_dir.join("config.toml"), contents).expect("write config");
    }

    fn skill_loader_config_for(workspace: &Path, codex_home: &Path) -> SkillLoaderConfig {
        SkillLoaderConfig {
            codex_home: codex_home.to_path_buf(),
            cwd: workspace.to_path_buf(),
            project_root: find_git_root(workspace),
            include_bundled_system_skills: false,
        }
    }

    #[test]
    fn disabled_skill_config_supports_stable_names() {
        let workspace = tempdir().expect("workspace");
        fs::create_dir(workspace.path().join(".git")).expect("create .git");

        let home = tempdir().expect("home");
        let codex_home = tempdir().expect("codex home");

        let old_plugin_skill_dir = workspace
            .path()
            .join(".agents/plugins/example-plugin-v1/skills/release-helper");
        write_skill(
            &old_plugin_skill_dir,
            "release-helper",
            "Prepare release notes",
        );

        write_codex_skill_config(
            home.path(),
            &format!(
                "[[skills.config]]\nname = \"release-helper\"\npath = \"{}\"\nenabled = false\n",
                old_plugin_skill_dir.display()
            ),
        );

        let new_plugin_skill_dir = workspace
            .path()
            .join(".agents/plugins/example-plugin-v2/skills/release-helper");
        write_skill(
            &new_plugin_skill_dir,
            "release-helper",
            "Prepare release notes",
        );

        fs::remove_dir_all(workspace.path().join(".agents/plugins/example-plugin-v1"))
            .expect("remove old plugin version");

        let outcome = load_skills_with_home_dir(
            &skill_loader_config_for(workspace.path(), codex_home.path()),
            Some(home.path()),
        );

        assert!(
            outcome
                .skills
                .iter()
                .all(|skill| skill.name != "release-helper"),
            "expected release-helper to stay disabled after plugin path changed"
        );
    }

    #[test]
    fn disabled_skill_config_preserves_path_based_entries() {
        let workspace = tempdir().expect("workspace");
        let home = tempdir().expect("home");
        let codex_home = tempdir().expect("codex home");

        let skill_dir = home.path().join(".agents/skills/path-disabled");
        write_skill(&skill_dir, "path-disabled", "Disabled by explicit path");

        write_codex_skill_config(
            home.path(),
            &format!(
                "[[skills.config]]\npath = \"{}\"\nenabled = false\n",
                skill_dir.join("SKILL.md").display()
            ),
        );

        let outcome = load_skills_with_home_dir(
            &skill_loader_config_for(workspace.path(), codex_home.path()),
            Some(home.path()),
        );

        assert!(
            outcome
                .skills
                .iter()
                .all(|skill| skill.name != "path-disabled"),
            "expected path-disabled to remain filtered by legacy path config"
        );
    }
}
