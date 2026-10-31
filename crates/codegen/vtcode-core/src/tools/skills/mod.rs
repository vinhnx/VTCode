use crate::config::ConfigManager;
use crate::config::ToolDocumentationMode;
use crate::config::types::CapabilityLevel;
use crate::llm::provider::ToolDefinition;
use crate::metrics::MetricsCollector;
use crate::skills::cli_bridge::CliToolConfig;
use crate::skills::command_skills::merge_built_in_command_skill_metadata;
use crate::skills::discovery::{DiscoveryConfig, SkillDiscovery};
use crate::skills::executor::{ForkSkillExecutor, SkillToolAdapter};
use crate::skills::file_references::FileReferenceValidator;
use crate::skills::loader::{EnhancedSkill, EnhancedSkillLoader, SkillLoaderConfig};
use crate::skills::manager::SkillsManager;
use crate::skills::model::{SkillErrorInfo, SkillLoadOutcome};
use crate::skills::types::{Skill, SkillManifestMetadata, SkillVariety};
use crate::tool_policy::ToolPolicy;
use crate::tools::handlers::{
    DeferredToolPolicy, SessionSurface, SessionToolsConfig, ToolModelCapabilities, ToolProfile,
};
use crate::tools::registry::{ToolMetadata, ToolRegistration, ToolRegistry, native_cgp_tool_factory};
use crate::tools::traits::Tool;
use crate::utils::file_utils::{read_file_with_context, read_file_with_context_sync};
use anyhow::Context;
use async_trait::async_trait;
use hashbrown::{HashMap, HashSet};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

#[cfg(test)]
use crate::tools::CgpRuntimeMode;
use crate::tools::error_messages::skill_ops;
use vtcode_commons::{VtCodePaths, canonicalize};

type SkillMap = Arc<RwLock<HashMap<String, Skill>>>;
type ToolDefList = Arc<RwLock<Vec<ToolDefinition>>>;
type ToolChangeNotifier = Arc<dyn Fn(&'static str) + Send + Sync>;

const SKILL_TOOL_PROMPT_PATH: &str = "skills/skill_instructions.md";
const SKILL_ACTIVATED_STATUS: &str = "Associated tools activated and added to context.";
const SKILL_ALREADY_ACTIVE_STATUS: &str = "Associated tools were already active.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillActivationState {
    Activated,
    AlreadyActive,
}

#[derive(Clone)]
pub struct SkillToolSessionRuntime {
    tool_registry: Arc<ToolRegistry>,
    active_tools: Option<ToolDefList>,
    tool_documentation_mode: ToolDocumentationMode,
    model_capabilities: ToolModelCapabilities,
    deferred_tool_policy: DeferredToolPolicy,
    anthropic_native_memory_enabled: bool,
    tool_profile: ToolProfile,
    on_tools_changed: Option<ToolChangeNotifier>,
    fork_executor: Option<Arc<dyn ForkSkillExecutor>>,
}

impl SkillToolSessionRuntime {
    pub fn new(
        tool_registry: Arc<ToolRegistry>,
        active_tools: Option<ToolDefList>,
        tool_documentation_mode: ToolDocumentationMode,
        model_capabilities: ToolModelCapabilities,
        on_tools_changed: Option<ToolChangeNotifier>,
    ) -> Self {
        Self {
            tool_registry,
            active_tools,
            tool_documentation_mode,
            model_capabilities,
            deferred_tool_policy: DeferredToolPolicy::default(),
            anthropic_native_memory_enabled: false,
            tool_profile: ToolProfile::default(),
            on_tools_changed,
            fork_executor: None,
        }
    }

    pub fn with_fork_executor(mut self, fork_executor: Arc<dyn ForkSkillExecutor>) -> Self {
        self.fork_executor = Some(fork_executor);
        self
    }

    pub fn with_deferred_tool_policy(mut self, deferred_tool_policy: DeferredToolPolicy) -> Self {
        self.deferred_tool_policy = deferred_tool_policy;
        self
    }

