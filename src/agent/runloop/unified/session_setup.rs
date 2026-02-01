use super::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::telemetry::build_trajectory_logger;
use crate::agent::runloop::welcome::{SessionBootstrap, prepare_session_bootstrap};
use anyhow::{Context, Result};
use chrono::Local;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use tokio::sync::{Notify, RwLock};
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use vtcode_config::constants::tools as tool_constants;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::subagents::SubagentRegistry;
use vtcode_core::tools::ApprovalRecorder;
use vtcode_core::tools::handlers::SpawnSubagentTool;
use vtcode_core::tools::traits::Tool;

use super::async_mcp_manager::AsyncMcpManager;
use super::palettes::apply_prompt_style;
use super::prompts::read_system_prompt;
use super::state::CtrlCState;
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::ui::{build_inline_header_context, render_session_banner};
use crate::agent::runloop::unified::turn::utils::render_hook_messages;
use crate::agent::runloop::unified::turn::workspace::load_workspace_files;
use crate::hooks::lifecycle::LifecycleHookEngine;
use crate::ide_context::IdeContextBridge;
use crate::workspace_trust;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::snapshots::{SnapshotConfig, SnapshotManager};
use vtcode_core::core::agent::state::recover_history_from_crash;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::{factory::create_provider_with_config, provider as uni};
use vtcode_core::mcp::{McpClient, McpToolInfo};
use vtcode_core::models::ModelId;
use vtcode_core::prompts::CustomPromptRegistry;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::build_function_declarations_cached;
use vtcode_core::tools::{SearchMetrics, ToolResultCache};
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::{
    InlineEventCallback, InlineHandle, InlineSession, spawn_session_with_prompts, theme_from_styles,
};
use vtcode_core::ui::user_confirmation::TaskComplexity;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::formatting::indent_block;
use vtcode_core::utils::session_archive::{SessionArchive, SessionArchiveMetadata};
use vtcode_core::utils::transcript;

/// Convert agent.circuit_breaker config to core CircuitBreakerConfig
/// Maps user-facing settings to the actual circuit breaker parameters
#[allow(clippy::unnecessary_cast)]
fn vtcode_config_circuit_breaker_to_core(
    vt_cfg: Option<&VTCodeConfig>,
    _agent_config: &CoreAgentConfig,
) -> vtcode_core::tools::circuit_breaker::CircuitBreakerConfig {
    // Get circuit breaker config from vtcode.toml if available
    let default_cfg = vtcode_config::core::agent::CircuitBreakerConfig::default();
    let cfg = vt_cfg
        .map(|c| &c.agent.circuit_breaker)
        .unwrap_or(&default_cfg);

    if !cfg.enabled {
        // Return a permissive config if disabled
        return vtcode_core::tools::circuit_breaker::CircuitBreakerConfig {
            failure_threshold: u32::MAX, // Never trip
            reset_timeout: Duration::from_secs(1),
            min_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(1),
            backoff_factor: 1.0,
        };
    }

    // Map to core config
    // recovery_cooldown (seconds) -> reset_timeout (duration)
    // failure_threshold -> failure_threshold
    vtcode_core::tools::circuit_breaker::CircuitBreakerConfig {
        failure_threshold: cfg.failure_threshold,
        reset_timeout: Duration::from_secs(cfg.recovery_cooldown.max(1) as u64),
        min_backoff: Duration::from_secs(10),
        max_backoff: Duration::from_secs(300),
        backoff_factor: 2.0,
    }
}

pub(crate) struct SessionState {
    pub session_bootstrap: SessionBootstrap,
    pub provider_client: Box<dyn uni::LLMProvider>,
    pub tool_registry: ToolRegistry,
    pub tools: Arc<RwLock<Vec<uni::ToolDefinition>>>,
    /// Cached tool definitions for efficient reuse across turns (HP-3 optimization)
    pub cached_tools: Option<Arc<Vec<uni::ToolDefinition>>>,
    pub conversation_history: Vec<uni::Message>,
    pub decision_ledger: Arc<RwLock<DecisionTracker>>,
    pub trajectory: TrajectoryLogger,
    pub base_system_prompt: String,
    pub full_auto_allowlist: Option<Vec<String>>,
    pub async_mcp_manager: Option<Arc<AsyncMcpManager>>,
    pub mcp_panel_state: mcp_events::McpPanelState,
    pub tool_result_cache: Arc<RwLock<ToolResultCache>>,
    pub tool_permission_cache: Arc<RwLock<ToolPermissionCache>>,
    #[allow(dead_code)]
    pub search_metrics: Arc<RwLock<SearchMetrics>>,

    pub custom_prompts: CustomPromptRegistry,
    pub custom_slash_commands: vtcode_core::prompts::CustomSlashCommandRegistry,

    /// Skills loaded in current session (name -> Skill mapping)
    pub loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    pub approval_recorder: Arc<ApprovalRecorder>,
    pub safety_validator: Arc<RwLock<ToolCallSafetyValidator>>,
    // Phase 4 Integration: Resilient execution components
    pub circuit_breaker: Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub validation_cache: Arc<vtcode_core::tools::validation_cache::ValidationCache>,
    pub telemetry: Arc<vtcode_core::core::telemetry::TelemetryManager>,
    pub autonomous_executor: Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
    pub error_recovery:
        Arc<StdRwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
}

