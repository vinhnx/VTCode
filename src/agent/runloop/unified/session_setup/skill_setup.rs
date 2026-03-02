use crate::agent::runloop::ResumeSession;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use vtcode_config::constants::tools as tool_constants;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::traits::Tool;

pub(crate) struct SkillSetupState {
    pub library_skills_map: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    pub active_skills_map: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    pub dormant_tool_defs: HashMap<String, uni::ToolDefinition>,
    pub discovered_skill_adapters: Vec<vtcode_core::skills::executor::SkillToolAdapter>,
}

pub(crate) async fn discover_skills(
    config: &CoreAgentConfig,
    resume: Option<&ResumeSession>,
) -> SkillSetupState {
    let discovered_skill_adapters: Vec<vtcode_core::skills::executor::SkillToolAdapter> =
        Vec::new();
    let library_skills_map = Arc::new(RwLock::new(HashMap::new()));
    let active_skills_map = Arc::new(RwLock::new(HashMap::new()));
    let dormant_tool_defs = HashMap::new();

    info!(
        workspace = %config.workspace.display(),
        "Deferring skill discovery until an explicit /skills command"
    );

    if let Some(resume_session) = resume
        && !resume_session.snapshot.metadata.loaded_skills.is_empty()
    {
        warn!(
            "Skipping loaded skill restore during startup; use /skills load to reactivate session skills"
        );
    }

    SkillSetupState {
        library_skills_map,
        active_skills_map,
        dormant_tool_defs,
        discovered_skill_adapters,
    }
}

pub(crate) async fn register_skill_tools(
    tool_registry: &mut ToolRegistry,
    tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
    skill_setup: &SkillSetupState,
) -> Result<()> {
    register_list_skills_tool(
        tool_registry,
        tools,
        skill_setup.library_skills_map.clone(),
        skill_setup.dormant_tool_defs.clone(),
    )
    .await?;
    register_load_skill_resource_tool(tool_registry, tools, skill_setup.library_skills_map.clone())
        .await?;
    register_load_skill_tool(
        tool_registry,
        tools,
        skill_setup.library_skills_map.clone(),
        skill_setup.active_skills_map.clone(),
        skill_setup.dormant_tool_defs.clone(),
        &skill_setup.discovered_skill_adapters,
    )
    .await?;
    Ok(())
}

async fn register_list_skills_tool(
    tool_registry: &mut ToolRegistry,
    tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
    library_skills_map: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    dormant_tool_defs: HashMap<String, uni::ToolDefinition>,
) -> Result<()> {
    let list_skills_tool =
        vtcode_core::tools::skills::ListSkillsTool::new(library_skills_map, dormant_tool_defs);
    let list_skills_reg = vtcode_core::tools::registry::ToolRegistration::from_tool_instance(
        tool_constants::LIST_SKILLS,
        vtcode_core::config::types::CapabilityLevel::Basic,
        list_skills_tool,
    );
    tool_registry
        .register_tool(list_skills_reg)
        .await
        .context("Failed to register list_skills tool")?;

    let mut tools_guard = tools.write().await;
    tools_guard.push(uni::ToolDefinition::function(
        tool_constants::LIST_SKILLS.to_string(),
        "List all available skills that can be loaded. Use 'query' to filter by name or 'variety' to filter by type (agent_skill, system_utility).".to_string(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Optional: filter skills by name (case-insensitive)"
                },
                "variety": {
                    "type": "string",
                    "enum": ["agent_skill", "system_utility", "built_in"],
                    "description": "Optional: filter by skill type"
                }
            },
            "additionalProperties": false
        }),
    ));
    Ok(())
}

async fn register_load_skill_resource_tool(
    tool_registry: &mut ToolRegistry,
    tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
    library_skills_map: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
) -> Result<()> {
    let load_resource_tool =
        vtcode_core::tools::skills::LoadSkillResourceTool::new(library_skills_map);
    let load_resource_reg = vtcode_core::tools::registry::ToolRegistration::from_tool_instance(
        tool_constants::LOAD_SKILL_RESOURCE,
        vtcode_core::config::types::CapabilityLevel::Basic,
        load_resource_tool,
    );
    tool_registry
        .register_tool(load_resource_reg)
        .await
        .context("Failed to register load_skill_resource tool")?;

    let mut tools_guard = tools.write().await;
    tools_guard.push(uni::ToolDefinition::function(
        tool_constants::LOAD_SKILL_RESOURCE.to_string(),
        "Load the content of a specific resource belonging to a skill. Use this when instructed by a skill's SKILL.md.".to_string(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill_name": {"type": "string"},
                "resource_path": {"type": "string"}
            },
            "required": ["skill_name", "resource_path"]
        }),
    ));
    Ok(())
}

async fn register_load_skill_tool(
    tool_registry: &mut ToolRegistry,
    tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
    library_skills_map: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    active_skills_map: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    dormant_tool_defs: HashMap<String, uni::ToolDefinition>,
    discovered_skill_adapters: &[vtcode_core::skills::executor::SkillToolAdapter],
) -> Result<()> {
    let mut dormant_adapters_map = HashMap::new();
    for adapter in discovered_skill_adapters.iter().cloned() {
        dormant_adapters_map.insert(
            adapter.name().to_string(),
            Arc::new(adapter) as Arc<dyn Tool>,
        );
    }
    let dormant_adapters = Arc::new(RwLock::new(dormant_adapters_map));

    let load_skill_tool = vtcode_core::tools::skills::LoadSkillTool::new(
        library_skills_map,
        active_skills_map,
        dormant_tool_defs,
        dormant_adapters,
        Some(tools.clone()),
        Some(Arc::new(RwLock::new(tool_registry.clone()))),
    );
    let load_skill_reg = vtcode_core::tools::registry::ToolRegistration::from_tool_instance(
        tool_constants::LOAD_SKILL,
        vtcode_core::config::types::CapabilityLevel::Basic,
        load_skill_tool,
    );
    tool_registry
        .register_tool(load_skill_reg)
        .await
        .context("Failed to register load_skill tool")?;

    let mut tools_guard = tools.write().await;
    tools_guard.push(uni::ToolDefinition::function(
        tool_constants::LOAD_SKILL.to_string(),
        "Load a specific skill to see full instructions and activate its tools.".to_string(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            },
            "required": ["name"]
        }),
    ));
    Ok(())
}