    pub fn with_anthropic_native_memory_enabled(mut self, enabled: bool) -> Self {
        self.anthropic_native_memory_enabled = enabled;
        self
    }

    pub fn with_tool_profile(mut self, tool_profile: ToolProfile) -> Self {
        self.tool_profile = tool_profile;
        self
    }

    pub async fn activate_skill(
        &self,
        active_skills: &Arc<RwLock<HashMap<String, Skill>>>,
        skill: Skill,
    ) -> anyhow::Result<SkillActivationState> {
        let skill_name = skill.name().to_string();
        if active_skills.read().await.contains_key(skill_name.as_str()) {
            return Ok(SkillActivationState::AlreadyActive);
        }

        if !self.tool_registry.has_tool(skill_name.as_str()).await {
            self.tool_registry
                .register_tool(build_traditional_skill_tool_registration_with_metrics(
                    &skill,
                    self.fork_executor.clone(),
                    Some(self.tool_registry.metrics_collector()),
                ))
                .await
                .with_context(|| format!("failed to register skill tool '{skill_name}'"))?;
            self.refresh_tool_snapshot("load_skill").await;
        }

        self.tool_registry
            .metrics_collector()
            .record_skill_created(skill_name.clone(), skill.variety.as_str().to_string());
        active_skills.write().await.insert(skill_name, skill);
        Ok(SkillActivationState::Activated)
    }

    pub async fn deactivate_skill(
        &self,
        active_skills: &Arc<RwLock<HashMap<String, Skill>>>,
        skill_name: &str,
    ) -> anyhow::Result<bool> {
        let removed = active_skills.write().await.remove(skill_name).is_some();
        let unregistered = self.tool_registry.unregister_tool(skill_name).await?;
        if unregistered {
            self.refresh_tool_snapshot("unload_skill").await;
        }
        let settled = removed || unregistered;
        if settled {
            self.tool_registry
                .metrics_collector()
                .record_skill_deleted(skill_name.to_string());
        }
        Ok(settled)
    }

    async fn refresh_tool_snapshot(&self, reason: &'static str) {
        if let Some(active_tools) = &self.active_tools {
            let refreshed = self
                .tool_registry
                .model_tools(
                    SessionToolsConfig::full_public(
                        SessionSurface::Interactive,
                        CapabilityLevel::CodeSearch,
                        self.tool_documentation_mode,
                        self.model_capabilities,
                    )
                    .with_planning_active(true)
                    .with_deferred_tool_policy(self.deferred_tool_policy.clone())
                    .with_anthropic_native_memory_enabled(self.anthropic_native_memory_enabled)
                    .with_tool_profile(self.tool_profile),
                )
                .await;
            *active_tools.write().await = refreshed;
        }

        if let Some(notifier) = &self.on_tools_changed {
            notifier(reason);
        }
    }
}

fn build_skill_tool_adapter(
    skill: Skill,
    fork_executor: Option<Arc<dyn ForkSkillExecutor>>,
    metrics: Option<Arc<MetricsCollector>>,
) -> SkillToolAdapter {
    let adapter = if skill.manifest.context.as_deref() == Some("fork") {
        match fork_executor {
            Some(executor) => SkillToolAdapter::with_fork_executor(skill, executor),
            None => SkillToolAdapter::new(skill),
        }
    } else {
        SkillToolAdapter::new(skill)
    };
    match metrics {
        Some(collector) => adapter.with_metrics(collector),
        None => adapter,
    }
}

pub fn build_traditional_skill_tool_registration(
    skill: &Skill,
    fork_executor: Option<Arc<dyn ForkSkillExecutor>>,
) -> ToolRegistration {
    build_traditional_skill_tool_registration_with_metrics(skill, fork_executor, None)
}