#[allow(dead_code)]
pub(crate) struct SessionUISetup {
    pub renderer: AnsiRenderer,
    pub session: InlineSession,
    pub handle: InlineHandle,
    pub ctrl_c_state: Arc<CtrlCState>,
    pub ctrl_c_notify: Arc<Notify>,
    pub checkpoint_manager: Option<SnapshotManager>,
    pub session_archive: Option<SessionArchive>,
    pub lifecycle_hooks: Option<LifecycleHookEngine>,
    pub session_end_reason: crate::hooks::lifecycle::SessionEndReason,
    pub context_manager: super::context_manager::ContextManager,
    pub default_placeholder: Option<String>,
    pub follow_up_placeholder: Option<String>,
    pub next_checkpoint_turn: usize,
    pub ui_redraw_batcher: crate::agent::runloop::unified::turn::utils::UIRedrawBatcher,
}

async fn build_conversation_history_from_resume(
    resume: Option<&ResumeSession>,
) -> Vec<uni::Message> {
    resume
        .map(|session| session.history.clone())
        .unwrap_or_default()
}

pub(crate) async fn initialize_session(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    full_auto: bool,
    resume: Option<&ResumeSession>,
) -> Result<SessionState> {
    let tool_documentation_mode = vt_cfg
        .map(|cfg| cfg.agent.tool_documentation_mode)
        .unwrap_or_default();

    // Create async MCP manager if enabled
    let async_mcp_manager = if let Some(cfg) = vt_cfg {
        if cfg.mcp.enabled {
            info!(
                "Setting up async MCP client with {} providers",
                cfg.mcp.providers.len()
            );

            let manager = AsyncMcpManager::new(
                cfg.mcp.clone(),
                cfg.security.hitl_notification_bell,
                Arc::new(|_event: mcp_events::McpEvent| {}),
            );
            let manager_arc = Arc::new(manager);

            // Start async initialization (non-blocking)
            if let Err(e) = manager_arc.start_initialization() {
                error!("Failed to start async MCP initialization: {}", e);
            }

            Some(manager_arc)
        } else {
            debug!("MCP is disabled in configuration");
            None
        }
    } else {
        debug!("No VT Code config provided");
        None
    };

    // Determine initial MCP error for session bootstrap based on manager status
    let mcp_error = if let Some(ref manager) = async_mcp_manager {
        match manager.get_status().await {
            super::async_mcp_manager::McpInitStatus::Error { message } => Some(message.clone()),
            super::async_mcp_manager::McpInitStatus::Initializing { .. } => {
                // Still initializing, no error yet
                None
            }
            super::async_mcp_manager::McpInitStatus::Disabled => None,
            super::async_mcp_manager::McpInitStatus::Ready { .. } => None,
        }
    } else {
        None
    };

    let session_bootstrap = prepare_session_bootstrap(config, vt_cfg, mcp_error).await;
    let provider_name = if config.provider.trim().is_empty() {
        config
            .model
            .parse::<ModelId>()
            .ok()
            .map(|model| model.provider().to_string())
            .unwrap_or_else(|| "gemini".to_string())
    } else {
        config.provider.to_lowercase()
    };
    let provider_client = create_provider_with_config(
        &provider_name,
        Some(config.api_key.clone()),
        None,
        Some(config.model.clone()),
        Some(config.prompt_cache.clone()),
        None,
        vt_cfg.as_ref().map(|cfg| cfg.provider.anthropic.clone()),
    )
    .context("Failed to initialize provider client")?;

    let mut full_auto_allowlist: Option<Vec<String>> = None;

    // Use cached declarations to avoid per-session rebuilds (HP-3 optimization)
    let base_declarations = build_function_declarations_cached(tool_documentation_mode);
    let mut tool_definitions: Vec<uni::ToolDefinition> = base_declarations
        .iter()
        .map(|decl| {
            uni::ToolDefinition::function(
                decl.name.clone(),
                decl.description.clone(),
                vtcode_core::llm::providers::gemini::sanitize_function_parameters(
                    decl.parameters.clone(),
                ),
            )
        })
        .collect();

    // Add GPT-5.1 specific tools if the model supports them
    if let Ok(model_id) = ModelId::from_str(&config.model)
        && model_id.is_gpt51_variant()
    {
        // Add apply_patch tool for GPT-5.1's structured diff editing
        tool_definitions.push(uni::ToolDefinition::apply_patch(
                "Apply VT Code patches to modify files. IMPORTANT: Use VT Code format (*** Begin Patch, *** Update File: path, @@ context, -/+ lines, *** End Patch), NOT unified diff (---/+++) format. The tool creates, updates, or deletes file content.".to_string()
            ));
    }

    // Add MCP tools if available (from async manager). Poll briefly for readiness
    // so a fast-starting MCP server will be exposed during session startup.
    if let Some(ref manager) = async_mcp_manager {
        debug!("Checking for MCP tools from async manager...");

        // Quick polling window to let MCP finish startup (non-blocking overall)
        let mut mcp_client_ready: Option<Arc<McpClient>> = None;
        for _ in 0..15 {
            let status = manager.get_status().await;
            if let super::async_mcp_manager::McpInitStatus::Ready { client } = &status {
                mcp_client_ready = Some(Arc::clone(client));
                break;
            }
            if status.is_error() {
                debug!("MCP manager reported error during startup: {:?}", status);
                break;
            }
            // wait a short interval before retrying
            sleep(Duration::from_millis(200)).await;
        }

        if let Some(client) = mcp_client_ready {
            match client.list_tools().await {
                Ok(mcp_tools) => {
                    info!("Found {} MCP tools", mcp_tools.len());
                    let extra_tools = build_mcp_tool_definitions(&mcp_tools);
                    tool_definitions.extend(extra_tools);
                }
                Err(err) => {
                    warn!("Failed to discover MCP tools from async manager: {err}");
                }
            }
        } else {
            debug!("MCP client not ready yet, tools will be available later");
        }
    }

    let tools = Arc::new(RwLock::new(tool_definitions));

    // Perform skill discovery (CLI tools)
    let mut discovered_skill_adapters: Vec<vtcode_core::skills::executor::SkillToolAdapter> =
        Vec::new();
    let mut discovered_skills_map = HashMap::new();

    // Initialize skill maps early for resume logic
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

            // Process Traditional Skills (Markdown)
            for skill_ctx in result.skills {
                // Create lightweight skill for prompt generation
                if let Ok(lightweight_skill) = vtcode_core::skills::types::Skill::new(
                    skill_ctx.manifest().clone(),
                    skill_ctx.path().clone(),
                    String::new(), // Placeholder instructions for prompt-only listing
                ) {
                    discovered_skills_map.insert(
                        lightweight_skill.name().to_string(),
                        lightweight_skill.clone(),
                    );
                    library_skills_map
                        .write()
                        .await
                        .insert(lightweight_skill.name().to_string(), lightweight_skill);
                }
            }

            // Process CLI tools
            for tool_config in result.tools {
                match vtcode_core::skills::cli_bridge::CliToolBridge::new(tool_config) {
                    Ok(bridge) => {
                        match bridge.to_skill() {
                            Ok(skill) => {
                                discovered_skills_map
                                    .insert(skill.name().to_string(), skill.clone());
                                library_skills_map
                                    .write()
                                    .await
                                    .insert(skill.name().to_string(), skill.clone());
                                let adapter =
                                    vtcode_core::skills::executor::SkillToolAdapter::new(skill);
                                discovered_skill_adapters.push(adapter.clone());

                                // Create definition but store in dormant map
                                let def = uni::ToolDefinition::function(
                                    adapter.name().to_string(),
                                    format!("(SKILL) {}", adapter.description()),
                                    adapter.parameter_schema().unwrap_or(serde_json::json!({
                                        "type": "object",
                                        "properties": {
                                            "input": {"type": "string", "description": "Input arguments"}
                                        }
                                    }))
                                );
                                dormant_tool_defs.insert(adapter.name().to_string(), def);
                            }
                            Err(e) => warn!("Failed to convert tool bridge to skill: {}", e),
                        }
                    }
                    Err(e) => warn!("Failed to create bridge for tool: {}", e),
                }
            }

            // Initialize skill maps early for resume logic

            // On Resume: Auto-activate skills that were active in the previous session
            if let Some(resume_session) = resume {
                let previously_active = &resume_session.snapshot.metadata.loaded_skills;
                if !previously_active.is_empty() {
                    let mut tools_guard = tools.write().await;
                    let mut active_skills = active_skills_map.write().await;
                    let library_skills = library_skills_map.read().await;

                    for skill_name in previously_active {
                        // Restore to active registry
                        if let Some(skill) = library_skills.get(skill_name) {
                            active_skills.insert(skill_name.clone(), skill.clone());
                        }

                        // Restore associated tools
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
            }
        }
        Err(e) => warn!("Skill discovery failed: {}", e),
    }

    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));

    let mut conversation_history = build_conversation_history_from_resume(resume).await;

    // Context Manager: Enforce call/output invariants during session resume.
    // This ensures that if the previous session crashed or was interrupted during tool execution,
    // we have synthetic outputs for any pending tool calls to maintain history validity.
    recover_history_from_crash(&mut conversation_history);

    let trajectory = build_trajectory_logger(&config.workspace, vt_cfg);
    let base_system_prompt = read_system_prompt(
        &config.workspace,
        session_bootstrap.prompt_addendum.as_deref(),
    )
    .await;

    // Initialize MCP panel state
    let mcp_panel_state = if let Some(cfg) = vt_cfg {
        mcp_events::McpPanelState::new(cfg.mcp.ui.max_events, cfg.mcp.enabled)
    } else {
        mcp_events::McpPanelState::default()
    };

    let _pty_config = vt_cfg.map(|cfg| cfg.pty.clone()).unwrap_or_default();

    let mut tool_registry = ToolRegistry::new(config.workspace.clone()).await;
    tool_registry.initialize_async().await?;

    if let Some(cfg) = vt_cfg {
        tool_registry.apply_commands_config(&cfg.commands);
        tool_registry.apply_timeout_policy(&cfg.timeouts);
        if let Err(err) = tool_registry.apply_config_policies(&cfg.tools).await {
            tracing::warn!("Failed to apply tool policies from config: {}", err);
        }

        // Add MCP client to tool registry if available from async manager
        if cfg.mcp.enabled
            && let Some(ref manager) = async_mcp_manager
        {
            let status = manager.get_status().await;
            if let super::async_mcp_manager::McpInitStatus::Ready { client } = &status {
                tool_registry = tool_registry.with_mcp_client(Arc::clone(client)).await;
                if let Err(err) = tool_registry.refresh_mcp_tools().await {
                    warn!("Failed to refresh MCP tools: {}", err);
                }
            }
        }

        // Initialize full auto mode if requested
        if full_auto {
            let automation_cfg = cfg.automation.full_auto.clone();
            tool_registry
                .enable_full_auto_mode(&automation_cfg.allowed_tools)
                .await;
            let allowlist = tool_registry
                .current_full_auto_allowlist()
                .await
                .unwrap_or_default();
            full_auto_allowlist = Some(allowlist);
        }
    }

    let workspace_trust_level = match session_bootstrap.acp_workspace_trust {
        Some(level) => Some(level.to_workspace_trust_level()),
        None => workspace_trust::workspace_trust_level(&config.workspace)
            .await
            .context("Failed to determine workspace trust level for tool policy")?,
    };

    let enforce_safe_mode_prompts = if full_auto {
        false
    } else {
        match workspace_trust_level {
            Some(WorkspaceTrustLevel::FullAuto) => false,
            Some(WorkspaceTrustLevel::ToolsPolicy) | None => true,
        }
    };

    tool_registry
        .set_enforce_safe_mode_prompts(enforce_safe_mode_prompts)
        .await;

    // Now register all on-demand skill tools in the registry

    // 1. ListSkills
    let list_skills_tool = vtcode_core::tools::skills::ListSkillsTool::new(
        library_skills_map.clone(),
        dormant_tool_defs.clone(),
    );
    let list_skills_reg = vtcode_core::tools::registry::ToolRegistration::from_tool_instance(
        tool_constants::LIST_SKILLS,
        vtcode_core::config::types::CapabilityLevel::Basic,
        list_skills_tool,
    );
    tool_registry
        .register_tool(list_skills_reg)
        .await
        .context("Failed to register list_skills tool")?;

    // Add list_skills to active tool definitions
    {
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
              })
         ));
    }

    // 2. LoadSkillResource
    let load_resource_tool =
        vtcode_core::tools::skills::LoadSkillResourceTool::new(library_skills_map.clone());
    let load_resource_reg = vtcode_core::tools::registry::ToolRegistration::from_tool_instance(
        tool_constants::LOAD_SKILL_RESOURCE,
        vtcode_core::config::types::CapabilityLevel::Basic,
        load_resource_tool,
    );
    tool_registry
        .register_tool(load_resource_reg)
        .await
        .context("Failed to register load_skill_resource tool")?;

    {
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
            })
         ));
    }

    // 3. LoadSkill
    let mut dormant_adapters_map = HashMap::new();
    for adapter in discovered_skill_adapters {
        dormant_adapters_map.insert(
            adapter.name().to_string(),
            Arc::new(adapter) as Arc<dyn vtcode_core::tools::traits::Tool>,
        );
    }
    let dormant_adapters = Arc::new(RwLock::new(dormant_adapters_map));

    let load_skill_tool = vtcode_core::tools::skills::LoadSkillTool::new(
        library_skills_map.clone(),
        active_skills_map.clone(),
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

    {
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
    }

    // 4. SpawnSubagent (optional)
    let subagent_config = vt_cfg.map(|cfg| cfg.subagents.clone()).unwrap_or_default();

    if subagent_config.enabled {
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

        {
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
        }
    } else {
        debug!("Subagents are disabled via vtcode.toml");
    }

    // 5. Discovered CLI tool adapters
    // DEFERRED: Adapters are now kept in dormant_tool_defs and registered on-demand via load_skill

    let custom_prompts = CustomPromptRegistry::load(
        vt_cfg.map(|cfg| &cfg.agent.custom_prompts),
        &config.workspace,
    )
    .await
    .unwrap_or_else(|err| {
        warn!("failed to load custom prompts: {err:#}");
        CustomPromptRegistry::default()
    });

    let custom_slash_commands =
        vtcode_core::prompts::CustomSlashCommandRegistry::load(None, &config.workspace)
            .await
            .unwrap_or_else(|err| {
                warn!("failed to load custom slash commands: {err:#}");
                vtcode_core::prompts::CustomSlashCommandRegistry::default()
            });

    let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(128))); // 128-entry cache
    let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new())); // Session-scoped
    let search_metrics = Arc::new(RwLock::new(SearchMetrics::new())); // Track search performance

    let cache_dir = std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".vtcode").join("cache"))
        .unwrap_or_else(|| PathBuf::from(".vtcode/cache"));
    let approval_recorder = Arc::new(ApprovalRecorder::new(cache_dir));

    // HP-3: Cache tool definitions once for efficient reuse across turns
    let cached_tools = {
        let guard = tools.read().await;
        if guard.is_empty() {
            None
        } else {
            Some(Arc::new(guard.clone()))
        }
    };

    // Initialize dynamic context discovery directories
    if let Some(cfg) = vt_cfg
        && cfg.context.dynamic.enabled
        && let Err(err) = vtcode_core::context::initialize_dynamic_context(
            &config.workspace,
            &cfg.context.dynamic,
        )
        .await
    {
        warn!("Failed to initialize dynamic context directories: {}", err);
    }

    Ok(SessionState {
        session_bootstrap,
        provider_client,
        tool_registry,
        tools,
        cached_tools,
        conversation_history,
        decision_ledger,
        trajectory,
        base_system_prompt,
        full_auto_allowlist,
        async_mcp_manager,
        mcp_panel_state,
        tool_result_cache,
        tool_permission_cache,
        search_metrics,
        custom_prompts,
        custom_slash_commands,
        loaded_skills: active_skills_map,
        approval_recorder,
        safety_validator: Arc::new(RwLock::new(ToolCallSafetyValidator::new())),
        // Phase 4 Integration: Resilient execution components
        // Use agent.circuit_breaker config if available, otherwise use defaults
        circuit_breaker: Arc::new(vtcode_core::tools::circuit_breaker::CircuitBreaker::new(
            vtcode_config_circuit_breaker_to_core(vt_cfg, config),
        )),
        tool_health_tracker: Arc::new(vtcode_core::tools::health::ToolHealthTracker::new(50)), // Keep last 50 execution stats per tool
        rate_limiter: Arc::new(
            vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter::default(),
        ),
        validation_cache: Arc::new(
            vtcode_core::tools::validation_cache::ValidationCache::default(),
        ),
        telemetry: Arc::new(vtcode_core::core::telemetry::TelemetryManager::new()),
        autonomous_executor: {
            let executor = vtcode_core::tools::autonomous_executor::AutonomousExecutor::new();
            if let Some(cfg) = vt_cfg {
                let loop_limits: std::collections::HashMap<_, _> = cfg
                    .tools
                    .loop_thresholds
                    .iter()
                    .map(|(k, v)| (k.clone(), *v))
                    .collect();
                executor.configure_loop_limits(&loop_limits).await;
            }
            Arc::new(executor)
        },
        error_recovery: Arc::new(StdRwLock::new(
            vtcode_core::core::agent::error_recovery::ErrorRecoveryState::new(),
        )),
    })
}

