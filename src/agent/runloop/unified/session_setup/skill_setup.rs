use crate::agent::runloop::ResumeSession;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use vtcode_config::constants::tools as tool_constants;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::subagents::SubagentRegistry;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::handlers::SpawnSubagentTool;
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
    tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
) -> SkillSetupState {
    let mut discovered_skill_adapters: Vec<vtcode_core::skills::executor::SkillToolAdapter> =
        Vec::new();
    let library_skills_map = Arc::new(RwLock::new(HashMap::new()));
    let active_skills_map = Arc::new(RwLock::new(HashMap::new()));
    let mut dormant_tool_defs = HashMap::new();

    let mut skill_discovery = vtcode_core::skills::discovery::SkillDiscovery::new();
    match skill_discovery.discover_all(&config.workspace).await {
        Ok(result) => {
            info!(
                "Discovered {} skills and {} CLI tools",
                result.skills.len(),
                result.tools.len()
            );

            for skill_ctx in result.skills {
                if let Ok(lightweight_skill) = vtcode_core::skills::types::Skill::new(
                    skill_ctx.manifest().clone(),
                    skill_ctx.path().clone(),
                    String::new(),
                ) {
                    library_skills_map
                        .write()
                        .await
                        .insert(lightweight_skill.name().to_string(), lightweight_skill);
                }
            }

            for tool_config in result.tools {
                match vtcode_core::skills::cli_bridge::CliToolBridge::new(tool_config) {
                    Ok(bridge) => match bridge.to_skill() {
                        Ok(skill) => {
                            library_skills_map
                                .write()
                                .await
                                .insert(skill.name().to_string(), skill.clone());
                            let adapter =
                                vtcode_core::skills::executor::SkillToolAdapter::new(skill);
                            discovered_skill_adapters.push(adapter.clone());

                            let def = uni::ToolDefinition::function(
                                adapter.name().to_string(),
                                format!("(SKILL) {}", adapter.description()),
                                adapter.parameter_schema().unwrap_or(serde_json::json!({
                                    "type": "object",
                                    "properties": {
                                        "input": {"type": "string", "description": "Input arguments"}
                                    }
                                })),
                            );
                            dormant_tool_defs.insert(adapter.name().to_string(), def);
                        }
                        Err(e) => warn!("Failed to convert tool bridge to skill: {}", e),
                    },
                    Err(e) => warn!("Failed to create bridge for tool: {}", e),
                }
            }

            if let Some(resume_session) = resume {
                restore_active_skills_from_resume(
                    resume_session,
                    tools,
                    &active_skills_map,
                    &library_skills_map,
                    &dormant_tool_defs,
                )
                .await;
            }
        }
        Err(e) => warn!("Skill discovery failed: {}", e),
    }

    SkillSetupState {
        library_skills_map,
        active_skills_map,
        dormant_tool_defs,
        discovered_skill_adapters,
    }
}

pub(crate) async fn register_skill_and_subagent_tools(
    tool_registry: &mut ToolRegistry,
    tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
    skill_setup: &SkillSetupState,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
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
    register_spawn_subagent_tool(tool_registry, tools, config, vt_cfg).await?;
    Ok(())
}

async fn restore_active_skills_from_resume(
    resume_session: &ResumeSession,
    tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
    active_skills_map: &Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    library_skills_map: &Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    dormant_tool_defs: &HashMap<String, uni::ToolDefinition>,
) {
    let previously_active = &resume_session.snapshot.metadata.loaded_skills;
    if previously_active.is_empty() {
        return;
    }

    let mut tools_guard = tools.write().await;
    let mut active_skills = active_skills_map.write().await;
    let library_skills = library_skills_map.read().await;

    for skill_name in previously_active {
        if let Some(skill) = library_skills.get(skill_name) {
            active_skills.insert(skill_name.clone(), skill.clone());
        }
        if let Some(def) = dormant_tool_defs.get(skill_name)
            && !tools_guard
                .iter()
                .any(|t| t.function_name() == def.function_name())
        {
            info!("Restoring active skill tool: {}", skill_name);
            tools_guard.push(def.clone());
        }
    }
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

async fn register_spawn_subagent_tool(
    tool_registry: &mut ToolRegistry,
    tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<()> {
    let subagent_config = vt_cfg.map(|cfg| cfg.subagents.clone()).unwrap_or_default();
    if !subagent_config.enabled {
        debug!("Subagents are disabled via vtcode.toml");
        return Ok(());
    }

    let subagent_registry =
        SubagentRegistry::new(config.workspace.clone(), subagent_config).await?;
    let spawn_subagent_tool = SpawnSubagentTool::new(
        Arc::new(subagent_registry),
        config.clone(),
        Arc::new(tool_registry.clone()),
        config.workspace.clone(),
    );
    let spawn_subagent_reg = vtcode_core::tools::registry::ToolRegistration::from_tool_instance(
        tool_constants::SPAWN_SUBAGENT,
        vtcode_core::config::types::CapabilityLevel::Basic,
        spawn_subagent_tool,
    );
    tool_registry
        .register_tool(spawn_subagent_reg)
        .await
        .context("Failed to register spawn_subagent tool")?;

    let mut tools_guard = tools.write().await;
    tools_guard.push(uni::ToolDefinition::function(
        tool_constants::SPAWN_SUBAGENT.to_string(),
        "Spawn a specialized subagent to handle a specific task with isolated context. Subagents are useful for focused expertise or preserving main conversation context.".to_string(),
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Task description for the subagent"
                },
                "subagent_type": {
                    "type": "string",
                    "description": "Optional: specific subagent type (explore, plan, general, code-reviewer, debugger)"
                },
                "resume": {
                    "type": "string",
                    "description": "Optional: agent ID to resume"
                },
                "thoroughness": {
                    "type": "string",
                    "description": "Optional: thoroughness level (quick, medium, very_thorough). Default: medium."
                },
                "timeout_seconds": {
                    "type": "integer",
                    "description": "Optional: timeout in seconds"
                },
                "parent_context": {
                    "type": "string",
                    "description": "Optional: context from parent agent"
                }
            },
            "required": ["prompt"]
        }),
    ));
    Ok(())
}
