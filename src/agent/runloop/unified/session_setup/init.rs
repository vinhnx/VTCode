use super::mcp_tools::build_mcp_tool_definitions;
use super::skill_setup::{discover_skills, register_skill_and_subagent_tools};
use super::types::{SessionState, build_conversation_history_from_resume};
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::telemetry::build_trajectory_logger;
use crate::agent::runloop::unified::async_mcp_manager::{AsyncMcpManager, McpInitStatus};
use crate::agent::runloop::unified::prompts::read_system_prompt;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::welcome::prepare_session_bootstrap;
use crate::workspace_trust;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::sync::RwLock;
use tokio::time::Duration;
use tracing::{debug, error, info, warn};
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::state::recover_history_from_crash;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::llm::{
    factory::{ProviderConfig, create_provider_with_config},
    provider as uni,
};

use vtcode::updater::Updater;
use vtcode_core::models::ModelId;
use vtcode_core::tools::build_function_declarations_cached;
use vtcode_core::tools::{ApprovalRecorder, SearchMetrics, ToolRegistry, ToolResultCache};
use vtcode_core::{apply_global_notification_config_from_vtcode, init_global_notification_manager};
use vtcode_tui::InlineHeaderHighlight;

const STARTUP_UPDATE_CHECK_COOLDOWN: StdDuration = StdDuration::from_secs(15 * 60);

#[allow(clippy::unnecessary_cast)]
fn vtcode_config_circuit_breaker_to_core(
    vt_cfg: Option<&VTCodeConfig>,
    _agent_config: &CoreAgentConfig,
) -> vtcode_core::tools::circuit_breaker::CircuitBreakerConfig {
    let default_cfg = vtcode_config::core::agent::CircuitBreakerConfig::default();
    let cfg = vt_cfg
        .map(|c| &c.agent.circuit_breaker)
        .unwrap_or(&default_cfg);

    if !cfg.enabled {
        return vtcode_core::tools::circuit_breaker::CircuitBreakerConfig {
            failure_threshold: u32::MAX,
            reset_timeout: Duration::from_secs(1),
            min_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(1),
            backoff_factor: 1.0,
        };
    }

    vtcode_core::tools::circuit_breaker::CircuitBreakerConfig {
        failure_threshold: cfg.failure_threshold,
        reset_timeout: Duration::from_secs(cfg.recovery_cooldown.max(1) as u64),
        min_backoff: Duration::from_secs(10),
        max_backoff: Duration::from_secs(300),
        backoff_factor: 2.0,
    }
}

