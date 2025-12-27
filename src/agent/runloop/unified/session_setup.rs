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
use tokio::sync::{Notify, RwLock};
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};
use vtcode_core::tools::ApprovalRecorder;

use super::async_mcp_manager::AsyncMcpManager;
use super::palettes::apply_prompt_style;
use super::prompts::read_system_prompt;
use super::state::CtrlCState;
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::context::{ContextTrimConfig, load_context_trim_config};
use crate::agent::runloop::ui::{build_inline_header_context, render_session_banner};
use crate::agent::runloop::unified::turn::utils::render_hook_messages;
use crate::agent::runloop::unified::turn::workspace::load_workspace_files;
use crate::hooks::lifecycle::LifecycleHookEngine;
use crate::ide_context::IdeContextBridge;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::snapshots::{SnapshotConfig, SnapshotManager};
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::pruning_decisions::PruningDecisionLedger;
use vtcode_core::core::token_budget::{
    TokenBudgetConfig as RuntimeTokenBudgetConfig, TokenBudgetManager,
};
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::{TokenCounter, factory::create_provider_with_config, provider as uni};
use vtcode_core::mcp::{McpClient, McpToolInfo};
use vtcode_core::models::ModelId;
use vtcode_core::prompts::CustomPromptRegistry;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::build_function_declarations_with_mode;
use vtcode_core::tools::{SearchMetrics, ToolResultCache};
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::{
    InlineEventCallback, InlineHandle, InlineSession, spawn_session_with_prompts, theme_from_styles,
};
use vtcode_core::ui::user_confirmation::TaskComplexity;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::session_archive::{SessionArchive, SessionArchiveMetadata};
use vtcode_core::utils::transcript;

pub(crate) struct SessionState {
    pub session_bootstrap: SessionBootstrap,
    pub provider_client: Box<dyn uni::LLMProvider>,
    pub tool_registry: ToolRegistry,
    pub tools: Arc<RwLock<Vec<uni::ToolDefinition>>>,
    /// Cached tool definitions for efficient reuse across turns (HP-3 optimization)
    pub cached_tools: Option<Arc<Vec<uni::ToolDefinition>>>,
    pub trim_config: ContextTrimConfig,
    pub conversation_history: Vec<uni::Message>,
    pub decision_ledger: Arc<RwLock<DecisionTracker>>,
    pub pruning_ledger: Arc<RwLock<PruningDecisionLedger>>,
    pub trajectory: TrajectoryLogger,
    pub base_system_prompt: String,
    pub full_auto_allowlist: Option<Vec<String>>,
    pub async_mcp_manager: Option<Arc<AsyncMcpManager>>,
    pub mcp_panel_state: mcp_events::McpPanelState,
    pub token_budget: Arc<TokenBudgetManager>,
    pub token_budget_enabled: bool,
    pub token_counter: Arc<RwLock<TokenCounter>>,
    pub tool_result_cache: Arc<RwLock<ToolResultCache>>,
    pub tool_permission_cache: Arc<RwLock<ToolPermissionCache>>,
    #[allow(dead_code)]
    pub search_metrics: Arc<RwLock<SearchMetrics>>,

    pub custom_prompts: CustomPromptRegistry,

    /// Skills loaded in current session (name -> Skill mapping)
    pub loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    pub approval_recorder: Arc<ApprovalRecorder>,
    pub safety_validator: Arc<RwLock<ToolCallSafetyValidator>>,
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
}

