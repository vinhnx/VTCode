use super::skill_setup::{discover_skills, register_skill_tools};
use super::types::{
    SessionMetadataContext, SessionState, ToolExecutionContext,
    build_conversation_history_from_resume,
};
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::telemetry::build_trajectory_logger;
use crate::agent::runloop::unified::async_mcp_manager::{
    AsyncMcpManager, McpInitStatus, approval_policy_from_human_in_the_loop,
};
use crate::agent::runloop::unified::prompts::read_system_prompt;
use crate::agent::runloop::unified::state::should_enforce_safe_mode_prompts;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::welcome::prepare_session_bootstrap;
use anyhow::{Context, Result, bail};
use hashbrown::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Duration;
use tracing::{debug, info, warn};
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::state::recover_history_from_crash;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::llm::factory::{ProviderConfig, create_provider_with_config, infer_provider};
use vtcode_core::llm::provider as uni;

use vtcode_core::models::ModelId;
use vtcode_core::subagents::{SubagentController, SubagentControllerConfig};
use vtcode_core::tools::handlers::{
    DeferredToolPolicy, SessionSurface, SessionToolsConfig, ToolModelCapabilities,
    anthropic_native_memory_enabled_for_runtime, deferred_tool_policy_for_runtime,
};
use vtcode_core::tools::{ApprovalRecorder, ToolRegistry, ToolResultCache};
use vtcode_core::utils::dot_config::load_workspace_trust_level;
use vtcode_core::{
    ActivePrimaryAgent, ActivePrimaryAgentState, apply_global_notification_config_from_vtcode,
    build_primary_agent_runtime_config, init_global_notification_manager,
};

use crate::startup::take_search_tools_bundle_notice;
use crate::updater::{Updater, append_notice_highlight};

#[expect(clippy::unnecessary_cast)]
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
        reset_timeout: Duration::from_secs(cfg.recovery_cooldown.max(1)),
        min_backoff: Duration::from_secs(5),
        max_backoff: Duration::from_secs(120),
        backoff_factor: 2.0,
    }
}