pub(crate) async fn initialize_session(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    full_auto: bool,
    resume: Option<&ResumeSession>,
) -> Result<SessionState> {
    if let Some(cfg) = vt_cfg {
        if let Err(err) = apply_global_notification_config_from_vtcode(cfg) {
            warn!("Failed to apply notification configuration: {}", err);
        }
    } else if let Err(err) = init_global_notification_manager() {
        tracing::debug!(
            "Notification manager already initialized or unavailable: {}",
            err
        );
    }

    let tool_documentation_mode = vt_cfg
        .map(|cfg| cfg.agent.tool_documentation_mode)
        .unwrap_or_default();
    let async_mcp_manager = create_async_mcp_manager(vt_cfg);
    let mcp_error = determine_mcp_bootstrap_error(async_mcp_manager.as_ref()).await;

    let mut session_bootstrap = prepare_session_bootstrap(config, vt_cfg, mcp_error).await;
    append_update_highlight(&mut session_bootstrap).await;
    let provider_client = create_provider_client(config, vt_cfg)?;
    let mut full_auto_allowlist = None;

    let tool_definitions =
        build_tool_definitions(config, tool_documentation_mode, async_mcp_manager.as_ref()).await;
    let tools = Arc::new(RwLock::new(tool_definitions));

    let skill_setup = discover_skills(config, resume, &tools).await;
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let mut conversation_history = build_conversation_history_from_resume(resume).await;
    recover_history_from_crash(&mut conversation_history);

    let trajectory = build_trajectory_logger(&config.workspace, vt_cfg);
    let available_tools = {
        let tool_defs = tools.read().await;
        tool_defs
            .iter()
            .map(|def| def.function_name().to_string())
            .collect::<Vec<_>>()
    };
    let base_system_prompt = read_system_prompt(
        &config.workspace,
        session_bootstrap.prompt_addendum.as_deref(),
        &available_tools,
    )
    .await;
    let mcp_panel_state = if let Some(cfg) = vt_cfg {
        mcp_events::McpPanelState::new(cfg.mcp.ui.max_events, cfg.mcp.enabled)
    } else {
        mcp_events::McpPanelState::default()
    };

    let mut tool_registry = ToolRegistry::new(config.workspace.clone()).await;
    tool_registry.initialize_async().await?;
    if let Some(cfg) = vt_cfg {
        tool_registry.apply_commands_config(&cfg.commands);
        tool_registry.apply_sandbox_config(&cfg.sandbox);
        tool_registry.apply_timeout_policy(&cfg.timeouts);
        if let Err(err) = tool_registry.apply_config_policies(&cfg.tools).await {
            warn!("Failed to apply tool policies from config: {}", err);
        }
        maybe_attach_mcp_client(&mut tool_registry, cfg, async_mcp_manager.as_ref()).await;
        if full_auto {
            let automation_cfg = cfg.automation.full_auto.clone();
            tool_registry
                .enable_full_auto_mode(&automation_cfg.allowed_tools)
                .await;
            full_auto_allowlist = Some(
                tool_registry
                    .current_full_auto_allowlist()
                    .await
                    .unwrap_or_default(),
            );
        }
    }

    let workspace_trust_level = match session_bootstrap.acp_workspace_trust {
        Some(level) => Some(level.to_workspace_trust_level()),
        None => workspace_trust::workspace_trust_level(&config.workspace)
            .await
            .context("Failed to determine workspace trust level for tool policy")?,
    };
    apply_workspace_trust_prompt_policy(&mut tool_registry, full_auto, workspace_trust_level).await;
    register_skill_and_subagent_tools(&mut tool_registry, &tools, &skill_setup, config, vt_cfg)
        .await?;

    let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(128)));
    let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let search_metrics = Arc::new(RwLock::new(SearchMetrics::new()));
    let cache_dir = std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".vtcode").join("cache"))
        .unwrap_or_else(|| PathBuf::from(".vtcode/cache"));
    let approval_recorder = Arc::new(ApprovalRecorder::new(cache_dir));
    let tool_catalog = Arc::new(ToolCatalogState::new());
    tool_catalog.bump_version();

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

    let circuit_breaker = Arc::new(vtcode_core::tools::circuit_breaker::CircuitBreaker::new(
        vtcode_config_circuit_breaker_to_core(vt_cfg, config),
    ));
    tool_registry.set_shared_circuit_breaker(circuit_breaker.clone());

    Ok(SessionState {
        session_bootstrap,
        provider_client,
        tool_registry,
        tools,
        tool_catalog,
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
        loaded_skills: skill_setup.active_skills_map,
        approval_recorder,
        safety_validator: Arc::new(RwLock::new(ToolCallSafetyValidator::new())),
        circuit_breaker,
        tool_health_tracker: Arc::new(vtcode_core::tools::health::ToolHealthTracker::new(50)),
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
                let loop_limits: HashMap<_, _> = cfg
                    .tools
                    .loop_thresholds
                    .iter()
                    .map(|(k, v)| (k.clone(), *v))
                    .collect();
                executor.configure_loop_limits(&loop_limits).await;
            }
            Arc::new(executor)
        },
        error_recovery: Arc::new(RwLock::new(
            vtcode_core::core::agent::error_recovery::ErrorRecoveryState::new(),
        )),
    })
}

async fn append_update_highlight(
    session_bootstrap: &mut crate::agent::runloop::welcome::SessionBootstrap,
) {
    let updater = match Updater::new(env!("CARGO_PKG_VERSION")) {
        Ok(updater) => updater,
        Err(err) => {
            debug!("Failed to initialize updater for startup check: {}", err);
            return;
        }
    };

    let update = match updater
        .check_for_updates_cached(STARTUP_UPDATE_CHECK_COOLDOWN)
        .await
    {
        Ok(update) => update,
        Err(err) => {
            debug!("Startup update check failed: {}", err);
            return;
        }
    };

    let Some(update) = update else {
        return;
    };

    let guidance = updater.update_guidance();
    let mut lines = vec![
        format!(
            "New version available: v{} (current: v{})",
            update.version,
            updater.current_version()
        ),
        format!("Update with: {}", guidance.command),
        "Release notes: https://github.com/vinhnx/vtcode/releases".to_string(),
    ];

    if guidance.source.is_managed() {
        lines.insert(
            1,
            format!("Detected install source: {}", guidance.source.label()),
        );
    }

    session_bootstrap
        .header_highlights
        .push(InlineHeaderHighlight {
            title: "Update Available".to_string(),
            lines,
        });
}