async fn build_conversation_history_from_resume(
    resume: Option<&ResumeSession>,
    token_budget: &TokenBudgetManager,
) -> Vec<uni::Message> {
    if let Some(progress) = resume.and_then(|session| session.snapshot.progress.clone())
        && let Some(usage) = progress.token_usage
    {
        token_budget.restore_stats(usage).await;
    }

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
    let todo_planning_enabled = vt_cfg
        .map(|cfg| cfg.agent.todo_planning_mode)
        .unwrap_or(true);

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

    let mut full_auto_allowlist = None;

    let base_declarations =
        build_function_declarations_with_mode(todo_planning_enabled, tool_documentation_mode);
    let mut tool_definitions: Vec<uni::ToolDefinition> = base_declarations
        .into_iter()
        .map(|decl| {
            uni::ToolDefinition::function(
                decl.name,
                decl.description,
                vtcode_core::llm::providers::gemini::sanitize_function_parameters(decl.parameters),
            )
        })
        .collect();

    // Add GPT-5.1 specific tools if the model supports them
    if let Ok(model_id) = ModelId::from_str(&config.model)
        && model_id.is_gpt51_variant()
    {
        // Add apply_patch tool for GPT-5.1's structured diff editing
        tool_definitions.push(uni::ToolDefinition::apply_patch(
                "Apply structured diffs to modify files. Use this tool to create, update, or delete file content using unified diff format. The tool enables iterative, multi-step code editing workflows by applying patches and reporting results back.".to_string()
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
    let mut discovered_skill_adapters = Vec::new();
    let mut discovered_skills_map = HashMap::new();
    
    let mut skill_discovery = vtcode_core::skills::discovery::SkillDiscovery::new();
    match skill_discovery.discover_all(&config.workspace).await {
         Ok(result) => {
             info!("Discovered {} skills and {} CLI tools", result.skills.len(), result.tools.len());
             
             // Process Traditional Skills (Markdown)
             for skill_ctx in result.skills {
                // Create lightweight skill for prompt generation
                if let Ok(lightweight_skill) = vtcode_core::skills::types::Skill::new(
                    skill_ctx.manifest().clone(),
                    skill_ctx.path().clone(),
                    String::new(), // Placeholder instructions for prompt-only listing
                ) {
                    discovered_skills_map.insert(lightweight_skill.name().to_string(), lightweight_skill);
                }
             }

             // Process CLI tools
             let mut dormant_tool_defs = HashMap::new();
             for tool_config in result.tools {
                 match vtcode_core::skills::cli_bridge::CliToolBridge::new(tool_config) {
                     Ok(bridge) => {
                         match bridge.to_skill() {
                            Ok(skill) => {
                                discovered_skills_map.insert(skill.name().to_string(), skill.clone());
                                let adapter = vtcode_core::skills::executor::SkillToolAdapter::new(skill);
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

             // Note: We DO NOT push to `tools` here. We rely on LoadSkillTool to activate them.
         }
         Err(e) => warn!("Skill discovery failed: {}", e),
    }

    let trim_config = load_context_trim_config(vt_cfg);
    let context_features = vt_cfg.map(|cfg| &cfg.context);
    let token_budget_enabled = context_features
        .map(|cfg| cfg.token_budget.enabled)
        .unwrap_or(true);
    let max_context_tokens = trim_config.max_tokens;
    let mut token_budget_config = RuntimeTokenBudgetConfig::for_model(
        context_features
            .map(|cfg| cfg.token_budget.model.as_str())
            .unwrap_or("gpt-5-nano"),
        max_context_tokens,
    );
    if let Some(cfg) = context_features {
        token_budget_config.warning_threshold = cfg.token_budget.warning_threshold;
        token_budget_config.alert_threshold = cfg.token_budget.alert_threshold;
        token_budget_config.detailed_tracking = cfg.token_budget.detailed_tracking;
        token_budget_config.tokenizer_id = cfg.token_budget.tokenizer.clone();
    }
    let token_budget = Arc::new(TokenBudgetManager::new(token_budget_config));

    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let pruning_ledger = Arc::new(RwLock::new(PruningDecisionLedger::new()));

    let conversation_history = build_conversation_history_from_resume(resume, &token_budget).await;
    let trajectory = build_trajectory_logger(&config.workspace, vt_cfg);
    let mut base_system_prompt = read_system_prompt(
        &config.workspace,
        session_bootstrap.prompt_addendum.as_deref(),
    )
    .await;

    // Register ListSkills tool
    let list_skills_tool = vtcode_core::tools::skills::ListSkillsTool::new(
        Arc::new(RwLock::new(discovered_skills_map.clone()))
    );
    let list_skills_reg = vtcode_core::tools::registry::ToolRegistration::from_tool_instance(
        "list_skills",
        vtcode_core::config::types::CapabilityLevel::General,
        list_skills_tool
    );
    if let Err(e) = tool_registry.register_tool(list_skills_reg) {
        warn!("Failed to register list_skills tool: {}", e);
    }
    // Add list_skills to tool definitions
    {
         let mut tools_guard = tools.write().await;
         tools_guard.push(uni::ToolDefinition::function(
             "list_skills".to_string(),
             "List all available skills that can be loaded. Use this to discover capabilities before loading them.".to_string(),
             serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            })
         ));
    }

    // Initialize MCP panel state
    let mcp_panel_state = if let Some(cfg) = vt_cfg {
        mcp_events::McpPanelState::new(cfg.mcp.ui.max_events, cfg.mcp.enabled)
    } else {
        mcp_events::McpPanelState::default()
    };

    let pty_config = vt_cfg.map(|cfg| cfg.pty).unwrap_or_default();

    let mut tool_registry =
        ToolRegistry::new_with_features(config.workspace.clone(), todo_planning_enabled).await;
    tool_registry.initialize_async().await?;
    if let Some(cfg) = vt_cfg {
        tool_registry.apply_commands_config(&cfg.commands);
        tool_registry.apply_timeout_policy(&cfg.timeouts);
        if let Err(err) = tool_registry.apply_config_policies(&cfg.tools).await {
            eprintln!(
                "Warning: Failed to apply tool policies from config: {}",
                err
            );
        }

        // Add MCP client to tool registry if available from async manager
        if cfg.mcp.enabled
            && let Some(ref manager) = async_mcp_manager
        {
            // If we polled earlier and grabbed a ready client, prefer that.
            let status = manager.get_status().await;
            if let super::async_mcp_manager::McpInitStatus::Ready { client } = &status {
                tool_registry = tool_registry.with_mcp_client(Arc::clone(client));
                if let Err(err) = tool_registry.refresh_mcp_tools().await {
                    warn!("Failed to refresh MCP tools: {}", err);

                    // Log which providers are configured to help with debugging
                    if !cfg.mcp.providers.is_empty() {
                        let provider_names: Vec<String> = cfg
                            .mcp
                            .providers
                            .iter()
                            .map(|p| format!("{} (enabled: {})", p.name, p.enabled))
                            .collect();
                        info!("Configured MCP providers: [{}]", provider_names.join(", "));
                    }
                }
            } else {
                debug!(
                    "MCP client not ready during startup; it will be available later if it finishes initializing"
                );

                // Log the status for debugging
                match &status {
                    super::async_mcp_manager::McpInitStatus::Error { message } => {
                        warn!("MCP initialization failed: {}", message);
                    }
                    super::async_mcp_manager::McpInitStatus::Initializing { progress } => {
                        info!("MCP still initializing: {}", progress);
                    }
                    super::async_mcp_manager::McpInitStatus::Disabled => {
                        info!("MCP is disabled");
                    }
                    super::async_mcp_manager::McpInitStatus::Ready { .. } => {
                        // This case is handled above
                    }
                };
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
                .unwrap_or_default();
            full_auto_allowlist = Some(allowlist);
        }
    }

    // Register discovered skills in registry
    for adapter in discovered_skill_adapters {
         use vtcode_core::tools::traits::Tool;
         let name = adapter.name(); 
         let reg = vtcode_core::tools::registry::ToolRegistration::from_tool_instance(
             name,
             vtcode_core::config::types::CapabilityLevel::General,
             adapter
         );
         if let Err(e) = tool_registry.register_tool(reg) {
             warn!("Failed to register skill tool {}: {}", name, e);
         }
    }

    // Register LoadSkill tool
    let load_skill_tool = vtcode_core::tools::skills::LoadSkillTool::new(
        Arc::new(RwLock::new(discovered_skills_map.clone())),
        dormant_tool_defs,
        Some(tools.clone()) // Pass shared tools list for dynamic activation
    );
    let load_skill_reg = vtcode_core::tools::registry::ToolRegistration::from_tool_instance(
        "load_skill",
        vtcode_core::config::types::CapabilityLevel::General,
        load_skill_tool
    );
    if let Err(e) = tool_registry.register_tool(load_skill_reg) {
        warn!("Failed to register load_skill tool: {}", e);
    }
    // Add load_skill to tool definitions
    {
         let mut tools_guard = tools.write().await;
         tools_guard.push(uni::ToolDefinition::function(
             "load_skill".to_string(),
             "Load detailed instructions for a specific skill and activate its associated tools. Use this when you want to understand/use a skill listed in your system prompt.".to_string(),
             serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The name of the skill to load"
                    }
                },
                "required": ["name"]
            })
         ));
    }

    let custom_prompts = CustomPromptRegistry::load(
        vt_cfg.map(|cfg| &cfg.agent.custom_prompts),
        &config.workspace,
    )
    .await
    .unwrap_or_else(|err| {
        warn!("failed to load custom prompts: {err:#}");
        CustomPromptRegistry::default()
    });

    let token_counter = Arc::new(RwLock::new(TokenCounter::new()));
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

    Ok(SessionState {
        session_bootstrap,
        provider_client,
        tool_registry,
        tools,
        cached_tools,
        trim_config,
        conversation_history,
        decision_ledger,
        pruning_ledger,
        trajectory,
        base_system_prompt,
        full_auto_allowlist,
        async_mcp_manager,
        mcp_panel_state,
        token_budget,
        token_budget_enabled,
        token_counter,
        tool_result_cache,
        tool_permission_cache,
        search_metrics,
        custom_prompts,
        loaded_skills: Arc::new(RwLock::new(discovered_skills_map)),
        approval_recorder,
        safety_validator: Arc::new(RwLock::new(ToolCallSafetyValidator::new())),
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
        session_state.trim_config,
        session_state.token_budget.clone(),
        session_state.token_budget_enabled,
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
    let show_timeline_pane = vt_cfg
        .as_ref()
        .map(|cfg| cfg.ui.show_timeline_pane)
        .unwrap_or(ui::INLINE_SHOW_TIMELINE_PANE);

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

    let session = spawn_session_with_prompts(
        theme_spec.clone(),
        default_placeholder.clone(),
        config.ui_surface,
        inline_rows,
        show_timeline_pane,
        Some(interrupt_callback),
        Some(session_state.custom_prompts.clone()),
        None,
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
                        for line in text.lines() {
                            renderer.line(style, &format!("    {}", line))?;
                        }
                    }
                    uni::MessageContent::Parts(parts) => {
                        renderer.line(style, &format!("    [content parts: {}]", parts.len()))?;
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

    // Connect PTY session tracking from tool registry to session state
    let pty_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    session_state
        .tool_registry
        .set_active_pty_sessions(pty_counter.clone());

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

    // Connect PTY session tracking from tool registry to session state
    let pty_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    session_state
        .tool_registry
        .set_active_pty_sessions(pty_counter.clone());

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
    })
}

pub(crate) fn spawn_signal_handler(
    ctrl_c_state: Arc<CtrlCState>,
    ctrl_c_notify: Arc<Notify>,
    async_mcp_manager: Option<Arc<AsyncMcpManager>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if tokio::signal::ctrl_c().await.is_err() {
                break;
            }

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
                    eprintln!(
                        "Info: MCP client shutdown encountered pipe errors during interrupt (normal): {}",
                        e
                    );
                } else {
                    eprintln!("Warning: Failed to shutdown MCP client on interrupt: {}", e);
                }
            }

            if matches!(signal, super::state::CtrlCSignal::Exit) {
                break;
            }
        }
    })
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
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;
    use vtcode_core::core::token_budget::TokenUsageStats;
    use vtcode_core::llm::provider::{Message, MessageRole};
    use vtcode_core::utils::session_archive::{
        SessionArchiveMetadata, SessionMessage, SessionProgress, SessionSnapshot,
    };

    #[tokio::test]
    async fn resume_restores_token_budget_stats() {
        let budget_cfg = RuntimeTokenBudgetConfig::for_model("test-model", 100);
        let budget = TokenBudgetManager::new(budget_cfg);

        let usage = TokenUsageStats {
            total_tokens: 42,
            ..TokenUsageStats::new()
        };

        let session_messages = vec![SessionMessage::new(MessageRole::Assistant, "hi")];
        let snapshot = SessionSnapshot {
            metadata: SessionArchiveMetadata::new(
                "ws", "/tmp/ws", "model", "provider", "theme", "medium",
            ),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            total_messages: 1,
            distinct_tools: vec!["tool_a".to_string()],
            transcript: Vec::new(),
            messages: session_messages.clone(),
            progress: Some(SessionProgress {
                turn_number: 2,
                recent_messages: session_messages.clone(),
                tool_summaries: vec!["tool_a".to_string()],
                token_usage: Some(usage.clone()),
                max_context_tokens: Some(100),
                loaded_skills: Vec::new(),
            }),
        };

        let resume = ResumeSession {
            identifier: "resume-1".to_string(),
            snapshot,
            history: session_messages.iter().map(Message::from).collect(),
            path: PathBuf::new(),
            is_fork: false,
        };

        let history = build_conversation_history_from_resume(Some(&resume), &budget).await;
        let restored = budget.get_stats().await;

        assert_eq!(history.len(), 1);
        assert_eq!(restored.total_tokens, usage.total_tokens);
    }
}
