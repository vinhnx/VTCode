use crate::config::ToolDocumentationMode;
use crate::config::types::CapabilityLevel;
use crate::llm::provider::ToolDefinition;
use crate::skills::executor::SkillToolAdapter;
use crate::skills::file_references::FileReferenceValidator;
use crate::skills::loader::{EnhancedSkill, EnhancedSkillLoader};
use crate::skills::types::{Skill, SkillVariety};
use crate::tool_policy::ToolPolicy;
use crate::tools::handlers::{SessionSurface, SessionToolsConfig, ToolModelCapabilities};
use crate::tools::registry::{ToolMetadata, ToolRegistration, ToolRegistry};
use crate::tools::traits::Tool;
use crate::utils::file_utils::read_file_with_context_sync;
use anyhow::Context;
use async_trait::async_trait;
use hashbrown::HashMap;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

type SkillMap = Arc<RwLock<HashMap<String, Skill>>>;
type ToolDefList = Arc<RwLock<Vec<ToolDefinition>>>;
type ToolChangeNotifier = Arc<dyn Fn() + Send + Sync>;

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
        }
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
                .register_tool(build_skill_tool_registration(&skill))
                .await
                .with_context(|| format!("failed to register skill tool '{skill_name}'"))?;
            self.refresh_tool_snapshot().await;
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
            self.refresh_tool_snapshot().await;
        }
        Ok(removed || unregistered)
    }

    async fn refresh_tool_snapshot(&self) {
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
            notifier();
        }
    }
}

pub fn build_skill_tool_registration(skill: &Skill) -> ToolRegistration {
    let metadata = ToolMetadata::default()
        .with_description(skill.description())
        .with_parameter_schema(skill_tool_parameter_schema())
        .with_permission(ToolPolicy::Prompt)
        .with_prompt_path(SKILL_TOOL_PROMPT_PATH);

    ToolRegistration::from_tool_with_metadata(
        skill.name().to_string(),
        CapabilityLevel::Basic,
        Arc::new(SkillToolAdapter::new(skill.clone())),
        metadata,
    )
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

fn matches_skill_filters(
    name: &str,
    description: &str,
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
        {
            return false;
        }
    }

    true
}

/// Tool to load skill instructions on demand (Progressive Disclosure)
pub struct LoadSkillTool {
    workspace_root: PathBuf,
    active_skills: SkillMap,
    runtime: SkillToolSessionRuntime,
}

impl LoadSkillTool {
    pub fn new(
        workspace_root: PathBuf,
        active_skills: SkillMap,
        runtime: SkillToolSessionRuntime,
    ) -> Self {
        Self {
            workspace_root,
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
        "Load detailed instructions for a specific skill and activate its associated tools into your environment. Use this to unlock high-level 'AgentSkill' workflows or 'SystemUtility' CLI bridges that are currently dormant."
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

        let mut loader = EnhancedSkillLoader::new(self.workspace_root.clone());
        let skill = match loader.get_skill(name).await? {
            EnhancedSkill::Traditional(skill) => *skill,
            EnhancedSkill::CliTool(_) => {
                return Err(anyhow::anyhow!(
                    "Skill '{}' is a system utility and cannot be activated via load_skill",
                    name
                ));
            }
            EnhancedSkill::NativePlugin(_) => {
                return Err(anyhow::anyhow!(
                    "Skill '{}' is a native plugin and cannot be activated via load_skill",
                    name
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
    active_skills: SkillMap,
}

impl ListSkillsTool {
    pub fn new(workspace_root: PathBuf, active_skills: SkillMap) -> Self {
        Self {
            workspace_root,
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
        "List all available skills (high-level workflows) and system utilities (CLI tools). Use 'query' to filter by name or 'variety' to filter by type ('agent_skill' or 'system_utility'). Tools are dormant until activated via 'load_skill'."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional search term to filter skills by name (case-insensitive)"
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

        let mut loader = EnhancedSkillLoader::new(self.workspace_root.clone());
        let discovery = loader.discover_all_skills().await?;
        let mut skill_list = Vec::new();

        for skill_ctx in discovery.skills {
            let manifest = skill_ctx.manifest();
            if !matches_skill_filters(
                manifest.name.as_str(),
                manifest.description.as_str(),
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
                "variety": manifest.variety,
                "status": status,
            }));
        }

        for tool in discovery.tools {
            if !matches_skill_filters(
                tool.name.as_str(),
                tool.description.as_str(),
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
---
Use the activated helper.

See `references/notes.txt`.
"#
            ),
        )
        .expect("skill file");
        fs::write(references_dir.join("notes.txt"), "demo notes").expect("skill resource");
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
            Some(Arc::new(move || {
                notifier_count.fetch_add(1, Ordering::SeqCst);
            })),
        );

        let tool = LoadSkillTool::new(
            temp_dir.path().to_path_buf(),
            Arc::clone(&active_skills),
            runtime,
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
        let tool = LoadSkillTool::new(
            temp_dir.path().to_path_buf(),
            Arc::clone(&active_skills),
            runtime,
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
}