/// Builds a traditional skill tool registration, attaching `metrics` to the
/// adapter (and its native factory rebuilds) so executions feed the skill
/// health report. `None` runs executions unrecorded.
pub fn build_traditional_skill_tool_registration_with_metrics(
    skill: &Skill,
    fork_executor: Option<Arc<dyn ForkSkillExecutor>>,
    metrics: Option<Arc<MetricsCollector>>,
) -> ToolRegistration {
    let metadata = ToolMetadata::default()
        .with_description(skill.description())
        .with_parameter_schema(skill_tool_parameter_schema())
        .with_permission(ToolPolicy::Prompt)
        .with_prompt_path(SKILL_TOOL_PROMPT_PATH);

    // Traditional skills already flow through shared fork executors, so keep
    // the trait-object bridge here and let the native CGP factory handle the
    // ownership-first path when runtime mode is known.
    let adapter: Arc<dyn Tool> =
        Arc::new(build_skill_tool_adapter(skill.clone(), fork_executor.clone(), metrics.clone()));
    let native_skill = skill.clone();
    let native_fork_executor = fork_executor;

    ToolRegistration::from_tool_with_metadata(skill.name().to_string(), CapabilityLevel::Basic, adapter, metadata)
        .with_native_cgp_factory(native_cgp_tool_factory(move || {
            build_skill_tool_adapter(native_skill.clone(), native_fork_executor.clone(), metrics.clone())
        }))
}

pub fn build_skill_tool_registration(skill: &Skill) -> ToolRegistration {
    build_traditional_skill_tool_registration(skill, None)
}

fn skill_tool_parameter_schema() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "description": "Flexible input for skill execution",
        "additionalProperties": true,
    })
}

fn load_skill_instructions(skill: &Skill, activation_status: &str) -> String {
    if !skill.instructions.is_empty() {
        return skill.instructions.clone();
    }

    let skill_file = skill.path.join("SKILL.md");
    if skill_file.exists() {
        return match read_file_with_context_sync(&skill_file, "skill file") {
            Ok(content) => content,
            Err(error) => format!("Error reading skill file: {error}"),
        };
    }

    format!("No detailed instructions available for {}. {}", skill.name(), activation_status)
}

fn build_skill_response(skill: &Skill, activation_status: &str) -> Value {
    let instructions = load_skill_instructions(skill, activation_status);
    let validator = FileReferenceValidator::new(skill.path.clone());
    let resources: Vec<String> = validator
        .list_valid_references()
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect();

    json!({
        "name": skill.name(),
        "variety": skill.variety,
        "instructions": instructions,
        "instructions_status": "These instructions are now [ACTIVE] and will persist in your system prompt for the remainder of this session.",
        "activation_status": activation_status,
        "resources": resources,
        "path": skill.path,
        "description": skill.description()
    })
}

fn default_vtcode_home_dir() -> anyhow::Result<PathBuf> {
    VtCodePaths::resolve()
        .map(|paths| paths.data_dir().to_path_buf())
        .context("could not resolve VT Code data directory for skill discovery")
}

fn effective_codex_home(explicit_home: Option<&Path>) -> anyhow::Result<PathBuf> {
    explicit_home.map(Path::to_path_buf).map_or_else(default_vtcode_home_dir, Ok)
}