#[allow(dead_code)]
pub(crate) async fn initialize_session_ui(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    session_state: &mut SessionState,
    resume_state: Option<&ResumeSession>,
    full_auto: bool,
) -> Result<SessionUISetup> {
    use crate::hooks::lifecycle::{LifecycleHookEngine, SessionEndReason, SessionStartTrigger};
    use vtcode_core::config::constants::ui;
    use vtcode_core::ui::tui::InlineEvent;

    let session_trigger = if resume_state.is_some() {
        SessionStartTrigger::Resume
    } else {
        SessionStartTrigger::Startup
    };
    let lifecycle_hooks = if let Some(vt) = vt_cfg {
        LifecycleHookEngine::new(config.workspace.clone(), &vt.hooks, session_trigger)?
    } else {
        None
    };

    let context_manager = super::context_manager::ContextManager::new(
        session_state.base_system_prompt.clone(),
        (),
        session_state.loaded_skills.clone(),
        vt_cfg.map(|cfg| cfg.agent.clone()),
    );

    let active_styles = theme::active_styles();
    let theme_spec = theme_from_styles(&active_styles);
    let default_placeholder = session_state
        .session_bootstrap
        .placeholder
        .clone()
        .or_else(|| Some(ui::CHAT_INPUT_PLACEHOLDER_BOOTSTRAP.to_string()));
    let follow_up_placeholder = if session_state.session_bootstrap.placeholder.is_none() {
        Some(ui::CHAT_INPUT_PLACEHOLDER_FOLLOW_UP.to_string())
    } else {
        None
    };
    let inline_rows = vt_cfg
        .as_ref()
        .map(|cfg| cfg.ui.inline_viewport_rows)
        .unwrap_or(ui::DEFAULT_INLINE_VIEWPORT_ROWS);

    // Set environment variable to indicate TUI mode is active
    unsafe {
        std::env::set_var("VTCODE_TUI_MODE", "1");
    }

    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let interrupt_callback: InlineEventCallback = {
        let state = ctrl_c_state.clone();
        let notify = ctrl_c_notify.clone();
        Arc::new(move |event: &InlineEvent| {
            if matches!(event, InlineEvent::Interrupt) {
                let _ = state.register_signal();
                notify.notify_waiters();
            }
        })
    };

    let pty_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    session_state
        .tool_registry
        .set_active_pty_sessions(pty_counter.clone());

    let session = spawn_session_with_prompts(
        theme_spec.clone(),
        default_placeholder.clone(),
        config.ui_surface,
        inline_rows,
        Some(interrupt_callback),
        Some(session_state.custom_prompts.clone()),
        Some(pty_counter.clone()),
        vt_cfg
            .map(|cfg| cfg.ui.keyboard_protocol.clone())
            .unwrap_or_default(),
    )
    .context("failed to launch inline session")?;
    let handle = session.clone_inline_handle();
    let highlight_config = vt_cfg
        .as_ref()
        .map(|cfg| cfg.syntax_highlighting.clone())
        .unwrap_or_default();

    transcript::set_inline_handle(Arc::new(handle.clone()));

    let mut ide_context_bridge = IdeContextBridge::from_env();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), highlight_config);

    // Create UI redraw batcher for optimizing UI updates with auto-flush
    let ui_redraw_batcher =
        crate::agent::runloop::unified::turn::utils::UIRedrawBatcher::with_auto_flush(
            handle.clone(),
        );

    // Load workspace files asynchronously in background.
    // See: https://ratatui.rs/faq/#when-should-i-use-tokio-and-async--await-
    // We spawn this task to avoid blocking the session setup on file loading.
    let workspace_for_indexer = config.workspace.clone();
    let workspace_for_palette = config.workspace.clone();
    let handle_for_indexer = handle.clone();
    let _file_palette_task = tokio::spawn(async move {
        match load_workspace_files(workspace_for_indexer).await {
            Ok(files) => {
                if !files.is_empty() {
                    handle_for_indexer.load_file_palette(files, workspace_for_palette);
                } else {
                    tracing::debug!("No files found in workspace for file palette");
                }
            }
            Err(err) => {
                tracing::warn!("Failed to load workspace files for file palette: {}", err);
            }
        }
    });
    // Note: Task is intentionally background-only; errors are logged but not propagated

    transcript::clear();

    // Handle resume session display
    if let Some(session) = resume_state {
        let ended_local = session
            .snapshot
            .ended_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M");

        let action = if session.is_fork {
            "Forking"
        } else {
            "Resuming"
        };

        renderer.line(
            vtcode_core::utils::ansi::MessageStyle::Info,
            &format!(
                "{} session {} · ended {} · {} messages",
                action,
                session.identifier,
                ended_local,
                session.message_count()
            ),
        )?;
        renderer.line(
            vtcode_core::utils::ansi::MessageStyle::Info,
            &format!("Previous archive: {}", session.path.display()),
        )?;

        if session.is_fork {
            renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                "Starting independent forked session",
            )?;
        }

        // Display full conversation history for context (compact but complete)
        if !session.history.is_empty() {
            renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                "Conversation history:",
            )?;

            for (idx, msg) in session.history.iter().enumerate() {
                let (style, role_label) = match msg.role {
                    uni::MessageRole::User => (vtcode_core::utils::ansi::MessageStyle::User, "You"),
                    uni::MessageRole::Assistant => (
                        vtcode_core::utils::ansi::MessageStyle::Response,
                        "Assistant",
                    ),
                    uni::MessageRole::Tool => {
                        (vtcode_core::utils::ansi::MessageStyle::ToolOutput, "Tool")
                    }
                    uni::MessageRole::System => {
                        (vtcode_core::utils::ansi::MessageStyle::Info, "System")
                    }
                };

                let tool_suffix = msg
                    .tool_call_id
                    .as_ref()
                    .map(|id| format!(" [tool_call_id: {}]", id))
                    .unwrap_or_default();

                renderer.line(
                    style,
                    &format!("  [{}] {}{}:", idx + 1, role_label, tool_suffix),
                )?;

                match &msg.content {
                    uni::MessageContent::Text(text) => {
                        let indented = indent_block(text, "  ");
                        renderer.line(style, &indented)?;
                    }
                    uni::MessageContent::Parts(parts) => {
                        renderer.line(style, &format!("  [content parts: {}]", parts.len()))?;
                    }
                }

                if idx + 1 < session.history.len() {
                    renderer.line(style, "")?;
                }
            }
        }
        renderer.line_if_not_empty(vtcode_core::utils::ansi::MessageStyle::Output)?;
    }

    // Setup session archive
    let workspace_label = config
        .workspace
        .file_name()
        .and_then(|component| component.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "workspace".to_string());
    let workspace_path = config.workspace.to_string_lossy().into_owned();
    let provider_label = if config.provider.trim().is_empty() {
        session_state.provider_client.name().to_string()
    } else {
        config.provider.clone()
    };
    let header_provider_label = provider_label.clone();

    // Setup checkpoint manager
    let mut checkpoint_config = SnapshotConfig::new(config.workspace.clone());
    checkpoint_config.enabled = config.checkpointing_enabled;
    checkpoint_config.storage_dir = config.checkpointing_storage_dir.clone();
    checkpoint_config.max_snapshots = config.checkpointing_max_snapshots;
    checkpoint_config.max_age_days = config.checkpointing_max_age_days;

    let checkpoint_manager = match SnapshotManager::new(checkpoint_config) {
        Ok(manager) => Some(manager),
        Err(err) => {
            warn!("Failed to initialize checkpoint manager: {}", err);
            None
        }
    };

    let mut session_archive_error: Option<String> = None;
    let session_archive = if let Some(resume) = resume_state {
        if resume.is_fork {
            // Fork: create new archive from source snapshot with custom ID
            let custom_id = resume
                .identifier
                .strip_prefix("forked-")
                .map(|s| s.to_string());
            match SessionArchive::fork(&resume.snapshot, custom_id).await {
                Ok(archive) => Some(archive),
                Err(err) => {
                    session_archive_error = Some(err.to_string());
                    None
                }
            }
        } else {
            // Resume: create normal archive (resume doesn't modify original)
            let archive_metadata = SessionArchiveMetadata::new(
                workspace_label,
                workspace_path,
                config.model.clone(),
                provider_label,
                config.theme.clone(),
                config.reasoning_effort.as_str().to_string(),
            );
            match SessionArchive::new(archive_metadata, None).await {
                Ok(archive) => Some(archive),
                Err(err) => {
                    session_archive_error = Some(err.to_string());
                    None
                }
            }
        }
    } else {
        // New session: create normal archive
        let archive_metadata = SessionArchiveMetadata::new(
            workspace_label,
            workspace_path,
            config.model.clone(),
            provider_label,
            config.theme.clone(),
            config.reasoning_effort.as_str().to_string(),
        );
        match SessionArchive::new(archive_metadata, None).await {
            Ok(archive) => Some(archive),
            Err(err) => {
                session_archive_error = Some(err.to_string());
                None
            }
        }
    };

    if let (Some(hooks), Some(archive)) = (&lifecycle_hooks, session_archive.as_ref()) {
        hooks
            .update_transcript_path(Some(archive.path().to_path_buf()))
            .await;
    }

    // Run session start hooks
    if let Some(hooks) = &lifecycle_hooks {
        match hooks.run_session_start().await {
            Ok(outcome) => {
                render_hook_messages(&mut renderer, &outcome.messages)?;
                for context in outcome.additional_context {
                    if !context.trim().is_empty() {
                        session_state
                            .conversation_history
                            .push(uni::Message::system(context));
                    }
                }
            }
            Err(err) => {
                renderer.line(
                    vtcode_core::utils::ansi::MessageStyle::Error,
                    &format!("Failed to run session start hooks: {}", err),
                )?;
            }
        }
    }

    // Display full-auto mode information if enabled
    if full_auto && let Some(allowlist) = session_state.full_auto_allowlist.as_ref() {
        if allowlist.is_empty() {
            renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                "Full-auto mode enabled with no tool permissions; tool calls will be skipped.",
            )?;
        } else {
            renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                &format!(
                    "Full-auto mode enabled. Permitted tools: {}",
                    allowlist.join(", ")
                ),
            )?;
        }
    }

    // Report MCP background initialization status
    if let Some(mcp_manager) = &session_state.async_mcp_manager {
        let mcp_status = mcp_manager.get_status().await;
        if mcp_status.is_initializing() {
            renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                "MCP is still initializing in the background...",
            )?;
        }
    }

    handle.set_theme(theme_spec.clone());
    apply_prompt_style(&handle);
    handle.set_placeholder(default_placeholder.clone());

    let reasoning_label = vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.reasoning_effort.as_str().to_string())
        .unwrap_or_else(|| config.reasoning_effort.as_str().to_string());

    // Render session banner
    render_session_banner(
        &mut renderer,
        config,
        &session_state.session_bootstrap,
        &config.model,
        &reasoning_label,
    )?;

    // Handle IDE context
    if let Some(bridge) = ide_context_bridge.as_mut() {
        match bridge.snapshot() {
            Ok(Some(context)) => {
                session_state
                    .conversation_history
                    .push(uni::Message::system(context));
            }
            Ok(None) => {}
            Err(err) => {
                warn!("Failed to update IDE context snapshot: {}", err);
            }
        }
    }

    // Setup header context
    let mode_label = match (config.ui_surface, full_auto) {
        (vtcode_core::config::types::UiSurfacePreference::Inline, true) => "auto".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Inline, false) => "inline".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Alternate, _) => "alt".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Auto, true) => "auto".to_string(),
        (vtcode_core::config::types::UiSurfacePreference::Auto, false) => "std".to_string(),
    };
    let header_context = build_inline_header_context(
        config,
        &session_state.session_bootstrap,
        header_provider_label,
        config.model.clone(),
        mode_label,
        reasoning_label.clone(),
    )
    .await?;
    handle.set_header_context(header_context);

    // Handle session archive error display
    if let Some(message) = session_archive_error {
        renderer.line(
            vtcode_core::utils::ansi::MessageStyle::Info,
            &format!("Session archiving disabled: {}", message),
        )?;
        renderer.line_if_not_empty(vtcode_core::utils::ansi::MessageStyle::Output)?;
    }

    let next_checkpoint_turn = checkpoint_manager
        .as_ref()
        .and_then(|manager| manager.next_turn_number().ok())
        .unwrap_or(1);

    Ok(SessionUISetup {
        renderer,
        session,
        handle,
        ctrl_c_state,
        ctrl_c_notify,
        checkpoint_manager,
        session_archive,
        lifecycle_hooks,
        session_end_reason: SessionEndReason::Completed,
        context_manager,
        default_placeholder,
        follow_up_placeholder,
        next_checkpoint_turn,
        ui_redraw_batcher,
    })
}

