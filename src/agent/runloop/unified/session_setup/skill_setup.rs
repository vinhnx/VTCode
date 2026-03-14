use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::unified::session_setup::init::refresh_tool_snapshot;
use crate::agent::runloop::unified::tool_catalog::{
    ToolCatalogState, tool_catalog_change_notifier,
};
use anyhow::{Context, Result};
use hashbrown::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use vtcode_config::constants::tools as tool_constants;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::handlers::ToolModelCapabilities;
use vtcode_core::tools::native_cgp_tool_factory;

pub(crate) struct SkillSetupState {
    pub active_skills_map: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
}

pub(crate) async fn discover_skills(
    config: &CoreAgentConfig,
    resume: Option<&ResumeSession>,
) -> SkillSetupState {
    let active_skills_map = Arc::new(RwLock::new(HashMap::new()));

    info!(
        workspace = %config.workspace.display(),
        "Deferring skill discovery until an explicit /skills command"
    );

    if let Some(resume_session) = resume
        && !resume_session.loaded_skills().is_empty()
    {
        warn!(
            "Skipping loaded skill restore during startup; use /skills load to reactivate session skills"
        );
    }

    SkillSetupState { active_skills_map }
}

pub(crate) async fn register_skill_tools(
    tool_registry: &mut ToolRegistry,
    tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
    tool_catalog: &Arc<ToolCatalogState>,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    tool_documentation_mode: vtcode_core::config::ToolDocumentationMode,
    skill_setup: &SkillSetupState,
) -> Result<()> {
    let runtime = vtcode_core::tools::skills::SkillToolSessionRuntime::new(
        Arc::new(tool_registry.clone()),
        Some(Arc::clone(tools)),
        tool_documentation_mode,
        ToolModelCapabilities::for_model_name(&config.model),
        Some(tool_catalog_change_notifier(tool_catalog)),
    )
    .with_fork_executor(Arc::new(
        vtcode_core::skills::executor::ChildAgentSkillExecutor::new(
            Arc::new(tool_registry.clone()),
            vtcode_core::skills::executor::ForkSkillRuntimeConfig {
                workspace: config.workspace.clone(),
                model: config.model.clone(),
                api_key: config.api_key.clone(),
                vt_cfg: vt_cfg.cloned(),
            },
        ),
    ));

    register_list_skills_tool(
        tool_registry,
        config.workspace.clone(),
        Arc::clone(&skill_setup.active_skills_map),
    )
    .await?;
    register_load_skill_resource_tool(tool_registry, Arc::clone(&skill_setup.active_skills_map))
        .await?;
    register_load_skill_tool(
        tool_registry,
        config.workspace.clone(),
        Arc::clone(&skill_setup.active_skills_map),
        runtime,
    )
    .await?;

    refresh_tool_snapshot(
        tool_registry,
        tools,
        tool_catalog,
        config,
        tool_documentation_mode,
    )
    .await;
    Ok(())
}

async fn register_list_skills_tool(
    tool_registry: &mut ToolRegistry,
    workspace_root: std::path::PathBuf,
    active_skills_map: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
) -> Result<()> {
    let list_skills_tool = vtcode_core::tools::skills::ListSkillsTool::new(
        workspace_root.clone(),
        Arc::clone(&active_skills_map),
    );
    let list_skills_reg = vtcode_core::tools::registry::ToolRegistration::from_tool_instance(
        tool_constants::LIST_SKILLS,
        vtcode_core::config::types::CapabilityLevel::Basic,
        list_skills_tool,
    )
    .with_native_cgp_factory(native_cgp_tool_factory({
        let workspace_root = workspace_root.clone();
        let active_skills_map = Arc::clone(&active_skills_map);
        move || {
            vtcode_core::tools::skills::ListSkillsTool::new(
                workspace_root.clone(),
                Arc::clone(&active_skills_map),
            )
        }
    }));
    tool_registry
        .register_tool(list_skills_reg)
        .await
        .context("Failed to register list_skills tool")?;
    Ok(())
}

async fn register_load_skill_resource_tool(
    tool_registry: &mut ToolRegistry,
    active_skills_map: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
) -> Result<()> {
    let load_resource_tool =
        vtcode_core::tools::skills::LoadSkillResourceTool::new(Arc::clone(&active_skills_map));
    let load_resource_reg = vtcode_core::tools::registry::ToolRegistration::from_tool_instance(
        tool_constants::LOAD_SKILL_RESOURCE,
        vtcode_core::config::types::CapabilityLevel::Basic,
        load_resource_tool,
    )
    .with_native_cgp_factory(native_cgp_tool_factory({
        let active_skills_map = Arc::clone(&active_skills_map);
        move || {
            vtcode_core::tools::skills::LoadSkillResourceTool::new(Arc::clone(&active_skills_map))
        }
    }));
    tool_registry
        .register_tool(load_resource_reg)
        .await
        .context("Failed to register load_skill_resource tool")?;
    Ok(())
}

async fn register_load_skill_tool(
    tool_registry: &mut ToolRegistry,
    workspace_root: std::path::PathBuf,
    active_skills_map: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    runtime: vtcode_core::tools::skills::SkillToolSessionRuntime,
) -> Result<()> {
    let load_skill_tool = vtcode_core::tools::skills::LoadSkillTool::new(
        workspace_root.clone(),
        Arc::clone(&active_skills_map),
        runtime.clone(),
    );
    let load_skill_reg = vtcode_core::tools::registry::ToolRegistration::from_tool_instance(
        tool_constants::LOAD_SKILL,
        vtcode_core::config::types::CapabilityLevel::Basic,
        load_skill_tool,
    )
    .with_native_cgp_factory(native_cgp_tool_factory({
        let workspace_root = workspace_root.clone();
        let active_skills_map = Arc::clone(&active_skills_map);
        let runtime = runtime.clone();
        move || {
            vtcode_core::tools::skills::LoadSkillTool::new(
                workspace_root.clone(),
                Arc::clone(&active_skills_map),
                runtime.clone(),
            )
        }
    }));
    tool_registry
        .register_tool(load_skill_reg)
        .await
        .context("Failed to register load_skill tool")?;
    Ok(())
}
