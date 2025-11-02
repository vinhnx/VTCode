use crate::agent::runloop::mcp_events;
use crate::agent::runloop::sandbox::SandboxCoordinator;
use crate::agent::runloop::telemetry::build_trajectory_logger;
use crate::agent::runloop::welcome::{SessionBootstrap, prepare_session_bootstrap};
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};

use super::async_mcp_manager::AsyncMcpManager;
use super::prompts::read_system_prompt;
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::context::{ContextTrimConfig, load_context_trim_config};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::context_curator::{
    ContextCurationConfig as RuntimeContextCurationConfig, ContextCurator,
};
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::token_budget::{
    TokenBudgetConfig as RuntimeTokenBudgetConfig, TokenBudgetManager,
};
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::{factory::create_provider_with_config, provider as uni};
use vtcode_core::mcp_client::{McpClient, McpToolInfo};
use vtcode_core::models::ModelId;
use vtcode_core::prompts::CustomPromptRegistry;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::build_function_declarations_with_mode;

pub(crate) struct SessionState {
    pub session_bootstrap: SessionBootstrap,
    pub provider_client: Box<dyn uni::LLMProvider>,
    pub tool_registry: ToolRegistry,
    pub tools: Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub trim_config: ContextTrimConfig,
    pub conversation_history: Vec<uni::Message>,
    pub decision_ledger: Arc<RwLock<DecisionTracker>>,
    pub trajectory: TrajectoryLogger,
    pub base_system_prompt: String,
    pub full_auto_allowlist: Option<Vec<String>>,
    pub async_mcp_manager: Option<Arc<AsyncMcpManager>>,
    pub mcp_panel_state: mcp_events::McpPanelState,
    pub token_budget: Arc<TokenBudgetManager>,
    pub token_budget_enabled: bool,
    pub curator: ContextCurator,
    pub custom_prompts: CustomPromptRegistry,
    pub sandbox: SandboxCoordinator,
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

    // Create async MCP manager if enabled
    let async_mcp_manager = if let Some(cfg) = vt_cfg {
        if cfg.mcp.enabled {
            info!(
                "Setting up async MCP client with {} providers",
                cfg.mcp.providers.len()
            );

            let manager =
                AsyncMcpManager::new(cfg.mcp.clone(), Arc::new(|_event: mcp_events::McpEvent| {}));
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
        debug!("No VTCode config provided");
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
    )
    .context("Failed to initialize provider client")?;

    let mut full_auto_allowlist = None;

    let base_declarations = build_function_declarations_with_mode(todo_planning_enabled);
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
        token_budget_config.compaction_threshold = cfg.token_budget.compaction_threshold;
        token_budget_config.detailed_tracking = cfg.token_budget.detailed_tracking;
        token_budget_config.tokenizer_id = cfg.token_budget.tokenizer.clone();
    }
    let token_budget = Arc::new(TokenBudgetManager::new(token_budget_config));

    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let mut curator_config = RuntimeContextCurationConfig::default();
    if let Some(cfg) = context_features {
        curator_config.enabled = cfg.curation.enabled;
        curator_config.max_tokens_per_turn = cfg.curation.max_tokens_per_turn;
        curator_config.preserve_recent_messages = cfg.curation.preserve_recent_messages;
        curator_config.max_tool_descriptions = cfg.curation.max_tool_descriptions;
        curator_config.include_ledger = cfg.curation.include_ledger && cfg.ledger.enabled;
        curator_config.ledger_max_entries = cfg.curation.ledger_max_entries;
        curator_config.include_recent_errors = cfg.curation.include_recent_errors;
        curator_config.max_recent_errors = cfg.curation.max_recent_errors;
    }
    let curator = ContextCurator::new(
        curator_config,
        Arc::clone(&token_budget),
        Arc::clone(&decision_ledger),
    );
    let conversation_history: Vec<uni::Message> = resume
        .map(|session| session.history.clone())
        .unwrap_or_default();
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

    let mut tool_registry =
        ToolRegistry::new_with_features(config.workspace.clone(), todo_planning_enabled).await;
    tool_registry.initialize_async().await?;
    if let Some(cfg) = vt_cfg {
        tool_registry.apply_commands_config(&cfg.commands);
        if let Err(err) = tool_registry.apply_config_policies(&cfg.tools).await {
            eprintln!(
                "Warning: Failed to apply tool policies from config: {}",
                err
            );
        }

        // Add MCP client to tool registry if available from async manager
        if cfg.mcp.enabled {
            if let Some(ref manager) = async_mcp_manager {
                // If we polled earlier and grabbed a ready client, prefer that.
                let status = manager.get_status().await;
                if let super::async_mcp_manager::McpInitStatus::Ready { client } = &status {
                    tool_registry = tool_registry.with_mcp_client(Arc::clone(client));
                    if let Err(err) = tool_registry.refresh_mcp_tools().await {
                        warn!("Failed to refresh MCP tools: {}", err);
                    }
                } else {
                    debug!(
                        "MCP client not ready during startup; it will be available later if it finishes initializing"
                    );
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
                .unwrap_or_default();
            full_auto_allowlist = Some(allowlist);
        }
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

    let sandbox = SandboxCoordinator::new(config.workspace.clone());

    Ok(SessionState {
        session_bootstrap,
        provider_client,
        tool_registry,
        tools,
        trim_config,
        conversation_history,
        decision_ledger,
        trajectory,
        base_system_prompt,
        full_auto_allowlist,
        async_mcp_manager,
        mcp_panel_state,
        token_budget,
        token_budget_enabled,
        curator,
        custom_prompts,
        sandbox,
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

pub(crate) fn build_mcp_tool_definitions(tools: &[McpToolInfo]) -> Vec<uni::ToolDefinition> {
    tools.iter().map(build_single_mcp_tool_definition).collect()
}