pub(crate) fn spawn_signal_handler(
    ctrl_c_state: Arc<CtrlCState>,
    ctrl_c_notify: Arc<Notify>,
    async_mcp_manager: Option<Arc<AsyncMcpManager>>,
    cancel_token: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    let signal = ctrl_c_state.register_signal();
                    ctrl_c_notify.notify_waiters();

                    // Shutdown MCP client on interrupt using async manager
                    if let Some(mcp_manager) = &async_mcp_manager
                        && let Err(e) = mcp_manager.shutdown().await
                    {
                        let error_msg = e.to_string();
                        if error_msg.contains("EPIPE")
                            || error_msg.contains("Broken pipe")
                            || error_msg.contains("write EPIPE")
                        {
                            tracing::debug!(
                                "MCP client shutdown encountered pipe errors during interrupt (normal): {}",
                                e
                            );
                        } else {
                            tracing::warn!("Failed to shutdown MCP client on interrupt: {}", e);
                        }
                    }

                    if matches!(signal, super::state::CtrlCSignal::Exit) {
                        // Emergency terminal cleanup on forced exit
                        // This ensures ANSI sequences don't leak to the terminal on double Ctrl+C
                        emergency_terminal_cleanup();
                        break;
                    }
                }
                _ = cancel_token.cancelled() => {
                    break;
                }
            }
        }
    })
}