fn find_project_root(path: &Path) -> Option<PathBuf> {
    let mut current = Some(path);
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

fn build_skill_loader_config(
    workspace_root: &Path,
    codex_home: &Path,
    include_bundled_system_skills: bool,
) -> SkillLoaderConfig {
    SkillLoaderConfig {
        codex_home: codex_home.to_path_buf(),
        cwd: workspace_root.to_path_buf(),
        project_root: find_project_root(workspace_root).or_else(|| Some(workspace_root.to_path_buf())),
        include_bundled_system_skills,
    }
}

fn discover_session_skill_metadata(workspace_root: &Path, codex_home: &Path) -> SkillLoadOutcome {
    let bundled_skills_enabled = ConfigManager::load_from_workspace(workspace_root)
        .map(|manager| manager.config().skills.bundled.enabled)
        .unwrap_or(true);
    let manager = SkillsManager::new_with_bundled_skills_enabled(codex_home.to_path_buf(), bundled_skills_enabled);
    manager.ensure_system_skills_installed();
    let config = build_skill_loader_config(workspace_root, codex_home, bundled_skills_enabled);

    #[cfg(test)]
    let mut discovery = crate::skills::loader::discover_skill_metadata_lightweight_hermetic(&config);

    #[cfg(not(test))]
    let mut discovery = crate::skills::loader::discover_skill_metadata_lightweight(&config);

    merge_built_in_command_skill_metadata(&mut discovery.skills);
    discovery
}

async fn discover_session_utilities(workspace_root: &Path, codex_home: &Path) -> anyhow::Result<Vec<CliToolConfig>> {
    let mut config = DiscoveryConfig::default();
    config.skill_paths.clear();
    config.tool_paths = vec![
        PathBuf::from("./tools"),
        PathBuf::from("./vendor/tools"),
        codex_home.join("tools"),
    ];
    if let Ok(paths) = VtCodePaths::resolve() {
        config.tool_paths.push(paths.data_path("tools")?);
        config.tool_paths.push(paths.legacy_dir().join("tools"));
    }

    let mut discovery = SkillDiscovery::with_config(config);
    Ok(discovery.discover_all(workspace_root).await?.tools)
}

fn discovery_error_samples(errors: &[SkillErrorInfo]) -> Vec<String> {
    errors
        .iter()
        .take(3)
        .map(|error| format!("{}: {}", error.path.display(), error.message))
        .collect()
}

fn log_discovery_warnings(operation: &'static str, errors: &[SkillErrorInfo]) {
    if errors.is_empty() {
        return;
    }

    warn!(
        operation,
        error_count = errors.len(),
        sample = ?discovery_error_samples(errors),
        "Session skill discovery reported warnings"
    );
}

fn discover_skill_catalog(
    workspace_root: &Path,
    explicit_codex_home: Option<&Path>,
    operation: &'static str,
) -> anyhow::Result<(PathBuf, SkillLoadOutcome)> {
    let codex_home = effective_codex_home(explicit_codex_home)?;
    debug!(
        operation,
        workspace = %workspace_root.display(),
        codex_home = %codex_home.display(),
        "Running session skill discovery"
    );

    let metadata = discover_session_skill_metadata(workspace_root, &codex_home);
    log_discovery_warnings(operation, metadata.errors.as_slice());
    Ok((codex_home, metadata))
}

fn required_string_arg<'a>(args: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Missing '{key}' argument"))
}

fn unsupported_activation_error(skill_name: &str, skill: EnhancedSkill) -> anyhow::Error {
    let message = match skill {
        EnhancedSkill::CliTool(_) => {
            format!("Skill '{skill_name}' is a system utility and cannot be activated via load_skill")
        }
        EnhancedSkill::BuiltInCommand(_) => {
            format!(
                "Skill '{skill_name}' is a built-in command skill and cannot be activated via load_skill; use /skills use {skill_name} instead"
            )
        }
        EnhancedSkill::NativePlugin(_) => {
            format!("Skill '{skill_name}' is a native plugin and cannot be activated via load_skill")
        }
        EnhancedSkill::Traditional(_) => {
            format!("Skill '{skill_name}' is already a traditional skill")
        }
    };

    anyhow::anyhow!(message)
}

fn resolve_skill_resource_path(skill_root: &Path, resource_path: &str) -> anyhow::Result<PathBuf> {
    let relative_path = Path::new(resource_path);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::RootDir | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(anyhow::anyhow!("Resource path '{resource_path}' must be relative to the skill directory"));
    }

    let full_path = skill_root.join(relative_path);
    let canonical_root =
        canonicalize(skill_root).with_context(|| format!("Failed to resolve skill root {}", skill_root.display()))?;
    let canonical_path = canonicalize(full_path).with_context(|| format!("Resource '{resource_path}' not found"))?;

    if !canonical_path.starts_with(&canonical_root) {
        return Err(anyhow::anyhow!("Resource '{resource_path}' escapes the skill directory"));
    }

    if !canonical_path.is_file() {
        return Err(anyhow::anyhow!("Resource '{resource_path}' is not a readable file"));
    }

    Ok(canonical_path)
}

