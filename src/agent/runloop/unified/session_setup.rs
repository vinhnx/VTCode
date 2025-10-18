use anyhow::{Context, Result};
use tracing::{debug, error, info, warn};

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
use vtcode_core::models::ModelId;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::build_function_declarations_with_mode;

use super::prompts::read_system_prompt;
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::context::ContextTrimConfig;
use crate::agent::runloop::context::load_context_trim_config;
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::telemetry::build_trajectory_logger;
use crate::agent::runloop::welcome::{SessionBootstrap, prepare_session_bootstrap};
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_core::mcp_client::McpClient;

pub(crate) struct SessionState {
    pub session_bootstrap: SessionBootstrap,
    pub provider_client: Box<dyn uni::LLMProvider>,
    pub tool_registry: ToolRegistry,
    pub tools: Vec<uni::ToolDefinition>,
    pub trim_config: ContextTrimConfig,
    pub conversation_history: Vec<uni::Message>,
    pub decision_ledger: Arc<RwLock<DecisionTracker>>,
    pub trajectory: TrajectoryLogger,
    pub base_system_prompt: String,
    pub full_auto_allowlist: Option<Vec<String>>,
    pub mcp_client: Option<Arc<McpClient>>,
    pub mcp_panel_state: mcp_events::McpPanelState,
    pub token_budget: Arc<TokenBudgetManager>,
    pub token_budget_enabled: bool,
    pub curator: ContextCurator,
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

    // Initialize MCP client if enabled and capture any errors for the welcome message
    let (mcp_client, mcp_error) = if let Some(cfg) = vt_cfg {
        if cfg.mcp.enabled {
            info!(
                "Initializing MCP client with {} providers",
                cfg.mcp.providers.len()
            );
            let mut client = McpClient::new(cfg.mcp.clone());
            match tokio::time::timeout(tokio::time::Duration::from_secs(30), client.initialize())
                .await
            {
                Ok(Ok(())) => {
                    info!("MCP client initialized successfully");

                    // Note: We don't call cleanup_dead_providers() here because no connections
                    // have been established yet during initialization. Cleanup will happen
                    // naturally when connections are first established and fail.

                    (Some(Arc::new(client)), None)
                }
                Ok(Err(e)) => {
                    let error_msg = e.to_string();
                    let mcp_error = if error_msg.contains("No such process")
                        || error_msg.contains("ESRCH")
                        || error_msg.contains("EPIPE")
                        || error_msg.contains("Broken pipe")
                        || error_msg.contains("write EPIPE")
                    {
                        debug!(
                            "MCP client initialization failed due to process/pipe issues (normal during shutdown), continuing without MCP: {}",
                            e
                        );
                        Some(format!("MCP server startup failed: {}", e))
                    } else {
                        warn!("MCP client initialization failed: {}", e);
                        Some(format!("MCP initialization error: {}", e))
                    };
                    (None, mcp_error)
                }
                Err(_) => {
                    error!(
                        "MCP client initialization timed out after 30 seconds, continuing without MCP"
                    );
                    let mcp_error =
                        Some("MCP initialization timed out after 30 seconds".to_string());
                    (None, mcp_error)
                }
            }
        } else {
            debug!("MCP is disabled in configuration");
            (None, None)
        }
    } else {
        debug!("No VTCode config provided");
        (None, None)
    };

    let session_bootstrap = prepare_session_bootstrap(config, vt_cfg, mcp_error);
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

    let mut declarations = build_function_declarations_with_mode(todo_planning_enabled);

    // Add MCP tools if available
    if let Some(mcp_client) = &mcp_client {
        debug!("Discovering MCP tools...");
        if let Ok(mcp_tools) = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async { mcp_client.list_tools().await })
        }) {
            info!("Found {} MCP tools", mcp_tools.len());
            for mcp_tool in mcp_tools {
                debug!("Registering MCP tool: {}", mcp_tool.name);
                declarations.push(vtcode_core::gemini::FunctionDeclaration {
                    name: format!("mcp_{}", mcp_tool.name),
                    description: format!(
                        "MCP tool from provider '{}': {}",
                        mcp_tool.provider, mcp_tool.description
                    ),
                    parameters: mcp_tool.input_schema,
                });
            }
        } else {
            warn!("Failed to discover MCP tools");
        }
    }

    let tools: Vec<uni::ToolDefinition> = declarations
        .into_iter()
        .map(|decl| uni::ToolDefinition::function(decl.name, decl.description, decl.parameters))
        .collect();

    let trim_config = load_context_trim_config(vt_cfg);
    let context_features = vt_cfg.map(|cfg| &cfg.context);
    let token_budget_enabled = context_features
        .map(|cfg| cfg.token_budget.enabled)
        .unwrap_or(true);
    let max_context_tokens = trim_config.max_tokens;
    let mut token_budget_config = RuntimeTokenBudgetConfig::for_model(
        context_features
            .map(|cfg| cfg.token_budget.model.as_str())
            .unwrap_or("gpt-4o-mini"),
        max_context_tokens,
    );
    if let Some(cfg) = context_features {
        token_budget_config.warning_threshold = cfg.token_budget.warning_threshold;
        token_budget_config.compaction_threshold = cfg.token_budget.compaction_threshold;
        token_budget_config.detailed_tracking = cfg.token_budget.detailed_tracking;
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
    );

    // Initialize MCP panel state
    let mcp_panel_state = if let Some(cfg) = vt_cfg {
        let _ui_config = vtcode_core::config::mcp::McpUiConfig {
            mode: cfg.mcp.ui.mode,
            max_events: cfg.mcp.ui.max_events,
            show_provider_names: cfg.mcp.ui.show_provider_names,
            renderers: cfg.mcp.ui.renderers.clone(),
        };
        mcp_events::McpPanelState::new(cfg.mcp.ui.max_events)
    } else {
        mcp_events::McpPanelState::default()
    };

    let mut tool_registry =
        ToolRegistry::new_with_features(config.workspace.clone(), todo_planning_enabled);
    tool_registry.initialize_async().await?;
    if let Some(cfg) = vt_cfg {
        if let Err(err) = tool_registry.apply_config_policies(&cfg.tools) {
            eprintln!(
                "Warning: Failed to apply tool policies from config: {}",
                err
            );
        }

        // Add MCP client to tool registry if enabled
        if cfg.mcp.enabled {
            if let Some(mcp_client) = &mcp_client {
                tool_registry = tool_registry.with_mcp_client(Arc::clone(mcp_client));
                if let Err(err) = tool_registry.refresh_mcp_tools().await {
                    warn!("Failed to refresh MCP tools: {}", err);
                }
            }
        }

        // Initialize full auto mode if requested
        if full_auto {
            let automation_cfg = cfg.automation.full_auto.clone();
            tool_registry.enable_full_auto_mode(&automation_cfg.allowed_tools);
            let allowlist = tool_registry
                .current_full_auto_allowlist()
                .unwrap_or_default();
            full_auto_allowlist = Some(allowlist);
        }
    }

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
        mcp_client: mcp_client.clone(),
        mcp_panel_state,
        token_budget,
        token_budget_enabled,
        curator,
    })
}