/// Emergency cleanup for terminal state when process is about to exit
/// This is called when double Ctrl+C is pressed to ensure the terminal
/// is left in a clean state even if normal finalization doesn't run.
fn emergency_terminal_cleanup() {
    use ratatui::crossterm::{
        execute,
        terminal::{LeaveAlternateScreen, disable_raw_mode},
    };
    use std::io::{self, Write};

    let mut stderr = io::stderr();

    // Clear current line to remove any echoed ^C characters from rapid Ctrl+C presses
    // \r returns to start of line, \x1b[K clears to end of line
    let _ = stderr.write_all(b"\r\x1b[K");

    // Attempt to leave alternate screen - this is the most critical
    // operation to prevent ANSI escape codes from leaking
    let _ = execute!(stderr, LeaveAlternateScreen);

    // Disable raw mode
    let _ = disable_raw_mode();

    // Flush stderr to ensure commands are processed
    let _ = stderr.flush();

    // Brief delay to allow terminal to process cleanup commands
    std::thread::sleep(std::time::Duration::from_millis(10));
}

fn build_single_mcp_tool_definition(tool: &McpToolInfo) -> uni::ToolDefinition {
    let parameters = vtcode_core::llm::providers::gemini::sanitize_function_parameters(
        tool.input_schema.clone(),
    );
    let description = if tool.description.trim().is_empty() {
        format!("MCP tool from provider '{}'", tool.provider)
    } else {
        format!(
            "MCP tool from provider '{}': {}",
            tool.provider, tool.description
        )
    };

    uni::ToolDefinition::function(format!("mcp_{}", tool.name), description, parameters)
}