fn extract_metadata_keywords(metadata: Option<&SkillManifestMetadata>) -> Vec<String> {
    let Some(meta) = metadata else {
        return Vec::new();
    };
    let Some(keywords_value) = meta.get("keywords") else {
        return Vec::new();
    };
    let Some(keywords_array) = keywords_value.as_array() else {
        return Vec::new();
    };
    keywords_array.iter().filter_map(|v| v.as_str().map(String::from)).collect()
}

fn matches_keywords(keywords: &[String], query_lower: &str) -> bool {
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    if query_words.is_empty() || keywords.is_empty() {
        return false;
    }
    query_words.iter().all(|word| {
        let normalized_word = word.replace('-', " ");
        keywords
            .iter()
            .any(|kw| kw.replace('-', " ").to_lowercase().contains(normalized_word.as_str()))
    })
}

fn matches_skill_filters(
    name: &str,
    description: &str,
    variety: SkillVariety,
    query: Option<&str>,
    variety_filter: Option<&str>,
    keywords: &[String],
) -> bool {
    let normalized_variety = format!("{variety:?}").to_lowercase();
    if let Some(filter) = variety_filter
        && !normalized_variety.contains(&filter.replace('_', "").to_lowercase())
    {
        return false;
    }

    if let Some(query) = query {
        let query = query.to_lowercase();
        if !name.to_lowercase().contains(query.as_str())
            && !description.to_lowercase().contains(query.as_str())
            && !matches_keywords(keywords, &query)
        {
            return false;
        }
    }

    true
}

/// Tool to load skill instructions on demand (Progressive Disclosure)
pub struct LoadSkillTool {
    workspace_root: PathBuf,
    codex_home: Option<PathBuf>,
    active_skills: SkillMap,
    runtime: SkillToolSessionRuntime,
}

impl LoadSkillTool {
    pub fn new(workspace_root: PathBuf, active_skills: SkillMap, runtime: SkillToolSessionRuntime) -> Self {
        Self::with_codex_home(workspace_root, active_skills, runtime, None)
    }

    pub fn with_codex_home(
        workspace_root: PathBuf,
        active_skills: SkillMap,
        runtime: SkillToolSessionRuntime,
        codex_home: Option<PathBuf>,
    ) -> Self {
        Self { workspace_root, codex_home, active_skills, runtime }
    }
}

#[async_trait]
impl Tool for LoadSkillTool {
    fn name(&self) -> &str {
        "load_skill"
    }