fn create_async_mcp_manager(vt_cfg: Option<&VTCodeConfig>) -> Option<Arc<AsyncMcpManager>> {
    let cfg = vt_cfg?;
    if !cfg.mcp.enabled {
        debug!("MCP is disabled in configuration");
        return None;
    }

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
    if let Err(e) = manager_arc.start_initialization() {
        error!("Failed to start async MCP initialization: {}", e);
    }
    Some(manager_arc)
}

async fn determine_mcp_bootstrap_error(manager: Option<&Arc<AsyncMcpManager>>) -> Option<String> {
    let manager = manager?;
    match manager.get_status().await {
        McpInitStatus::Error { message } => Some(message.clone()),
        McpInitStatus::Initializing { .. } => None,
        McpInitStatus::Disabled => None,
        McpInitStatus::Ready { .. } => None,
    }
}

fn create_provider_client(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<Box<dyn uni::LLMProvider>> {
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
    create_provider_with_config(
        &provider_name,
        ProviderConfig {
            api_key: Some(config.api_key.clone()),
            base_url: None,
            model: Some(config.model.clone()),
            prompt_cache: Some(config.prompt_cache.clone()),
            timeouts: None,
            openai: vt_cfg.map(|cfg| cfg.provider.openai.clone()),
            anthropic: vt_cfg.map(|cfg| cfg.provider.anthropic.clone()),
            model_behavior: vt_cfg.map(|cfg| cfg.model.clone()),
        },
    )
    .context("Failed to initialize provider client")
}

async fn build_tool_definitions(
    config: &CoreAgentConfig,
    tool_documentation_mode: vtcode_core::config::ToolDocumentationMode,
    async_mcp_manager: Option<&Arc<AsyncMcpManager>>,
) -> Vec<uni::ToolDefinition> {
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

    if let Ok(model_id) = ModelId::from_str(&config.model)
        && model_id.supports_shell_tool()
    {
        tool_definitions.push(uni::ToolDefinition::apply_patch(
            "Apply VT Code patches to modify files. IMPORTANT: Use VT Code format (*** Begin Patch, *** Update File: path, @@ context, -/+ lines, *** End Patch), NOT unified diff (---/+++) format. The tool creates, updates, or deletes file content.".to_string(),
        ));
    }

    if let Some(manager) = async_mcp_manager {
        let status = manager.get_status().await;
        if let McpInitStatus::Ready { client } = &status {
            match client.list_tools().await {
                Ok(mcp_tools) => {
                    info!("Found {} MCP tools", mcp_tools.len());
                    tool_definitions.extend(build_mcp_tool_definitions(&mcp_tools));
                }
                Err(err) => warn!("Failed to discover MCP tools from async manager: {err}"),
            }
        } else {
            debug!("MCP client not ready yet, tools will be available later via dynamic update");
        }
    }

    vtcode_core::prompts::sort_tool_definitions(tool_definitions)
}

async fn maybe_attach_mcp_client(
    tool_registry: &mut ToolRegistry,
    cfg: &VTCodeConfig,
    async_mcp_manager: Option<&Arc<AsyncMcpManager>>,
) {
    if !cfg.mcp.enabled {
        return;
    }
    let Some(manager) = async_mcp_manager else {
        return;
    };
    let status = manager.get_status().await;
    if let McpInitStatus::Ready { client } = &status {
        *tool_registry = tool_registry
            .clone()
            .with_mcp_client(Arc::clone(client))
            .await;
        if let Err(err) = tool_registry.refresh_mcp_tools().await {
            warn!("Failed to refresh MCP tools: {}", err);
        }
    }
}

async fn apply_workspace_trust_prompt_policy(
    tool_registry: &mut ToolRegistry,
    full_auto: bool,
    workspace_trust_level: Option<WorkspaceTrustLevel>,
) {
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
}