pub fn build_mcp_tool_definitions(tools: &[McpToolInfo]) -> Vec<uni::ToolDefinition> {
    tools.iter().map(build_single_mcp_tool_definition).collect()
}

/// Analyze user query and log task complexity estimation
#[allow(dead_code)]
fn estimate_and_log_task_complexity(query: &str) -> TaskComplexity {
    if query.is_empty() {
        return TaskComplexity::Moderate;
    }

    // Simple heuristic for task complexity based on query length and keywords
    let lower = query.to_lowercase();
    let complexity = if query.len() > 200
        || lower.contains("refactor")
        || lower.contains("debug")
        || lower.contains("design")
        || lower.contains("architecture")
        || lower.contains("multiple")
    {
        TaskComplexity::Complex
    } else if query.len() > 100
        || lower.contains("fix")
        || lower.contains("modify")
        || lower.contains("implement")
    {
        TaskComplexity::Moderate
    } else {
        TaskComplexity::Simple
    };

    debug!("Task complexity: {:?} (estimated)", complexity);

    // Log some basic detections
    if lower.contains("refactor") {
        debug!("Detected: Refactoring work");
    }
    if lower.contains("debug") || lower.contains("fix") {
        debug!("Detected: Debugging/troubleshooting");
    }
    if lower.contains("multiple") {
        debug!("Detected: Multi-file changes");
    }
    if lower.contains("explain") {
        debug!("Detected: Explanation/documentation needed");
    }

    complexity
}

#[cfg(test)]
mod tests {
    // Intentionally empty test module
}