    fn description(&self) -> &str {
        "Load detailed instructions for a specific traditional skill and activate its associated tool into your environment. This operation requires approval because skill content may be executable or native-backed. Use list_skills first to see what is available. Skills that list_skills reports as active are already loaded, so load_skill is only needed for inactive ones; calling it for an active skill returns the cached instructions without reloading. Returns the skill instructions and activation status."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The name of the skill to load"
                }
            },
            "required": ["name"]
        }))
    }

    fn default_permission(&self) -> ToolPolicy {
        // A skill may be backed by executable content or a native plugin. The
        // approval gate must run before discovery can reach any such loader.
        ToolPolicy::Prompt
    }

    fn is_mutating(&self) -> bool {
        // Activation changes the session tool set and may cross into
        // executable-backed skill implementations.
        true
    }

    fn is_parallel_safe(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let name = required_string_arg(&args, "name")?;

        if let Some(skill) = self.active_skills.read().await.get(name).cloned() {
            return Ok(build_skill_response(&skill, SKILL_ALREADY_ACTIVE_STATUS));
        }

        let (codex_home, metadata) =
            discover_skill_catalog(&self.workspace_root, self.codex_home.as_deref(), "load_skill")?;

        let mut loader = EnhancedSkillLoader::with_codex_home(self.workspace_root.clone(), codex_home.clone());
        let skill = match loader.get_skill(name).await {
            Ok(EnhancedSkill::Traditional(skill)) => *skill,
            Ok(skill) => return Err(unsupported_activation_error(name, skill)),
            Err(error) => {
                let tools = discover_session_utilities(&self.workspace_root, &codex_home).await?;
                if tools.iter().any(|tool| tool.name == name) {
                    return Err(anyhow::anyhow!(
                        "Skill '{name}' is a system utility and cannot be activated via load_skill"
                    ));
                }

                let detail = if metadata.errors.is_empty() {
                    String::new()
                } else {
                    format!(
                        " Session discovery also reported {} issue(s); use `list_skills` to inspect warning samples.",
                        metadata.errors.len()
                    )
                };

                return Err(anyhow::anyhow!("Failed to load skill '{name}': {error}.{detail}"));
            }
        };

        let activation_status = match self.runtime.activate_skill(&self.active_skills, skill.clone()).await? {
            SkillActivationState::Activated => SKILL_ACTIVATED_STATUS,
            SkillActivationState::AlreadyActive => SKILL_ALREADY_ACTIVE_STATUS,
        };

        Ok(build_skill_response(&skill, activation_status))
    }
}

/// Tool to list all available skills
pub struct ListSkillsTool {
    workspace_root: PathBuf,
    codex_home: Option<PathBuf>,
    active_skills: SkillMap,
}

impl ListSkillsTool {
    pub fn new(workspace_root: PathBuf, active_skills: SkillMap) -> Self {
        Self::with_codex_home(workspace_root, active_skills, None)
    }

    pub fn with_codex_home(workspace_root: PathBuf, active_skills: SkillMap, codex_home: Option<PathBuf>) -> Self {
        Self { workspace_root, codex_home, active_skills }
    }
}

#[async_trait]
impl Tool for ListSkillsTool {
    fn name(&self) -> &str {
        "list_skills"
    }

