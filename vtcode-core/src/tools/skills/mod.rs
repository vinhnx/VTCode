use crate::config::ConfigManager;
use crate::config::ToolDocumentationMode;
use crate::config::types::CapabilityLevel;
use crate::llm::provider::ToolDefinition;
use crate::skills::cli_bridge::CliToolConfig;
use crate::skills::discovery::{DiscoveryConfig, SkillDiscovery};
use crate::skills::executor::{ForkSkillExecutor, SkillToolAdapter};
use crate::skills::file_references::FileReferenceValidator;
use crate::skills::loader::{
    EnhancedSkill, EnhancedSkillLoader, SkillLoaderConfig, discover_skill_metadata_lightweight,
};
use crate::skills::manager::SkillsManager;
use crate::skills::model::{SkillErrorInfo, SkillLoadOutcome};
use crate::skills::types::{Skill, SkillVariety};
use crate::tool_policy::ToolPolicy;
use crate::tools::handlers::{SessionSurface, SessionToolsConfig, ToolModelCapabilities};
use crate::tools::registry::{
    ToolMetadata, ToolRegistration, ToolRegistry, native_cgp_tool_factory,
};
use crate::tools::traits::Tool;
use crate::utils::file_utils::read_file_with_context_sync;
use anyhow::Context;
use async_trait::async_trait;
use hashbrown::HashMap;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