/// Resolve the provider display label, preferring custom provider display names.
///
/// Returns an empty string when the provider key is blank (caller should fall
/// back to the runtime provider name).
pub(crate) fn resolve_provider_label(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> String {
    if config.provider.eq_ignore_ascii_case("openai") && config.openai_chatgpt_auth.is_some() {
        return "OpenAI (ChatGPT)".to_string();
    }

    let key = config.provider.trim();
    if key.is_empty() {
        return String::new();
    }

    if let Some(vt_cfg) = vt_cfg {
        return vt_cfg.provider_display_name(key);
    }

    key.to_string()
}

pub(crate) async fn initialize_session(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    full_auto: bool,
    primary_agent_explicitly_configured: bool,
    resume: Option<&ResumeSession>,
    parent_session_id: &str,
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
    let async_mcp_manager = create_async_mcp_manager(vt_cfg, None);
    let mcp_error = determine_mcp_bootstrap_error(async_mcp_manager.as_ref()).await;

    let mut session_bootstrap = prepare_session_bootstrap(config, vt_cfg, mcp_error).await;
    session_bootstrap.search_tools_notice = take_search_tools_bundle_notice().await;
    let startup_update_check = load_startup_update_check();
    if let Some(notice) = startup_update_check.cached_notice.as_ref() {
        append_notice_highlight(&mut session_bootstrap.header_highlights, notice);
    }
    // Register custom OpenAI-compatible providers from config
    if let Some(cfg) = vt_cfg {
        vtcode_core::llm::factory::register_custom_providers(&cfg.custom_providers);
    }

    let provider_client = create_provider_client(config, vt_cfg)?;
    let deferred_tool_policy = active_deferred_tool_policy(config, vt_cfg, &*provider_client);
    let mut full_auto_allowlist = None;

    let skill_setup = discover_skills(config, resume).await;
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let mut conversation_history = build_conversation_history_from_resume(resume).await;
    recover_history_from_crash(&mut conversation_history);
    let mcp_panel_state = if let Some(cfg) = vt_cfg {
        mcp_events::McpPanelState::new(cfg.mcp.ui.max_events, cfg.mcp.enabled)
    } else {
        mcp_events::McpPanelState::default()
    };

    let mut tool_registry = ToolRegistry::new(config.workspace.clone()).await;
    tool_registry.initialize_async().await?;
    tool_registry.set_harness_session(parent_session_id.to_string());
    if let Some(cfg) = vt_cfg {
        if let Err(err) = tool_registry
            .apply_session_runtime_config(
                &cfg.commands,
                &cfg.permissions,
                &cfg.sandbox,
                &cfg.timeouts,
                &cfg.tools,
            )
            .await
        {
            warn!("Failed to apply tool policies from config: {}", err);
        }
        maybe_attach_mcp_client(&mut tool_registry, cfg, async_mcp_manager.as_ref()).await;
        if full_auto {
            let automation_cfg = cfg.automation.full_auto.clone();
            tool_registry
                .enable_full_auto_permission(&automation_cfg.allowed_tools)
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
        None => load_workspace_trust_level(&config.workspace)
            .await
            .context("Failed to determine workspace trust level for tool policy")?,
    };
    let auto_permission_review_active = full_auto;
    apply_workspace_trust_prompt_policy(
        &mut tool_registry,
        auto_permission_review_active,
        workspace_trust_level,
    )
    .await;

    let subagent_controller = if resume.is_none_or(ResumeSession::is_root_thread)
        && let Some(cfg) = vt_cfg
        && cfg.subagents.enabled
    {
        match SubagentController::new(SubagentControllerConfig {
            workspace_root: config.workspace.clone(),
            parent_session_id: parent_session_id.to_string(),
            parent_model: config.model.clone(),
            parent_provider: config.provider.clone(),
            parent_reasoning_effort: config.reasoning_effort,
            api_key: config.api_key.clone(),
            vt_cfg: cfg.clone(),
            openai_chatgpt_auth: config.openai_chatgpt_auth.clone(),
            depth: 0,
            exec_sessions: tool_registry.exec_session_manager(),
            pty_manager: tool_registry.pty_manager().clone(),
            managed_background_runtime: false,
        })
        .await
        {
            Ok(controller) => {
                controller.set_parent_messages(&conversation_history).await;
                let controller = Arc::new(controller);
                tool_registry.set_subagent_controller(controller.clone());
                if cfg.subagents.background.auto_restore
                    && let Err(err) = controller.restore_background_subagents().await
                {
                    warn!("Failed to restore background subagents: {}", err);
                }
                Some(controller)
            }
            Err(err) => {
                warn!("Failed to initialize subagent controller: {}", err);
                None
            }
        }
    } else {
        None
    };

    // CGP Phase 5: Wrap registered tools through the CGP approval → sandbox → middleware pipeline.
    let cgp_mode = if full_auto {
        vtcode_core::tools::CgpRuntimeMode::Ci
    } else {
        vtcode_core::tools::CgpRuntimeMode::Interactive
    };
    tool_registry.enable_cgp_pipeline(cgp_mode).await;

    let tool_catalog = tool_registry.tool_catalog_state();
    let anthropic_native_memory_enabled =
        active_anthropic_native_memory(config, vt_cfg, provider_client.as_ref());

    let tools = Arc::new(RwLock::new(
        tool_registry
            .model_tools(
                SessionToolsConfig::full_public(
                    SessionSurface::Interactive,
                    vtcode_core::config::types::CapabilityLevel::CodeSearch,
                    tool_documentation_mode,
                    ToolModelCapabilities::for_model_name(&config.model),
                )
                .with_deferred_tool_policy(deferred_tool_policy.clone())
                .with_anthropic_native_memory_enabled(anthropic_native_memory_enabled),
            )
            .await,
    ));
    register_skill_tools(
        &mut tool_registry,
        &tools,
        &tool_catalog,
        config,
        vt_cfg,
        tool_documentation_mode,
        deferred_tool_policy.clone(),
        anthropic_native_memory_enabled,
        &skill_setup,
    )
    .await?;
    refresh_tool_snapshot(
        &tool_registry,
        &tools,
        &tool_catalog,
        config,
        vt_cfg,
        tool_documentation_mode,
        &deferred_tool_policy,
    )
    .await;

    let trajectory = build_trajectory_logger(&config.workspace, vt_cfg);
    let available_subagents = if let Some(controller) = subagent_controller.as_ref() {
        controller
            .effective_specs()
            .await
            .into_iter()
            .filter(|spec| spec.is_subagent())
            .map(|spec| {
                let read_only = spec.is_read_only();
                (spec.name, spec.description, read_only)
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let base_system_prompt = read_system_prompt(
        &config.workspace,
        session_bootstrap.prompt_addendum.as_deref(),
        &available_subagents,
    )
    .await;
    let active_primary_agent = if let Some(controller) = subagent_controller.as_ref() {
        active_primary_agent_from_specs_for_mode(
            &controller.effective_specs().await,
            vt_cfg,
            full_auto,
            primary_agent_explicitly_configured,
        )?
    } else {
        let discovered = vtcode_config::discover_subagents(
            &vtcode_config::SubagentDiscoveryInput::new(config.workspace.clone()),
        )
        .with_context(|| {
            format!(
                "Failed to discover primary agents in {}",
                config.workspace.display()
            )
        })?;
        active_primary_agent_from_specs_for_mode(
            &discovered.effective,
            vt_cfg,
            full_auto,
            primary_agent_explicitly_configured,
        )?
    };
    if let (Some(manager), Some(cfg)) = (async_mcp_manager.as_ref(), vt_cfg) {
        manager
            .reconfigure(primary_agent_mcp_config(cfg, active_primary_agent.active()))
            .await?;
    }

    let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(128)));
    let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let cache_dir = std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".vtcode").join("cache"))
        .unwrap_or_else(|| PathBuf::from(".vtcode/cache"));
    let approval_recorder = Arc::new(ApprovalRecorder::new(cache_dir));
    let permissions_state = Arc::new(RwLock::new(
        vt_cfg
            .map(|cfg| cfg.permissions.clone())
            .unwrap_or_default(),
    ));
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

    let circuit_breaker = Arc::new(
        vtcode_core::tools::circuit_breaker::CircuitBreaker::with_metrics(
            vtcode_config_circuit_breaker_to_core(vt_cfg, config),
            tool_registry.metrics_collector(),
        ),
    );
    tool_registry.set_shared_circuit_breaker(circuit_breaker.clone());
    let shared_safety_gateway = tool_registry.safety_gateway();

    Ok(SessionState {
        session_bootstrap,
        startup_update_check,
        provider_client,
        tool_registry,
        tools,
        tool_catalog,
        conversation_history,
        execution: ToolExecutionContext {
            tool_result_cache,
            tool_permission_cache,
            permissions_state,
            approval_recorder,
            safety_validator: Arc::new(ToolCallSafetyValidator::with_gateway(
                shared_safety_gateway,
            )),
            circuit_breaker: circuit_breaker.clone(),
            tool_health_tracker: Arc::new(vtcode_core::tools::health::ToolHealthTracker::new(50)),
            rate_limiter: Arc::new(
                vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter::default(),
            ),
            validation_cache: Arc::new(
                vtcode_core::tools::validation_cache::ValidationCache::default(),
            ),
            autonomous_executor: {
                let executor = vtcode_core::tools::autonomous_executor::AutonomousExecutor::new();
                if let Some(cfg) = vt_cfg {
                    let loop_limits: HashMap<_, _> = cfg
                        .tools
                        .loop_thresholds
                        .iter()
                        .map(|(k, v)| (k.clone(), *v))
                        .collect();
                    executor.configure_loop_limits(&loop_limits);
                }
                Arc::new(executor)
            },
        },
        metadata: SessionMetadataContext {
            decision_ledger,
            trajectory,
            telemetry: Arc::new(vtcode_core::core::telemetry::TelemetryManager::new()),
            error_recovery: Arc::new(RwLock::new(
                vtcode_core::core::agent::error_recovery::ErrorRecoveryState::new(),
            )),
        },
        base_system_prompt,
        full_auto_allowlist,
        async_mcp_manager,
        mcp_panel_state,
        loaded_skills: skill_setup.active_skills_map,
        active_primary_agent,
    })
}

#[cfg(test)]
fn active_primary_agent_from_specs(
    specs: &[vtcode_config::SubagentSpec],
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<ActivePrimaryAgentState> {
    active_primary_agent_from_specs_for_mode(specs, vt_cfg, false, false)
}

pub(crate) fn active_primary_agent_from_specs_for_mode(
    specs: &[vtcode_config::SubagentSpec],
    vt_cfg: Option<&VTCodeConfig>,
    full_auto: bool,
    primary_agent_explicitly_configured: bool,
) -> Result<ActivePrimaryAgentState> {
    if full_auto && !primary_agent_explicitly_configured {
        let mut active = ActivePrimaryAgentState::default();
        if active.select_from_specs(specs, "auto").is_err() {
            bail!(
                "Full-auto needs the defaulted 'auto' primary agent, but no effective primary agent named 'auto' was discovered. Configure default_primary_agent explicitly or add an 'auto' primary agent."
            );
        }
        return Ok(active);
    }

    let default_primary_agent = vt_cfg
        .map(|cfg| cfg.default_primary_agent.as_str())
        .unwrap_or(vtcode_config::constants::defaults::DEFAULT_PRIMARY_AGENT_NAME);
    Ok(ActivePrimaryAgentState::from_specs_with_default(
        specs,
        default_primary_agent,
    ))
}

fn load_startup_update_check() -> crate::updater::StartupUpdateCheck {
    let updater = match Updater::new(env!("CARGO_PKG_VERSION")) {
        Ok(updater) => updater,
        Err(err) => {
            debug!("Failed to initialize updater for startup check: {}", err);
            return crate::updater::StartupUpdateCheck::default();
        }
    };

    match updater.startup_update_check() {
        Ok(check) => check,
        Err(err) => {
            debug!("Startup update check failed: {}", err);
            crate::updater::StartupUpdateCheck::default()
        }
    }
}

fn create_async_mcp_manager(
    vt_cfg: Option<&VTCodeConfig>,
    active_primary_agent: Option<&ActivePrimaryAgent>,
) -> Option<Arc<AsyncMcpManager>> {
    let cfg = vt_cfg?;
    if !cfg.mcp.enabled {
        debug!("MCP is disabled in configuration");
        return None;
    }
    let mcp_config = active_primary_agent
        .map(|agent| primary_agent_mcp_config(cfg, agent))
        .unwrap_or_else(|| cfg.mcp.clone());

    info!(
        "Setting up async MCP client with {} providers",
        mcp_config.providers.len()
    );
    let approval_policy = approval_policy_from_human_in_the_loop(cfg.security.human_in_the_loop);
    let manager = AsyncMcpManager::new(
        mcp_config,
        cfg.security.hitl_notification_bell,
        approval_policy,
        Arc::new(|_event: mcp_events::McpEvent| {}),
    );
    Some(Arc::new(manager))
}

fn primary_agent_mcp_config(
    cfg: &VTCodeConfig,
    active_primary_agent: &ActivePrimaryAgent,
) -> vtcode_core::config::mcp::McpClientConfig {
    build_primary_agent_runtime_config(cfg, active_primary_agent).mcp
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

pub(crate) fn create_provider_client(
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
            openai_chatgpt_auth: config.openai_chatgpt_auth.clone(),
            copilot_auth: vt_cfg.map(|cfg| cfg.auth.copilot.clone()),
            base_url: None,
            model: Some(config.model.clone()),
            prompt_cache: Some(config.prompt_cache.clone()),
            timeouts: None,
            openai: vt_cfg.map(|cfg| cfg.provider.openai.clone()),
            anthropic: vt_cfg.map(|cfg| cfg.provider.anthropic.clone()),
            model_behavior: vt_cfg.map(|cfg| cfg.model.clone()),
            workspace_root: Some(config.workspace.clone()),
        },
    )
    .context("Failed to initialize provider client")
}

pub(crate) fn active_deferred_tool_policy(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    provider_client: &dyn uni::LLMProvider,
) -> DeferredToolPolicy {
    deferred_tool_policy_for_runtime(
        infer_provider(Some(&config.provider), &config.model),
        provider_client.supports_responses_compaction(&config.model),
        vt_cfg,
    )
}

pub(crate) fn active_anthropic_native_memory(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    provider_client: &dyn uni::LLMProvider,
) -> bool {
    anthropic_native_memory_enabled_for_runtime(
        vtcode_core::config::models::Provider::from_str(provider_client.name()).ok(),
        &config.model,
        vt_cfg,
    )
}

pub(crate) async fn refresh_tool_snapshot(
    tool_registry: &ToolRegistry,
    tools: &Arc<RwLock<Vec<uni::ToolDefinition>>>,
    _tool_catalog: &ToolCatalogState,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    tool_documentation_mode: vtcode_core::config::ToolDocumentationMode,
    deferred_tool_policy: &DeferredToolPolicy,
) {
    let anthropic_native_memory_enabled = anthropic_native_memory_enabled_for_runtime(
        infer_provider(Some(&config.provider), &config.model),
        &config.model,
        vt_cfg,
    );
    let next = tool_registry
        .model_tools(
            SessionToolsConfig::full_public(
                SessionSurface::Interactive,
                vtcode_core::config::types::CapabilityLevel::CodeSearch,
                tool_documentation_mode,
                ToolModelCapabilities::for_model_name(&config.model),
            )
            .with_deferred_tool_policy(deferred_tool_policy.clone())
            .with_anthropic_native_memory_enabled(anthropic_native_memory_enabled),
        )
        .await;
    *tools.write().await = next;
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
    auto_permission_review_active: bool,
    workspace_trust_level: Option<WorkspaceTrustLevel>,
) {
    let enforce_safe_mode_prompts = should_enforce_safe_mode_prompts(
        false,
        auto_permission_review_active,
        workspace_trust_level,
    );
    tool_registry
        .set_enforce_safe_mode_prompts(enforce_safe_mode_prompts)
        .await;
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use vtcode_config::{
        AgentMode, SubagentMcpServer, SubagentSource, SubagentSpec,
        core::permissions::{AgentPermissionsConfig, PermissionDefault},
    };

    use super::*;

    #[test]
    fn async_mcp_manager_uses_primary_agent_merged_mcp_config() {
        let mut cfg = VTCodeConfig::default();
        cfg.mcp.enabled = true;
        cfg.mcp.providers.push(
            serde_json::from_value(json!({
                "name": "global",
                "command": "global-mcp",
                "args": []
            }))
            .expect("global provider"),
        );

        let mut spec = test_primary_agent_spec("mcp-primary");
        spec.mcp_servers = vec![SubagentMcpServer::Inline(BTreeMap::from([
            (
                "global".to_string(),
                json!({
                    "type": "stdio",
                    "command": "duplicate-mcp"
                }),
            ),
            (
                "local".to_string(),
                json!({
                    "type": "stdio",
                    "command": "local-mcp"
                }),
            ),
        ]))];
        let active = ActivePrimaryAgent::from_spec(&spec);

        let manager =
            create_async_mcp_manager(Some(&cfg), Some(&active)).expect("manager should exist");
        let manager_config = manager.config();
        let provider_names = manager_config
            .providers
            .iter()
            .map(|provider| provider.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(provider_names, vec!["global", "local"]);
    }

    #[test]
    fn startup_primary_agent_defaults_to_duck_without_config() {
        let active = active_primary_agent_from_specs(&[test_primary_agent_spec("builder")], None)
            .expect("default primary agent");

        assert_eq!(active.active().identity.name, "duck");
        assert_eq!(active.active().identity.source, SubagentSource::Builtin);
    }

    #[test]
    fn startup_primary_agent_uses_default_primary_agent_config() {
        let mut cfg = VTCodeConfig {
            default_primary_agent: "builder".to_string(),
            ..VTCodeConfig::default()
        };
        let active =
            active_primary_agent_from_specs(&[test_primary_agent_spec("builder")], Some(&cfg))
                .expect("configured primary agent");

        assert_eq!(active.active().identity.name, "builder");

        cfg.default_primary_agent = "missing".to_string();
        let fallback =
            active_primary_agent_from_specs(&[test_primary_agent_spec("builder")], Some(&cfg))
                .expect("fallback primary agent");

        assert_eq!(fallback.active().identity.name, "duck");
        assert_eq!(fallback.active().identity.source, SubagentSource::Builtin);
    }

    #[test]
    fn full_auto_with_defaulted_primary_agent_selects_effective_auto() {
        let mut auto = test_primary_agent_spec("auto");
        auto.prompt = "Custom auto instructions".to_string();

        let active =
            active_primary_agent_from_specs_for_mode(&[auto], None, true, false).expect("auto");

        assert_eq!(active.active().identity.name, "auto");
        assert_eq!(active.active().instructions, "Custom auto instructions");
    }

    #[test]
    fn full_auto_honours_explicit_primary_agent_config() {
        let cfg = VTCodeConfig {
            default_primary_agent: "builder".to_string(),
            ..VTCodeConfig::default()
        };
        let specs = [
            test_primary_agent_spec("auto"),
            test_primary_agent_spec("builder"),
        ];

        let active = active_primary_agent_from_specs_for_mode(&specs, Some(&cfg), true, true)
            .expect("explicit builder");

        assert_eq!(active.active().identity.name, "builder");
    }

    #[test]
    fn full_auto_honours_explicit_duck_primary_agent_config() {
        let cfg = VTCodeConfig {
            default_primary_agent: "duck".to_string(),
            ..VTCodeConfig::default()
        };
        let specs = [test_primary_agent_spec("auto")];

        let active = active_primary_agent_from_specs_for_mode(&specs, Some(&cfg), true, true)
            .expect("explicit duck");

        assert_eq!(active.active().identity.name, "duck");
        assert_eq!(active.active().identity.source, SubagentSource::Builtin);
    }

    #[test]
    fn full_auto_missing_defaulted_auto_fails_fast() {
        let err = active_primary_agent_from_specs_for_mode(
            &[test_primary_agent_spec("builder")],
            None,
            true,
            false,
        )
        .expect_err("missing auto should fail");

        assert!(
            err.to_string()
                .contains("no effective primary agent named 'auto' was discovered")
        );
    }

    fn test_primary_agent_spec(name: &str) -> SubagentSpec {
        SubagentSpec {
            name: name.to_string(),
            description: format!("{name} description"),
            prompt: format!("{name} instructions"),
            tools: None,
            disallowed_tools: Vec::new(),
            model: None,
            color: None,
            reasoning_effort: None,
            permissions: AgentPermissionsConfig::new(PermissionDefault::Deny),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: AgentMode::Primary,
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::ProjectVtcode,
            file_path: None,
            warnings: Vec::new(),
        }
    }
}