    fn description(&self) -> &str {
        "List all available skills and system utilities. Use 'query' to filter by name, description, or routing hints, or 'variety' to filter by type ('agent_skill' or 'system_utility'). Traditional skills stay inactive until activated via 'load_skill'. Returns matching skills with names, descriptions, and types."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional search term to filter skills by name, description, or routing hints (case-insensitive)"
                },
                "variety": {
                    "type": "string",
                    "enum": ["agent_skill", "system_utility", "built_in"],
                    "description": "Optional variety to filter by"
                }
            },
            "additionalProperties": false
        }))
    }

    fn default_permission(&self) -> ToolPolicy {
        ToolPolicy::Allow
    }

    fn is_mutating(&self) -> bool {
        false
    }

    fn is_parallel_safe(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let query = args.get("query").and_then(|v| v.as_str()).map(|s| s.to_lowercase());
        let variety_filter = args.get("variety").and_then(|v| v.as_str());

        let active_names: HashSet<String> = self.active_skills.read().await.keys().cloned().collect();
        let (codex_home, discovery) =
            discover_skill_catalog(&self.workspace_root, self.codex_home.as_deref(), "list_skills")?;

        let mut skill_list = Vec::new();

        for skill_meta in discovery.skills.iter().filter(|skill| skill.manifest.is_some()) {
            let manifest = skill_meta.manifest.as_ref().expect("filtered to skills with manifests");
            let keywords = extract_metadata_keywords(manifest.metadata.as_ref());
            if !matches_skill_filters(
                manifest.name.as_str(),
                manifest.description.as_str(),
                manifest.variety,
                query.as_deref(),
                variety_filter,
                &keywords,
            ) {
                continue;
            }

            let status = if active_names.contains(manifest.name.as_str()) {
                "active"
            } else {
                "dormant"
            };

            skill_list.push(json!({
                "name": manifest.name,
                "description": manifest.description,
                "path": skill_meta.path,
                "scope": skill_meta.scope,
                "variety": manifest.variety,
                "status": status,
            }));
        }

        for tool in discover_session_utilities(&self.workspace_root, &codex_home).await? {
            if !matches_skill_filters(
                tool.name.as_str(),
                tool.description.as_str(),
                SkillVariety::SystemUtility,
                query.as_deref(),
                variety_filter,
                &[],
            ) {
                continue;
            }

            skill_list.push(json!({
                "name": tool.name,
                "description": tool.description,
                "variety": SkillVariety::SystemUtility,
                "status": "dormant",
            }));
        }

        // Sort by name for stable output
        skill_list.sort_by(|a, b| {
            let na = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let nb = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            na.cmp(nb)
        });

        // Group by variety for "better" discovery
        let mut grouped = HashMap::with_capacity(skill_list.len());
        for skill in &skill_list {
            let variety = skill.get("variety").and_then(|v| v.as_str()).unwrap_or("unknown");
            grouped.entry(variety.to_string()).or_insert_with(Vec::new).push(skill.clone());
        }

        let mut response = serde_json::json!({
            "count": skill_list.len(),
            "groups": grouped,
        });

        // Add context message for queries
        if (query.is_some() || variety_filter.is_some())
            && let Some(response_object) = response.as_object_mut()
        {
            response_object.insert("filter_applied".to_string(), serde_json::json!(true));
        }

        if !discovery.errors.is_empty()
            && let Some(response_object) = response.as_object_mut()
        {
            response_object.insert("discovery_errors".to_string(), serde_json::json!(discovery.errors.len()));
            response_object.insert(
                "discovery_error_samples".to_string(),
                serde_json::json!(discovery_error_samples(discovery.errors.as_slice())),
            );
        }

        Ok(response)
    }
}

/// Tool to load a specific resource from a skill (Level 3)
pub struct LoadSkillResourceTool {
    skills: SkillMap,
}

impl LoadSkillResourceTool {
    pub fn new(skills: SkillMap) -> Self {
        Self { skills }
    }
}

#[async_trait]
impl Tool for LoadSkillResourceTool {
    fn name(&self) -> &str {
        "load_skill_resource"
    }

    fn description(&self) -> &str {
        "Read a resource file (script, template, or doc) from an active skill's directory. The skill must already be loaded via load_skill, which is also the tool that returns skill instructions. Provide skill_name and resource_path relative to the skill root. Returns the raw file content."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "skill_name": {
                    "type": "string",
                    "description": "The name of the skill"
                },
                "resource_path": {
                    "type": "string",
                    "description": "The relative path of the resource (e.g. 'scripts/helper.py')"
                }
            },
            "required": ["skill_name", "resource_path"]
        }))
    }

    fn default_permission(&self) -> ToolPolicy {
        ToolPolicy::Allow
    }

    fn is_mutating(&self) -> bool {
        false
    }

    fn is_parallel_safe(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let skill_name = required_string_arg(&args, "skill_name")?;
        let resource_path = required_string_arg(&args, "resource_path")?;

        let skills = self.skills.read().await;
        if skills.is_empty() {
            return Err(anyhow::anyhow!(
                "No skills are active in this session yet. Use `load_skill` (or `/skills load <name>`) first."
            ));
        }
        if let Some(skill) = skills.get(skill_name) {
            let full_path = resolve_skill_resource_path(&skill.path, resource_path)?;
            let content = read_file_with_context(&full_path, "skill resource")
                .await
                .context(format!("Failed to read resource at {}", full_path.display()))?;

            Ok(serde_json::json!({
                "skill_name": skill_name,
                "resource_path": resource_path,
                "content": content
            }))
        } else {
            Err(skill_ops::skill_not_found_error(skill_name))
        }
    }
}

#[cfg(test)]
mod tests;