#[cfg(test)]
use crate::tools::CgpRuntimeMode;

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
            on_tools_changed,
            fork_executor: None,
        }
    }

    pub fn with_fork_executor(mut self, fork_executor: Arc<dyn ForkSkillExecutor>) -> Self {
        self.fork_executor = Some(fork_executor);
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
                .register_tool(build_traditional_skill_tool_registration(
                    &skill,
                    self.fork_executor.clone(),
                ))
                .await
                .with_context(|| format!("failed to register skill tool '{skill_name}'"))?;
            self.refresh_tool_snapshot("load_skill").await;
        }

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
        Ok(removed || unregistered)
    }

    async fn refresh_tool_snapshot(&self, reason: &'static str) {
        if let Some(active_tools) = &self.active_tools {
            let refreshed = self
                .tool_registry
                .model_tools(SessionToolsConfig::full_public(
                    SessionSurface::Interactive,
                    CapabilityLevel::CodeSearch,
                    self.tool_documentation_mode,
                    self.model_capabilities,
                ))
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
) -> SkillToolAdapter {
    if skill.manifest.context.as_deref() == Some("fork") {
        match fork_executor {
            Some(executor) => SkillToolAdapter::with_fork_executor(skill, executor),
            None => SkillToolAdapter::new(skill),
        }
    } else {
        SkillToolAdapter::new(skill)
    }
}

pub fn build_traditional_skill_tool_registration(
    skill: &Skill,
    fork_executor: Option<Arc<dyn ForkSkillExecutor>>,
) -> ToolRegistration {
    let metadata = ToolMetadata::default()
        .with_description(skill.description())
        .with_parameter_schema(skill_tool_parameter_schema())
        .with_permission(ToolPolicy::Prompt)
        .with_prompt_path(SKILL_TOOL_PROMPT_PATH);

    let adapter: Arc<dyn Tool> = Arc::new(build_skill_tool_adapter(
        skill.clone(),
        fork_executor.clone(),
    ));
    let native_skill = skill.clone();
    let native_fork_executor = fork_executor;

    ToolRegistration::from_tool_with_metadata(
        skill.name().to_string(),
        CapabilityLevel::Basic,
        adapter,
        metadata,
    )
    .with_native_cgp_factory(native_cgp_tool_factory(move || {
        build_skill_tool_adapter(native_skill.clone(), native_fork_executor.clone())
    }))
}

pub fn build_skill_tool_registration(skill: &Skill) -> ToolRegistration {
    build_traditional_skill_tool_registration(skill, None)
}

fn skill_tool_parameter_schema() -> Value {
    json!({
        "type": "object",
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

    format!(
        "No detailed instructions available for {}. {}",
        skill.name(),
        activation_status
    )
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

fn default_vtcode_home_dir() -> PathBuf {
    std::env::var_os("VTCODE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".vtcode")))
        .unwrap_or_else(|| PathBuf::from(".vtcode"))
}

fn effective_codex_home(explicit_home: Option<&Path>) -> PathBuf {
    explicit_home
        .map(Path::to_path_buf)
        .unwrap_or_else(default_vtcode_home_dir)
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
        project_root: find_project_root(workspace_root)
            .or_else(|| Some(workspace_root.to_path_buf())),
        include_bundled_system_skills,
    }
}

fn discover_session_skill_metadata(workspace_root: &Path, codex_home: &Path) -> SkillLoadOutcome {
    let bundled_skills_enabled = ConfigManager::load_from_workspace(workspace_root)
        .map(|manager| manager.config().skills.bundled.enabled)
        .unwrap_or(true);
    let manager = SkillsManager::new_with_bundled_skills_enabled(
        codex_home.to_path_buf(),
        bundled_skills_enabled,
    );
    manager.ensure_system_skills_installed();
    let config = build_skill_loader_config(workspace_root, codex_home, bundled_skills_enabled);
    discover_skill_metadata_lightweight(&config)
}

async fn discover_session_utilities(
    workspace_root: &Path,
    codex_home: &Path,
) -> anyhow::Result<Vec<CliToolConfig>> {
    let mut config = DiscoveryConfig::default();
    config.skill_paths.clear();
    config.tool_paths = vec![
        PathBuf::from("./tools"),
        PathBuf::from("./vendor/tools"),
        codex_home.join("tools"),
    ];

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

fn matches_skill_filters(
    name: &str,
    description: &str,
    when_to_use: Option<&str>,
    when_not_to_use: Option<&str>,
    variety: SkillVariety,
    query: Option<&str>,
    variety_filter: Option<&str>,
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
            && !when_to_use.is_some_and(|value| value.to_lowercase().contains(query.as_str()))
            && !when_not_to_use.is_some_and(|value| value.to_lowercase().contains(query.as_str()))
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
    pub fn new(
        workspace_root: PathBuf,
        active_skills: SkillMap,
        runtime: SkillToolSessionRuntime,
    ) -> Self {
        Self::with_codex_home(workspace_root, active_skills, runtime, None)
    }

    pub fn with_codex_home(
        workspace_root: PathBuf,
        active_skills: SkillMap,
        runtime: SkillToolSessionRuntime,
        codex_home: Option<PathBuf>,
    ) -> Self {
        Self {
            workspace_root,
            codex_home,
            active_skills,
            runtime,
        }
    }
}

#[async_trait]
impl Tool for LoadSkillTool {
    fn name(&self) -> &'static str {
        "load_skill"
    }

    fn description(&self) -> &'static str {
        "Load detailed instructions for a specific traditional skill and activate its associated tool into your environment."
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
        // Loading instructions is safe and read-only
        ToolPolicy::Allow
    }

    fn is_mutating(&self) -> bool {
        false
    }

    fn is_parallel_safe(&self) -> bool {
        false
    }

    async fn execute(&self, args: Value) -> anyhow::Result<Value> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;

        if let Some(skill) = self.active_skills.read().await.get(name).cloned() {
            return Ok(build_skill_response(&skill, SKILL_ALREADY_ACTIVE_STATUS));
        }

        let codex_home = effective_codex_home(self.codex_home.as_deref());
        debug!(
            skill = name,
            workspace = %self.workspace_root.display(),
            codex_home = %codex_home.display(),
            "Loading session skill via manager-backed discovery"
        );

        let metadata = discover_session_skill_metadata(&self.workspace_root, &codex_home);
        if !metadata.errors.is_empty() {
            warn!(
                skill = name,
                error_count = metadata.errors.len(),
                sample = ?discovery_error_samples(&metadata.errors),
                "Session skill discovery reported warnings during load_skill"
            );
        }

        let mut loader =
            EnhancedSkillLoader::with_codex_home(self.workspace_root.clone(), codex_home.clone());
        let skill = match loader.get_skill(name).await {
            Ok(EnhancedSkill::Traditional(skill)) => *skill,
            Ok(EnhancedSkill::CliTool(_)) => {
                return Err(anyhow::anyhow!(
                    "Skill '{}' is a system utility and cannot be activated via load_skill",
                    name
                ));
            }
            Ok(EnhancedSkill::NativePlugin(_)) => {
                return Err(anyhow::anyhow!(
                    "Skill '{}' is a native plugin and cannot be activated via load_skill",
                    name
                ));
            }
            Err(error) => {
                let tools = discover_session_utilities(&self.workspace_root, &codex_home).await?;
                if tools.iter().any(|tool| tool.name == name) {
                    return Err(anyhow::anyhow!(
                        "Skill '{}' is a system utility and cannot be activated via load_skill",
                        name
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

                return Err(anyhow::anyhow!(
                    "Failed to load skill '{}': {}.{}",
                    name,
                    error,
                    detail
                ));
            }
        };

        let activation_status = match self
            .runtime
            .activate_skill(&self.active_skills, skill.clone())
            .await?
        {
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

    pub fn with_codex_home(
        workspace_root: PathBuf,
        active_skills: SkillMap,
        codex_home: Option<PathBuf>,
    ) -> Self {
        Self {
            workspace_root,
            codex_home,
            active_skills,
        }
    }
}

#[async_trait]
impl Tool for ListSkillsTool {
    fn name(&self) -> &'static str {
        "list_skills"
    }

    fn description(&self) -> &'static str {
        "List all available skills and system utilities. Use 'query' to filter by name, description, or routing hints, or 'variety' to filter by type ('agent_skill' or 'system_utility'). Traditional skills stay inactive until activated via 'load_skill'."
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
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase());
        let variety_filter = args.get("variety").and_then(|v| v.as_str());

        let active_skills = self.active_skills.read().await;
        let mut active_names = HashMap::new();
        for (name, skill) in active_skills.iter() {
            active_names.insert(name.clone(), skill.variety);
        }
        drop(active_skills);

        let codex_home = effective_codex_home(self.codex_home.as_deref());
        debug!(
            workspace = %self.workspace_root.display(),
            codex_home = %codex_home.display(),
            "Listing session skills via manager-backed discovery"
        );

        let discovery = discover_session_skill_metadata(&self.workspace_root, &codex_home);
        if !discovery.errors.is_empty() {
            warn!(
                error_count = discovery.errors.len(),
                sample = ?discovery_error_samples(&discovery.errors),
                "Session skill discovery reported warnings during list_skills"
            );
        }

        let mut skill_list = Vec::new();

        for skill_meta in discovery
            .skills
            .iter()
            .filter(|skill| skill.manifest.is_some())
        {
            let manifest = skill_meta
                .manifest
                .as_ref()
                .expect("filtered to skills with manifests");
            if !matches_skill_filters(
                manifest.name.as_str(),
                manifest.description.as_str(),
                manifest.when_to_use.as_deref(),
                manifest.when_not_to_use.as_deref(),
                manifest.variety,
                query.as_deref(),
                variety_filter,
            ) {
                continue;
            }

            let status = if active_names.contains_key(manifest.name.as_str()) {
                "active"
            } else {
                "dormant"
            };

            skill_list.push(json!({
                "name": manifest.name,
                "description": manifest.description,
                "path": skill_meta.path,
                "scope": skill_meta.scope,
                "when_to_use": manifest.when_to_use,
                "when_not_to_use": manifest.when_not_to_use,
                "variety": manifest.variety,
                "status": status,
            }));
        }

        for tool in discover_session_utilities(&self.workspace_root, &codex_home).await? {
            if !matches_skill_filters(
                tool.name.as_str(),
                tool.description.as_str(),
                None,
                None,
                SkillVariety::SystemUtility,
                query.as_deref(),
                variety_filter,
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
        let mut grouped = HashMap::new();
        for skill in &skill_list {
            let variety = skill
                .get("variety")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            grouped
                .entry(variety.to_string())
                .or_insert_with(Vec::new)
                .push(skill.clone());
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
            response_object.insert(
                "discovery_errors".to_string(),
                serde_json::json!(discovery.errors.len()),
            );
            response_object.insert(
                "discovery_error_samples".to_string(),
                serde_json::json!(discovery_error_samples(&discovery.errors)),
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
    fn name(&self) -> &'static str {
        "load_skill_resource"
    }

    fn description(&self) -> &'static str {
        "Access Level 3 resources (scripts, templates, technical docs) referenced in a skill's SKILL.md. Use this to read files from 'scripts/', 'references/', or 'assets/' when the high-level instructions require them."
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
        let skill_name = args
            .get("skill_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'skill_name' argument"))?;
        let resource_path = args
            .get("resource_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'resource_path' argument"))?;

        let skills = self.skills.read().await;
        if skills.is_empty() {
            return Err(anyhow::anyhow!(
                "No skills are active in this session yet. Use `load_skill` (or `/skills load <name>`) first."
            ));
        }
        if let Some(skill) = skills.get(skill_name) {
            // Security check: must be relative and within skill path
            let full_path = skill.path.join(resource_path);
            if !full_path.exists() {
                return Err(anyhow::anyhow!(
                    "Resource '{}' not found in skill '{}'",
                    resource_path,
                    skill_name
                ));
            }

            // Read content (limit size for safety)
            let content = read_file_with_context_sync(&full_path, "skill resource").context(
                format!("Failed to read resource at {}", full_path.display()),
            )?;

            Ok(serde_json::json!({
                "skill_name": skill_name,
                "resource_path": resource_path,
                "content": content
            }))
        } else {
            Err(anyhow::anyhow!("Skill '{}' not found", skill_name))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::{fs, path::Path};
    use tempfile::TempDir;

    const DEMO_SKILL_TOOL_NAME: &str = "demo-skill";

    fn temp_codex_home(workspace: &Path) -> PathBuf {
        workspace.join(".test-vtcode-home")
    }

    fn write_skill_fixture(workspace: &Path, name: &str) {
        let skill_dir = workspace.join(".agents/skills").join(name);
        let references_dir = skill_dir.join("references");
        fs::create_dir_all(&references_dir).expect("skill fixture dirs");
        fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                r#"---
name: {name}
description: Demo skill
vtcode-native: true
when-to-use: Use this when activated helper workflows are needed.
when-not-to-use: Do not use this for shell access.
---
Use the activated helper.

See `references/notes.txt`.
"#
            ),
        )
        .expect("skill file");
        fs::write(references_dir.join("notes.txt"), "demo notes").expect("skill resource");
    }

    fn write_invalid_skill_fixture(workspace: &Path, name: &str) {
        let skill_dir = workspace.join(".agents/skills").join(name);
        fs::create_dir_all(&skill_dir).expect("invalid skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                r#"---
name: {name}
description:
  - invalid
---
Broken skill
"#
            ),
        )
        .expect("invalid skill file");
    }

    fn write_rust_skills_metadata_fixture(workspace: &Path) {
        let skill_dir = workspace.join(".agents/skills").join("rust-skills");
        fs::create_dir_all(&skill_dir).expect("rust-skills dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: rust-skills
description: Rust guidance
license: MIT
metadata:
  author: leonardomso
  version: "1.0.0"
  sources:
    - Rust API Guidelines
    - Rust Performance Book
---
Use `/rust-skills`.
"#,
        )
        .expect("rust-skills skill file");
    }

    #[tokio::test]
    async fn traditional_skill_registration_exposes_native_cgp_factory() {
        let temp_dir = TempDir::new().expect("temp dir");
        write_skill_fixture(temp_dir.path(), DEMO_SKILL_TOOL_NAME);

        let mut loader = EnhancedSkillLoader::new(temp_dir.path().to_path_buf());
        let skill = match loader
            .get_skill(DEMO_SKILL_TOOL_NAME)
            .await
            .expect("discover skill")
        {
            EnhancedSkill::Traditional(skill) => *skill,
            _ => panic!("expected traditional skill"),
        };

        let registration = build_traditional_skill_tool_registration(&skill, None);
        assert!(registration.native_cgp_factory().is_some());
    }

    #[tokio::test]
    async fn traditional_skill_native_factory_preserves_registration_metadata() {
        let temp_dir = TempDir::new().expect("temp dir");
        write_skill_fixture(temp_dir.path(), DEMO_SKILL_TOOL_NAME);

        let mut loader = EnhancedSkillLoader::new(temp_dir.path().to_path_buf());
        let skill = match loader
            .get_skill(DEMO_SKILL_TOOL_NAME)
            .await
            .expect("discover skill")
        {
            EnhancedSkill::Traditional(skill) => *skill,
            _ => panic!("expected traditional skill"),
        };

        let registration = build_traditional_skill_tool_registration(&skill, None);
        let native_factory = registration
            .native_cgp_factory()
            .expect("registration should expose native factory");
        let wrapped = native_factory(
            &registration,
            temp_dir.path().to_path_buf(),
            CgpRuntimeMode::Interactive,
        );

        assert_eq!(wrapped.name(), DEMO_SKILL_TOOL_NAME);
        assert_eq!(wrapped.description(), skill.description());
        assert_eq!(
            wrapped.prompt_path().as_deref(),
            Some(SKILL_TOOL_PROMPT_PATH)
        );
        assert_eq!(wrapped.default_permission(), ToolPolicy::Prompt);
        assert!(wrapped.parameter_schema().is_some());
    }

    #[tokio::test]
    async fn load_skill_notifies_when_tool_snapshot_changes() {
        let temp_dir = TempDir::new().expect("temp dir");
        let skill_name = DEMO_SKILL_TOOL_NAME;
        write_skill_fixture(temp_dir.path(), skill_name);

        let active_tools = Arc::new(RwLock::new(Vec::new()));
        let change_count = Arc::new(AtomicUsize::new(0));
        let notifier_count = Arc::clone(&change_count);
        let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
        let active_skills = Arc::new(RwLock::new(HashMap::new()));
        let runtime = SkillToolSessionRuntime::new(
            Arc::clone(&registry),
            Some(Arc::clone(&active_tools)),
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
            Some(Arc::new(move |_| {
                notifier_count.fetch_add(1, Ordering::SeqCst);
            })),
        );

        let tool = LoadSkillTool::with_codex_home(
            temp_dir.path().to_path_buf(),
            Arc::clone(&active_skills),
            runtime,
            Some(temp_codex_home(temp_dir.path())),
        );

        let result = tool
            .execute(json!({ "name": skill_name }))
            .await
            .expect("load skill succeeds");

        assert_eq!(
            result["activation_status"].as_str(),
            Some("Associated tools activated and added to context.")
        );
        assert_eq!(change_count.load(Ordering::SeqCst), 1);
        assert!(active_skills.read().await.contains_key(skill_name));
        assert!(
            active_tools
                .read()
                .await
                .iter()
                .any(|tool| tool.function_name() == skill_name)
        );
    }

    #[tokio::test]
    async fn load_skill_resource_reads_from_active_skill_map() {
        let temp_dir = TempDir::new().expect("temp dir");
        let skill_name = DEMO_SKILL_TOOL_NAME;
        write_skill_fixture(temp_dir.path(), skill_name);

        let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
        let active_skills = Arc::new(RwLock::new(HashMap::new()));
        let runtime = SkillToolSessionRuntime::new(
            Arc::clone(&registry),
            None,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
            None,
        );
        let tool = LoadSkillTool::with_codex_home(
            temp_dir.path().to_path_buf(),
            Arc::clone(&active_skills),
            runtime,
            Some(temp_codex_home(temp_dir.path())),
        );

        tool.execute(json!({ "name": skill_name }))
            .await
            .expect("skill loads");

        let resource_tool = LoadSkillResourceTool::new(Arc::clone(&active_skills));
        let result = resource_tool
            .execute(json!({
                "skill_name": skill_name,
                "resource_path": "references/notes.txt"
            }))
            .await
            .expect("resource loads");

        assert_eq!(result["content"].as_str(), Some("demo notes"));
    }

    #[tokio::test]
    async fn load_skill_resource_fails_before_activation() {
        let active_skills = Arc::new(RwLock::new(HashMap::new()));
        let resource_tool = LoadSkillResourceTool::new(active_skills);

        let error = resource_tool
            .execute(json!({
                "skill_name": DEMO_SKILL_TOOL_NAME,
                "resource_path": "references/notes.txt"
            }))
            .await
            .expect_err("resource load should fail before activation");

        assert!(
            error
                .to_string()
                .contains("Use `load_skill` (or `/skills load <name>`) first.")
        );
    }

    #[tokio::test]
    async fn deactivate_skill_unregisters_tool() {
        let temp_dir = TempDir::new().expect("temp dir");
        let skill_name = DEMO_SKILL_TOOL_NAME;
        write_skill_fixture(temp_dir.path(), skill_name);

        let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
        let active_tools = Arc::new(RwLock::new(Vec::new()));
        let active_skills = Arc::new(RwLock::new(HashMap::new()));
        let runtime = SkillToolSessionRuntime::new(
            Arc::clone(&registry),
            Some(Arc::clone(&active_tools)),
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
            None,
        );
        let mut loader = EnhancedSkillLoader::new(temp_dir.path().to_path_buf());
        let skill = match loader
            .get_skill(skill_name)
            .await
            .expect("discover skill for activation")
        {
            EnhancedSkill::Traditional(skill) => *skill,
            _ => panic!("expected traditional skill"),
        };

        let activation_state = runtime
            .activate_skill(&active_skills, skill)
            .await
            .expect("activate skill");
        assert_eq!(activation_state, SkillActivationState::Activated);
        assert!(registry.has_tool(skill_name).await);

        let removed = runtime
            .deactivate_skill(&active_skills, skill_name)
            .await
            .expect("deactivate skill");
        assert!(removed);
        assert!(!active_skills.read().await.contains_key(skill_name));
        assert!(!registry.has_tool(skill_name).await);
        assert!(
            active_tools
                .read()
                .await
                .iter()
                .all(|tool| tool.function_name() != skill_name)
        );
    }

    #[tokio::test]
    async fn list_skills_discovers_bundled_skill_creator_from_vtcode_home() {
        let temp_dir = TempDir::new().expect("temp dir");
        let active_skills = Arc::new(RwLock::new(HashMap::new()));
        let tool = ListSkillsTool::with_codex_home(
            temp_dir.path().to_path_buf(),
            active_skills,
            Some(temp_codex_home(temp_dir.path())),
        );

        let result = tool
            .execute(json!({ "query": "skill-creator" }))
            .await
            .expect("list skills succeeds");

        assert_eq!(result["count"].as_u64(), Some(1));
        let groups = result["groups"]["agent_skill"]
            .as_array()
            .expect("agent skill group");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["name"].as_str(), Some("skill-creator"));
    }

    #[tokio::test]
    async fn load_skill_activates_bundled_skill_creator_from_vtcode_home() {
        let temp_dir = TempDir::new().expect("temp dir");
        let registry = Arc::new(ToolRegistry::new(temp_dir.path().to_path_buf()).await);
        let active_skills = Arc::new(RwLock::new(HashMap::new()));
        let runtime = SkillToolSessionRuntime::new(
            Arc::clone(&registry),
            None,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
            None,
        );
        let tool = LoadSkillTool::with_codex_home(
            temp_dir.path().to_path_buf(),
            Arc::clone(&active_skills),
            runtime,
            Some(temp_codex_home(temp_dir.path())),
        );

        let result = tool
            .execute(json!({ "name": "skill-creator" }))
            .await
            .expect("load bundled skill succeeds");

        assert_eq!(result["name"].as_str(), Some("skill-creator"));
        assert_eq!(
            result["activation_status"].as_str(),
            Some("Associated tools activated and added to context.")
        );
        assert!(active_skills.read().await.contains_key("skill-creator"));
    }

    #[tokio::test]
    async fn list_skills_surfaces_discovery_errors() {
        let temp_dir = TempDir::new().expect("temp dir");
        write_invalid_skill_fixture(temp_dir.path(), "broken-skill");
        let active_skills = Arc::new(RwLock::new(HashMap::new()));
        let tool = ListSkillsTool::with_codex_home(
            temp_dir.path().to_path_buf(),
            active_skills,
            Some(temp_codex_home(temp_dir.path())),
        );

        let result = tool.execute(json!({})).await.expect("list skills succeeds");

        assert_eq!(result["discovery_errors"].as_u64(), Some(1));
        let samples = result["discovery_error_samples"]
            .as_array()
            .expect("error samples");
        assert_eq!(samples.len(), 1);
        assert!(
            samples[0]
                .as_str()
                .expect("sample string")
                .contains("broken-skill")
        );
    }

    #[tokio::test]
    async fn list_skills_accepts_rust_skills_metadata_arrays() {
        let temp_dir = TempDir::new().expect("temp dir");
        write_rust_skills_metadata_fixture(temp_dir.path());
        let active_skills = Arc::new(RwLock::new(HashMap::new()));
        let tool = ListSkillsTool::with_codex_home(
            temp_dir.path().to_path_buf(),
            active_skills,
            Some(temp_codex_home(temp_dir.path())),
        );

        let result = tool
            .execute(json!({ "query": "rust-skills" }))
            .await
            .expect("list skills succeeds");

        assert_eq!(result["count"].as_u64(), Some(1));
        assert_eq!(result.get("discovery_errors"), None);
        let groups = result["groups"]["agent_skill"]
            .as_array()
            .expect("agent skill group");
        assert_eq!(groups[0]["name"].as_str(), Some("rust-skills"));
    }

    #[tokio::test]
    async fn list_skills_emits_agent_skill_routing_metadata() {
        let temp_dir = TempDir::new().expect("temp dir");
        write_skill_fixture(temp_dir.path(), DEMO_SKILL_TOOL_NAME);
        let active_skills = Arc::new(RwLock::new(HashMap::new()));
        let tool = ListSkillsTool::with_codex_home(
            temp_dir.path().to_path_buf(),
            active_skills,
            Some(temp_codex_home(temp_dir.path())),
        );

        let result = tool
            .execute(json!({ "query": DEMO_SKILL_TOOL_NAME }))
            .await
            .expect("list skills succeeds");

        let groups = result["groups"]["agent_skill"]
            .as_array()
            .expect("agent skill group");
        assert_eq!(groups.len(), 1);
        let entry = &groups[0];
        assert!(
            entry["path"]
                .as_str()
                .expect("path string")
                .contains(DEMO_SKILL_TOOL_NAME)
        );
        assert_eq!(entry["scope"].as_str(), Some("repo"));
        assert_eq!(
            entry["when_to_use"].as_str(),
            Some("Use this when activated helper workflows are needed.")
        );
        assert_eq!(
            entry["when_not_to_use"].as_str(),
            Some("Do not use this for shell access.")
        );
    }

    #[tokio::test]
    async fn list_skills_query_matches_when_to_use_guidance() {
        let temp_dir = TempDir::new().expect("temp dir");
        write_skill_fixture(temp_dir.path(), DEMO_SKILL_TOOL_NAME);
        let active_skills = Arc::new(RwLock::new(HashMap::new()));
        let tool = ListSkillsTool::with_codex_home(
            temp_dir.path().to_path_buf(),
            active_skills,
            Some(temp_codex_home(temp_dir.path())),
        );

        let result = tool
            .execute(json!({ "query": "helper workflows" }))
            .await
            .expect("list skills succeeds");

        assert_eq!(result["count"].as_u64(), Some(1));
        let groups = result["groups"]["agent_skill"]
            .as_array()
            .expect("agent skill group");
        assert_eq!(groups[0]["name"].as_str(), Some(DEMO_SKILL_TOOL_NAME));
    }
}
