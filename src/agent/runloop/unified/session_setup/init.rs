use super::skill_setup::{SkillSetupState, discover_skills, register_skill_tools};
use super::types::{
    SessionMetadataContext, SessionState, ToolExecutionContext, build_conversation_history_from_resume,
};
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::telemetry::build_trajectory_logger;
use crate::agent::runloop::unified::async_mcp_manager::{
    AsyncMcpManager, McpInitStatus, approval_policy_from_human_in_the_loop,
};
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::prompts::{fallback_base_system_prompt, read_system_prompt};
use crate::agent::runloop::unified::state::should_enforce_safe_mode_prompts;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::welcome::{SessionBootstrapMode, prepare_session_bootstrap_with_mode};
use anyhow::{Context, Result};
use hashbrown::HashMap;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Duration;
use tracing::{debug, error, info, warn};
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::state::recover_history_from_crash;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::llm::factory::{ProviderConfig, create_provider_with_config, infer_provider};
use vtcode_core::llm::provider as uni;
use vtcode_core::mcp::plugin_providers::discover_plugin_mcp_providers;

use vtcode_commons::VtCodePaths;
use vtcode_core::models::ModelId;
use vtcode_core::subagents::{SubagentController, SubagentControllerConfig};
use vtcode_core::tools::handlers::{
    DeferredToolPolicy, SessionSurface, SessionToolsConfig, ToolModelCapabilities,
    anthropic_native_memory_enabled_for_runtime, deferred_tool_policy_for_runtime,
};
use vtcode_core::tools::{ApprovalRecorder, ToolRegistry, ToolResultCache};
use vtcode_core::utils::dot_config::load_workspace_trust_level;
use vtcode_core::{
    ActivePrimaryAgent, apply_global_notification_config_from_vtcode, build_primary_agent_runtime_config,
    init_global_notification_manager,
};

use crate::startup::take_search_tools_bundle_notice;
use crate::updater::{Updater, append_notice_highlight};
use vtcode_config::MiMoAuthMethod;
use vtcode_config::core::AnthropicConfig;
use vtcode_config::models::detect_mimo_auth_method;

#[cfg(test)]
use super::session_mode::active_primary_agent_from_specs;
use super::session_mode::active_primary_agent_from_specs_for_mode;

fn vtcode_config_circuit_breaker_to_core(
    vt_cfg: Option<&VTCodeConfig>,
    _agent_config: &CoreAgentConfig,
) -> vtcode_core::tools::circuit_breaker::CircuitBreakerConfig {
    let default_cfg = vtcode_config::core::agent::CircuitBreakerConfig::default();
    let cfg = vt_cfg.map(|c| &c.agent.circuit_breaker).unwrap_or(&default_cfg);

    if !cfg.enabled {
        return vtcode_core::tools::circuit_breaker::CircuitBreakerConfig {
            failure_threshold: u32::MAX,
            reset_timeout: Duration::from_secs(1),
            min_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(1),
            backoff_factor: 1.0,
            half_open_probe_count: 1,
        };
    }

    vtcode_core::tools::circuit_breaker::CircuitBreakerConfig {
        failure_threshold: cfg.failure_threshold,
        reset_timeout: Duration::from_secs(cfg.recovery_cooldown.max(1)),
        min_backoff: Duration::from_secs(5),
        max_backoff: Duration::from_secs(120),
        backoff_factor: 2.0,
        half_open_probe_count: 1,
    }
}

/// Resolve the provider display label, preferring custom provider display names.
///
/// Returns an empty string when the provider key is blank (caller should fall
/// back to the runtime provider name).
pub(crate) fn resolve_provider_label(config: &CoreAgentConfig, vt_cfg: Option<&VTCodeConfig>) -> String {
    if config.provider.eq_ignore_ascii_case("openai") && config.openai_chatgpt_auth.is_some() {
        return "OpenAI (ChatGPT)".to_string();
    }

    // MiMo auth method detection
    if config.provider.eq_ignore_ascii_case("mimo") {
        if let Some(vt_cfg) = vt_cfg
            && let Some(method) = vt_cfg.provider.mimo_auth_method
        {
            if method != MiMoAuthMethod::Unknown {
                return format!("{} ({})", "Xiaomi MiMo", method.label());
            }
            warn!("Unrecognized MiMo auth method in config; falling back to API-key detection");
        }
        if !config.api_key.is_empty() {
            let method = detect_mimo_auth_method(&config.api_key, None);
            return format!("{} ({})", "Xiaomi MiMo", method.label());
        }
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

/// Combined interactive/non-UI session bootstrap is intentionally split into
/// [`initialize_session_critical`] + [`hydrate_session_runtime`]. Tests that
/// need a fully hydrated session without a TUI call both helpers in order.
#[cfg(test)]
pub(crate) async fn initialize_session(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    full_auto: bool,
    primary_agent_explicitly_configured: bool,
    resume: Option<&ResumeSession>,
    parent_session_id: &str,
    session_primary_agent_override: Option<&str>,
) -> Result<SessionState> {
    let mut session_state = initialize_session_critical(
        config,
        vt_cfg,
        full_auto,
        primary_agent_explicitly_configured,
        resume,
        parent_session_id,
        session_primary_agent_override,
    )
    .await?;
    complete_session_registry(
        &mut session_state,
        config,
        vt_cfg,
        full_auto,
        primary_agent_explicitly_configured,
        resume,
        parent_session_id,
        session_primary_agent_override,
    )
    .await?;
    let mut context_manager = ContextManager::new(
        session_state.base_system_prompt.clone(),
        (),
        session_state.loaded_skills.clone(),
        vt_cfg.map(|cfg| cfg.agent.clone()),
    );
    context_manager.set_workspace_root(&config.workspace);
    hydrate_session_runtime(
        &mut session_state,
        &mut context_manager,
        config,
        vt_cfg,
        full_auto,
        primary_agent_explicitly_configured,
        resume,
        parent_session_id,
        session_primary_agent_override,
    )
    .await?;
    Ok(session_state)
}

/// Critical-path session state required to spawn the TUI.
///
/// Registry-light critical path: provider construction, resume history, and
/// cheap bootstrap metadata only. `ToolRegistry` construction and subagent
/// discovery run in [`complete_session_registry`] after first paint; full tool
/// catalog projection, system-prompt composition, subagent controller
/// creation, CGP wiring, and MCP reconfigure run in [`hydrate_session_runtime`].
pub(crate) async fn initialize_session_critical(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    _full_auto: bool,
    _primary_agent_explicitly_configured: bool,
    resume: Option<&ResumeSession>,
    _parent_session_id: &str,
    _session_primary_agent_override: Option<&str>,
) -> Result<SessionState> {
    if let Some(cfg) = vt_cfg {
        if let Err(err) = apply_global_notification_config_from_vtcode(cfg) {
            warn!("Failed to apply notification configuration: {}", err);
        }
    } else if let Err(err) = init_global_notification_manager() {
        tracing::debug!("Notification manager already initialized or unavailable: {}", err);
    }

    let async_mcp_manager = create_async_mcp_manager_inner(vt_cfg, None, &config.workspace, false);
    // First-paint path avoids all disk I/O except the cheap bootstrap and
    // resume-history joins below. Update-cache and release-notes reads move
    // to hydration (see `hydrate_session_runtime`); only the in-memory
    // preflight notice (no I/O) is consulted here so an already-fetched
    // update still surfaces without paying `Updater::new` on the critical
    // path.
    let (mcp_error, mut session_bootstrap, conversation_history) = tokio::join!(
        determine_mcp_bootstrap_error(async_mcp_manager.as_ref()),
        prepare_session_bootstrap_with_mode(config, vt_cfg, None, SessionBootstrapMode::Critical),
        build_conversation_history_from_resume(resume),
    );
    let startup_update_check = crate::updater::get_preflight_notice()
        .map(|notice| crate::updater::StartupUpdateCheck { cached_notice: Some(notice), should_refresh: false })
        .unwrap_or_default();
    let release_highlights = None;
    session_bootstrap.mcp_error = mcp_error;
    session_bootstrap.search_tools_notice = take_search_tools_bundle_notice().await;
    if let Some(notice) = startup_update_check.cached_notice.as_ref() {
        append_notice_highlight(&mut session_bootstrap.header_highlights, notice);
    }
    session_bootstrap.release_highlights = release_highlights;

    if let Some(cfg) = vt_cfg {
        vtcode_core::llm::factory::register_custom_providers(&cfg.custom_providers);
    }

    let provider_client = create_provider_client(config, vt_cfg)?;
    let skill_setup = discover_skills(config, resume).await;
    let mut conversation_history = conversation_history;
    recover_history_from_crash(&mut conversation_history);
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let mcp_panel_state = if let Some(cfg) = vt_cfg {
        mcp_events::McpPanelState::new(cfg.mcp.ui.max_events, cfg.mcp.enabled)
    } else {
        mcp_events::McpPanelState::default()
    };

    // Registry-light critical path: `ToolRegistry` and workspace/plugin
    // discovery run in `complete_session_registry` after first paint. The
    // painted shell only needs bootstrap chrome and a typeable input.
    let tools = Arc::new(RwLock::new(Vec::new()));
    let tool_catalog = Arc::new(ToolCatalogState::new());
    // Config/default primary agent until discovery re-drives the header.
    let active_primary_agent = vtcode_core::primary_agent::ActivePrimaryAgentState::default();

    let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(128)));
    let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    // First paint avoids approval-cache disk I/O: resolve paths (env + joins,
    // no syscalls) and construct a deferred recorder without reading pattern
    // files or creating directories. Hydration ensures the directory and loads
    // patterns before the first model turn; writes self-ensure via
    // `ensure_user_dir`, so paint correctness never depends on the mkdir.
    let paths = VtCodePaths::resolve().context("failed to resolve private VT Code approval cache")?;
    let cache_dir = paths
        .cache_path("approval")
        .context("failed to resolve private VT Code approval cache")?;
    let legacy_cache_dirs = [
        paths.cache_dir().to_path_buf(),
        paths.config_dir().join("cache"),
        paths.legacy_dir().join("cache"),
    ];
    let approval_recorder = Arc::new(ApprovalRecorder::new_deferred(cache_dir, legacy_cache_dirs));
    let permissions_state = Arc::new(RwLock::new(vt_cfg.map(|cfg| cfg.permissions.clone()).unwrap_or_default()));
    let circuit_breaker = Arc::new(vtcode_core::tools::circuit_breaker::CircuitBreaker::new(
        vtcode_config_circuit_breaker_to_core(vt_cfg, config),
    ));
    let shared_safety_gateway = Arc::new(vtcode_core::tools::safety_gateway::SafetyGateway::new());

    // Seed prompt only; full composition runs after first paint.
    let base_system_prompt = fallback_base_system_prompt(vt_cfg).to_string();

    Ok(SessionState {
        session_bootstrap,
        startup_update_check,
        provider_client,
        tool_registry: None,
        tools,
        tool_catalog,
        conversation_history,
        execution: ToolExecutionContext {
            tool_result_cache,
            tool_permission_cache,
            permissions_state,
            approval_recorder,
            safety_validator: Arc::new(ToolCallSafetyValidator::with_gateway(shared_safety_gateway)),
            circuit_breaker: circuit_breaker.clone(),
            tool_health_tracker: Arc::new(vtcode_core::tools::health::ToolHealthTracker::new(50)),
            rate_limiter: Arc::new(vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter::default()),
            validation_cache: Arc::new(vtcode_core::tools::validation_cache::ValidationCache::default()),
            autonomous_executor: {
                let executor = vtcode_core::tools::autonomous_executor::AutonomousExecutor::new();
                if let Some(cfg) = vt_cfg {
                    let loop_limits: HashMap<_, _> =
                        cfg.tools.loop_thresholds.iter().map(|(k, v)| (k.clone(), *v)).collect();
                    executor.configure_loop_limits(&loop_limits);
                }
                Arc::new(executor)
            },
        },
        metadata: SessionMetadataContext {
            decision_ledger,
            trajectory: vtcode_core::core::trajectory::TrajectoryLogger::disabled(),
            telemetry: Arc::new(vtcode_core::core::telemetry::TelemetryManager::new()),
            error_recovery: Arc::new(RwLock::new(vtcode_core::core::agent::error_recovery::ErrorRecoveryState::new())),
        },
        base_system_prompt,
        full_auto_allowlist: None,
        async_mcp_manager,
        mcp_panel_state,
        loaded_skills: skill_setup.active_skills_map,
        active_primary_agent,
        discovered_subagents: None,
    })
}

/// Build `ToolRegistry` and run workspace/plugin discovery after first paint.
pub(crate) async fn complete_session_registry(
    session_state: &mut SessionState,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    full_auto: bool,
    primary_agent_explicitly_configured: bool,
    resume: Option<&ResumeSession>,
    parent_session_id: &str,
    session_primary_agent_override: Option<&str>,
) -> Result<()> {
    let registry_phase = vtcode_commons::startup_trace::phase_started();
    let workspace_for_registry = config.workspace.clone();
    let workspace_for_discovery = config.workspace.clone();
    let vt_cfg_snapshot = vt_cfg.cloned();
    let (tool_registry, discovered) = tokio::join!(
        async move {
            if let Some(snapshot) = vt_cfg_snapshot.as_ref() {
                ToolRegistry::new_for_first_paint_with_loaded_config(workspace_for_registry, snapshot).await
            } else {
                ToolRegistry::new(workspace_for_registry).await
            }
        },
        async move { vtcode_core::subagents::discover_controller_subagents(&workspace_for_discovery).await },
    );
    let tool_registry = tool_registry;
    tool_registry.set_harness_session(parent_session_id.to_string());

    let resumed_primary_agent = resume
        .and_then(|r| r.snapshot().metadata.primary_agent.clone())
        .or_else(|| session_primary_agent_override.map(str::to_owned));
    let discovered =
        discovered.with_context(|| format!("Failed to discover primary agents in {}", config.workspace.display()))?;
    let active_primary_agent = active_primary_agent_from_specs_for_mode(
        &discovered.effective,
        vt_cfg,
        full_auto,
        primary_agent_explicitly_configured,
        resumed_primary_agent.clone(),
    )?;
    tool_registry.set_matrix_coordinator(active_primary_agent.active().name() == "coordinator");

    let tools = session_state.tools.clone();
    tool_registry.attach_session_model_tools(tools.clone());
    // Rebuild the shared breaker with registry metrics now that it exists.
    let circuit_breaker = Arc::new(vtcode_core::tools::circuit_breaker::CircuitBreaker::with_metrics(
        vtcode_config_circuit_breaker_to_core(vt_cfg, config),
        tool_registry.metrics_collector(),
    ));
    tool_registry.set_shared_circuit_breaker(circuit_breaker.clone());
    session_state.execution.circuit_breaker = circuit_breaker;
    session_state.execution.safety_validator =
        Arc::new(ToolCallSafetyValidator::with_gateway(tool_registry.safety_gateway()));
    session_state.tool_catalog = tool_registry.tool_catalog_state();
    session_state.tool_registry = Some(tool_registry);
    session_state.active_primary_agent = active_primary_agent;
    session_state.discovered_subagents = Some(discovered);
    vtcode_commons::startup_trace::record_phase("session_setup_registry", registry_phase);
    Ok(())
}

/// Finish session runtime setup after the TUI first frame.
///
/// Must complete before the interaction loop dispatches the first model turn.
/// Failures here abort the session with the same setup error surface as the
/// previous pre-paint `initialize_session` path.
#[allow(
    clippy::too_many_arguments,
    reason = "session hydration mirrors initialize_session_critical inputs"
)]
pub(crate) async fn hydrate_session_runtime(
    session_state: &mut SessionState,
    context_manager: &mut ContextManager,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    full_auto: bool,
    primary_agent_explicitly_configured: bool,
    resume: Option<&ResumeSession>,
    parent_session_id: &str,
    session_primary_agent_override: Option<&str>,
) -> Result<()> {
    let tool_documentation_mode = vt_cfg.map(|cfg| cfg.agent.tool_documentation_mode).unwrap_or_default();
    let resumed_primary_agent = resume
        .and_then(|r| r.snapshot().metadata.primary_agent.clone())
        .or_else(|| session_primary_agent_override.map(str::to_owned));

    // Enrich bootstrap metadata that required workspace scans.
    let full_bootstrap = prepare_session_bootstrap_with_mode(config, vt_cfg, None, SessionBootstrapMode::Full).await;
    session_state.session_bootstrap.prompt_addendum = full_bootstrap.prompt_addendum;
    if session_state.session_bootstrap.placeholder.is_none() {
        session_state.session_bootstrap.placeholder = full_bootstrap.placeholder;
    }

    // Deferred post-paint maintenance reads: update-cache and release-notes
    // file I/O moved off the first-paint path. Merge here so hydrated UI
    // (header highlights, update prompt, release notes) matches the prior
    // pre-paint behavior before the first model turn.
    // Join with nothing else here yet; both are local cache reads and cheap
    // relative to tool-registry init below, but keeping them out of
    // `initialize_session_critical` saves ~10-50ms on cold caches.
    if session_state.startup_update_check.cached_notice.is_none() {
        let deferred_check = load_startup_update_check();
        if let Some(notice) = deferred_check.cached_notice.as_ref() {
            append_notice_highlight(&mut session_state.session_bootstrap.header_highlights, notice);
        }
        // Preserve a background refresh request from the deferred check.
        if deferred_check.should_refresh {
            session_state.startup_update_check.should_refresh = true;
        }
        if session_state.startup_update_check.cached_notice.is_none() {
            session_state.startup_update_check.cached_notice = deferred_check.cached_notice;
        }
    }
    if session_state.session_bootstrap.release_highlights.is_none() {
        session_state.session_bootstrap.release_highlights = load_release_highlights_for_startup().await;
    }

    // Finish deferred approval-cache setup off the paint path: create the
    // directory and load persisted patterns before the first model turn so
    // auto-approval history matches the prior pre-paint behavior. Fail-open:
    // an empty in-memory map is safe, writes self-ensure later.
    if let Ok(paths) = VtCodePaths::resolve() {
        if let Err(err) = paths.ensure_cache_child_dir("approval") {
            warn!("Failed to create approval cache directory during hydration: {err:#}");
        }
    } else {
        warn!("Failed to resolve approval cache paths during hydration");
    }
    session_state.execution.approval_recorder.reload().await;

    let deferred_tool_policy = active_deferred_tool_policy(config, vt_cfg, &*session_state.provider_client);

    let Some(tool_registry) = session_state.tool_registry.as_mut() else {
        anyhow::bail!("session hydration requires the post-paint tool registry (complete_session_registry)");
    };
    // Attach the workspace policy manager skipped on the paint path before any
    // tool runs; evaluation fails open to metadata defaults until then.
    tool_registry.ensure_workspace_policy_manager(&config.workspace).await;
    tool_registry.initialize_async().await?;
    if let Some(cfg) = vt_cfg {
        if let Err(err) = tool_registry
            .apply_session_runtime_config(&cfg.commands, &cfg.permissions, &cfg.sandbox, &cfg.timeouts, &cfg.tools)
            .await
        {
            warn!("Failed to apply tool policies from config: {}", err);
        }
        maybe_attach_mcp_client(tool_registry, cfg, session_state.async_mcp_manager.as_ref()).await;
    }

    let workspace_trust_level = match session_state.session_bootstrap.acp_workspace_trust {
        Some(level) => Some(level.to_workspace_trust_level()),
        None => load_workspace_trust_level(&config.workspace)
            .await
            .context("Failed to determine workspace trust level for tool policy")?,
    };
    apply_workspace_trust_prompt_policy(tool_registry, full_auto, workspace_trust_level).await;

    let subagent_controller = if resume.is_none_or(ResumeSession::is_root_thread)
        && let Some(cfg) = vt_cfg
        && cfg.subagents.enabled
    {
        let workspace_gated = cfg.workspace_lifecycle_hooks.as_ref().is_some_and(|hooks| !hooks.is_empty())
            || session_state
                .active_primary_agent
                .active()
                .contributes_workspace_controlled_hooks();
        let controller_config = SubagentControllerConfig {
            workspace_root: config.workspace.clone(),
            parent_session_id: parent_session_id.to_string(),
            parent_model: config.model.clone(),
            parent_provider: config.provider.clone(),
            parent_reasoning_effort: config.reasoning_effort,
            api_key: config.api_key.clone(),
            vt_cfg: cfg.clone(),
            openai_chatgpt_auth: config.openai_chatgpt_auth.clone(),
            depth: 0,
            workspace_gated,
            exec_sessions: tool_registry.exec_session_manager(),
            pty_manager: tool_registry.pty_manager().clone(),
            managed_background_runtime: false,
        };
        // Reuse the critical-path discovery result when available so hydration
        // skips a second workspace/plugin scan; fall back to full discovery
        // for embedded callers without critical-path state.
        let controller_result = if let Some(discovered) = session_state.discovered_subagents.clone() {
            SubagentController::new_with_discovered(controller_config, discovered).await
        } else {
            SubagentController::new(controller_config).await
        };
        match controller_result {
            Ok(controller) => {
                controller.set_parent_messages(&session_state.conversation_history).await;
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

    let cgp_mode = if full_auto {
        vtcode_core::tools::CgpRuntimeMode::Ci
    } else {
        vtcode_core::tools::CgpRuntimeMode::Interactive
    };
    tool_registry.enable_cgp_pipeline(cgp_mode).await;

    let tool_catalog = session_state.tool_catalog.clone();
    let anthropic_native_memory_enabled =
        active_anthropic_native_memory(config, vt_cfg, &*session_state.provider_client);
    let tools = session_state.tools.clone();
    {
        let next_tools = tool_registry
            .model_tools(interactive_session_base_tools_config(
                &config.model,
                vt_cfg,
                tool_documentation_mode,
                deferred_tool_policy.clone(),
                anthropic_native_memory_enabled,
            ))
            .await;
        *tools.write().await = next_tools;
    }
    tool_registry.attach_session_model_tools(tools.clone());
    let skill_setup = SkillSetupState {
        active_skills_map: session_state.loaded_skills.clone(),
    };
    register_skill_tools(
        tool_registry,
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
        tool_registry,
        &tools,
        &tool_catalog,
        config,
        vt_cfg,
        tool_documentation_mode,
        &deferred_tool_policy,
    )
    .await;

    if full_auto && let Some(cfg) = vt_cfg {
        let session_tools_config = interactive_session_tools_config(
            &config.model,
            vt_cfg,
            tool_documentation_mode,
            deferred_tool_policy.clone(),
            anthropic_native_memory_enabled,
            tool_registry.is_planning_active(),
        );
        tool_registry
            .enable_full_auto_permission_for_session(&cfg.automation.full_auto.allowed_tools, session_tools_config)
            .await;
        session_state.full_auto_allowlist = Some(tool_registry.current_full_auto_allowlist().await.unwrap_or_default());
    }

    session_state.metadata.trajectory = build_trajectory_logger(&config.workspace, vt_cfg).await;

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

    let (base_system_prompt, system_prompt_report) = read_system_prompt(
        &config.workspace,
        session_state.session_bootstrap.prompt_addendum.as_deref(),
        &available_subagents,
    )
    .await;
    session_state.session_bootstrap.system_prompt_report = system_prompt_report;
    session_state.base_system_prompt = base_system_prompt.clone();
    context_manager.set_base_system_prompt(base_system_prompt);

    if let Some(controller) = subagent_controller.as_ref() {
        // Controller specs are authoritative once constructed.
        session_state.active_primary_agent = active_primary_agent_from_specs_for_mode(
            &controller.effective_specs().await,
            vt_cfg,
            full_auto,
            primary_agent_explicitly_configured,
            resumed_primary_agent.clone(),
        )?;
    }

    if let Some(registry) = session_state.tool_registry.as_ref() {
        registry.set_matrix_coordinator(session_state.active_primary_agent.active().name() == "coordinator");
    }
    if let (Some(manager), Some(cfg)) = (session_state.async_mcp_manager.as_ref(), vt_cfg) {
        let mcp_config =
            session_mcp_config(Some(cfg), Some(session_state.active_primary_agent.active()), &config.workspace);
        manager.reconfigure(mcp_config).await?;
        if let Err(err) = manager.start_initialization() {
            warn!("MCP background initialization did not restart after session config merge: {err:#}");
        }
    }

    if let Some(cfg) = vt_cfg
        && cfg.context.dynamic.enabled
        && let Err(err) =
            vtcode_core::context::initialize_dynamic_context(&config.workspace, &cfg.context.dynamic).await
    {
        warn!("Failed to initialize dynamic context directories: {}", err);
    }

    Ok(())
}

fn load_startup_update_check() -> crate::updater::StartupUpdateCheck {
    // The preflight check ran at binary startup and already fetched from
    // GitHub (force fetch), respecting the user's check_interval_hours,
    // pinned-version, and release_channel config.  Use its result when
    // available — it is always fresher than the on-disk cache.
    if let Some(notice) = crate::updater::get_preflight_notice() {
        return crate::updater::StartupUpdateCheck { cached_notice: Some(notice), should_refresh: false };
    }

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

async fn load_release_highlights_for_startup() -> Option<(semver::Version, Vec<String>)> {
    if !crate::updater::should_show_release_notes_for_current_version() {
        return None;
    }
    crate::updater::cached_current_release_highlights()
}

/// Test helper that creates the MCP manager and starts background init
/// immediately (pre-split production behavior).
#[cfg(test)]
fn create_async_mcp_manager(
    vt_cfg: Option<&VTCodeConfig>,
    active_primary_agent: Option<&ActivePrimaryAgent>,
    workspace_root: &Path,
) -> Option<Arc<AsyncMcpManager>> {
    create_async_mcp_manager_inner(vt_cfg, active_primary_agent, workspace_root, true)
}

/// Create the session MCP manager.
///
/// `start_background_init=false` is used on the interactive critical path so
/// first paint does not pay MCP process startup; hydration reconfigures with
/// the resolved primary agent and then starts initialization once.
fn create_async_mcp_manager_inner(
    vt_cfg: Option<&VTCodeConfig>,
    active_primary_agent: Option<&ActivePrimaryAgent>,
    workspace_root: &Path,
    start_background_init: bool,
) -> Option<Arc<AsyncMcpManager>> {
    let cfg = vt_cfg?;
    if !cfg.mcp.enabled {
        debug!("MCP is disabled in configuration");
        return None;
    }
    let mcp_config = session_mcp_config(vt_cfg, active_primary_agent, workspace_root);

    info!("Setting up async MCP client with {} providers", mcp_config.providers.len());
    let approval_policy = approval_policy_from_human_in_the_loop(cfg.security.human_in_the_loop);
    let sandbox_context = if cfg.sandbox.enabled
        && !matches!(cfg.sandbox.default_policy, vtcode_config::SandboxPolicy::DangerFullAccess)
    {
        let policy =
            match vtcode_core::tools::registry::sandbox_policy_from_runtime_config(&cfg.sandbox, workspace_root) {
                Ok(policy) => policy,
                Err(error) => {
                    error!("Unable to construct the MCP sandbox policy: {error}");
                    return None;
                }
            };
        Some(vtcode_core::mcp::McpSandboxContext::new(policy, workspace_root))
    } else {
        None
    };

    let manager = AsyncMcpManager::new_with_sandbox(
        mcp_config,
        cfg.security.hitl_notification_bell,
        approval_policy,
        Arc::new(|_event: mcp_events::McpEvent| {}),
        sandbox_context,
    );
    let manager = Arc::new(manager);
    if start_background_init
        && let Err(err) = manager
            .start_initialization()
            .context("failed to start MCP initialization task")
    {
        warn!("MCP background initialization did not start: {err:#}");
    }
    Some(manager)
}

/// Build the session MCP config: the base config (optionally merged with the
/// active primary agent's `mcp_servers`) plus any `mcp.json`-declared plugin
/// providers discovered from `<workspace>/.agents/plugins` and
/// `~/.agents/plugins`.
///
/// Centralized so session bootstrap and live `/plugin refresh` reconfigure
/// produce identical configurations.
pub(crate) fn session_mcp_config(
    vt_cfg: Option<&VTCodeConfig>,
    active_primary_agent: Option<&ActivePrimaryAgent>,
    workspace_root: &Path,
) -> vtcode_core::config::mcp::McpClientConfig {
    let cfg = vt_cfg.expect("MCP config requires a loaded VTCodeConfig");
    let mut mcp_config = active_primary_agent
        .map(|agent| primary_agent_mcp_config(cfg, agent))
        .unwrap_or_else(|| cfg.mcp.clone());

    let plugin_providers = discover_plugin_mcp_providers(workspace_root);
    let plugin_provider_count = plugin_providers.len();
    if !plugin_providers.is_empty() {
        mcp_config.providers.extend(plugin_providers);
        info!("Added {} plugin-provided MCP providers", plugin_provider_count);
    }
    mcp_config
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
        McpInitStatus::Initializing { .. } | McpInitStatus::Disabled | McpInitStatus::Ready { .. } => None,
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
            anthropic: configured_anthropic_config(vt_cfg),
            model_behavior: vt_cfg.map(|cfg| cfg.model.clone()),
            workspace_root: Some(config.workspace.clone()),
        },
    )
    .context("Failed to initialize provider client")
}

/// `[provider.anthropic]` settings for a runtime provider client.
///
/// The startup client and every client rebuilt mid-session (model switch, API
/// key update, OAuth sync) read the Anthropic section through this helper, so a
/// rebuilt client keeps the thinking, advisor, fallback, and budget settings
/// the startup client had instead of silently reverting to defaults.
pub(crate) fn configured_anthropic_config(vt_cfg: Option<&VTCodeConfig>) -> Option<AnthropicConfig> {
    vt_cfg.map(|cfg| cfg.provider.anthropic.clone())
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
        .model_tools(interactive_session_base_tools_config(
            &config.model,
            vt_cfg,
            tool_documentation_mode,
            deferred_tool_policy.clone(),
            anthropic_native_memory_enabled,
        ))
        .await;
    *tools.write().await = next;
}

fn interactive_session_tools_config(
    model: &str,
    vt_cfg: Option<&VTCodeConfig>,
    tool_documentation_mode: vtcode_core::config::ToolDocumentationMode,
    deferred_tool_policy: DeferredToolPolicy,
    anthropic_native_memory_enabled: bool,
    planning_active: bool,
) -> SessionToolsConfig {
    SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        vtcode_core::config::types::CapabilityLevel::CodeSearch,
        tool_documentation_mode,
        ToolModelCapabilities::for_model_name(model),
    )
    .with_planning_active(planning_active)
    .with_deferred_tool_policy(deferred_tool_policy)
    .with_anthropic_native_memory_enabled(anthropic_native_memory_enabled)
    .with_tool_profile(vt_cfg.map(|cfg| cfg.tools.profile).unwrap_or_default())
}

fn interactive_session_base_tools_config(
    model: &str,
    vt_cfg: Option<&VTCodeConfig>,
    tool_documentation_mode: vtcode_core::config::ToolDocumentationMode,
    deferred_tool_policy: DeferredToolPolicy,
    anthropic_native_memory_enabled: bool,
) -> SessionToolsConfig {
    // The attached list is a stable superset. Each turn filters it with the
    // registry's current planning state before exposing tools to the model.
    interactive_session_tools_config(
        model,
        vt_cfg,
        tool_documentation_mode,
        deferred_tool_policy,
        anthropic_native_memory_enabled,
        true,
    )
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
        *tool_registry = tool_registry.clone().with_mcp_client(Arc::clone(client)).await;
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
    let enforce_safe_mode_prompts =
        should_enforce_safe_mode_prompts(false, auto_permission_review_active, workspace_trust_level);
    tool_registry.set_enforce_safe_mode_prompts(enforce_safe_mode_prompts).await;
}

#[cfg(test)]
mod tests {
    #[test]
    fn critical_path_avoids_registry_and_discovery() {
        let src = include_str!("init.rs");
        // Use full source: a #[cfg(test)] helper sits above initialize_session_critical.
        let production = src;
        let start = production
            .find("pub(crate) async fn initialize_session_critical")
            .expect("critical present");
        let end = production
            .find("pub(crate) async fn complete_session_registry")
            .expect("completer present");
        let critical = &production[start..end];
        for forbidden in ["ToolRegistry::new", "discover_controller_subagents"] {
            assert!(!critical.contains(forbidden), "critical must not call {forbidden}");
        }
        assert!(production.contains("session_setup_registry"), "completer emits phase");
    }

    use std::collections::BTreeMap;

    use clap::Parser;
    use serde_json::json;
    use tempfile::TempDir;
    use vtcode_config::{
        AgentMode, SubagentMcpServer, SubagentSource, SubagentSpec, ToolProfile,
        core::permissions::{AgentPermissionsConfig, PermissionDefault},
    };
    use vtcode_core::cli::args::Cli;
    use vtcode_core::config::constants::tools;
    use vtcode_core::config::types::ModelSelectionSource;
    use vtcode_core::core::agent::config::{RuntimeModelSelection, build_runtime_agent_config};

    use super::*;

    #[tokio::test]
    async fn interactive_catalogue_uses_configured_tool_profile() {
        let temp = TempDir::new().expect("temp dir");
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        let mut advanced = VTCodeConfig::default();
        advanced.tools.profile = ToolProfile::AdvancedVtCode;

        let advanced_tools = registry
            .model_tools(interactive_session_tools_config(
                "gpt-5",
                Some(&advanced),
                vtcode_core::config::ToolDocumentationMode::default(),
                DeferredToolPolicy::default(),
                false,
                registry.is_planning_active(),
            ))
            .await;
        let default_tools = registry
            .model_tools(interactive_session_tools_config(
                "gpt-5",
                None,
                vtcode_core::config::ToolDocumentationMode::default(),
                DeferredToolPolicy::default(),
                false,
                registry.is_planning_active(),
            ))
            .await;

        assert!(advanced_tools.iter().any(|tool| tool.function_name() == tools::CODE_SEARCH));
        assert!(default_tools.iter().all(|tool| tool.function_name() != tools::CODE_SEARCH));
    }

    #[tokio::test]
    async fn planning_transition_exposes_planning_tools_from_attached_base() {
        let temp = TempDir::new().expect("temp dir");
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        let base_tools = Arc::new(RwLock::new(
            registry
                .model_tools(interactive_session_base_tools_config(
                    "gpt-5",
                    None,
                    vtcode_core::config::ToolDocumentationMode::default(),
                    DeferredToolPolicy::default(),
                    false,
                ))
                .await,
        ));
        let tool_catalog = registry.tool_catalog_state();

        let inactive = tool_catalog
            .filtered_snapshot_with_stats(&base_tools, registry.is_planning_active(), false)
            .await;
        let inactive_names = inactive.active_tool_names.as_ref();
        // `request_user_input` is gated by the interactive/planning capability
        // signal (passed as `request_user_input_enabled = false` here), so it is
        // absent until planning enables it. `code_search` is a read-only tool and
        // is intentionally exposed in both modes (planning-inactive exposes every
        // non-gated tool; planning keeps read-only tools) — see
        // `FeatureSet::tool_enabled_for_mode`.
        assert!(inactive_names.iter().any(|name| name == tools::CODE_SEARCH));
        assert!(!inactive_names.iter().any(|name| name == tools::REQUEST_USER_INPUT));

        registry.enable_planning();
        let active = tool_catalog
            .filtered_snapshot_with_stats(&base_tools, registry.is_planning_active(), true)
            .await;
        let active_names = active.active_tool_names.as_ref();
        assert!(active_names.iter().any(|name| name == tools::CODE_SEARCH));
        assert!(active_names.iter().any(|name| name == tools::REQUEST_USER_INPUT));
    }

    #[tokio::test]
    async fn mcp_refresh_route_retains_configured_tool_profile() {
        let temp = TempDir::new().expect("temp dir");
        let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
        let tool_catalog = registry.tool_catalog_state();
        let active_tools = Arc::new(RwLock::new(Vec::new()));
        let mut advanced = VTCodeConfig::default();
        advanced.tools.profile = ToolProfile::AdvancedVtCode;
        let cli = Cli::parse_from(["vtcode"]);
        let runtime_config = build_runtime_agent_config(
            &cli,
            &advanced,
            temp.path().to_path_buf(),
            RuntimeModelSelection {
                model: "gpt-5".to_string(),
                provider: "openai".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
                model_source: ModelSelectionSource::WorkspaceConfig,
            },
            "test-key".to_string(),
            vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        );

        refresh_tool_snapshot(
            &registry,
            &active_tools,
            tool_catalog.as_ref(),
            &runtime_config,
            Some(&advanced),
            vtcode_core::config::ToolDocumentationMode::default(),
            &DeferredToolPolicy::default(),
        )
        .await;

        assert!(
            active_tools
                .read()
                .await
                .iter()
                .any(|tool| tool.function_name() == tools::CODE_SEARCH)
        );
    }

    #[tokio::test]
    async fn enabled_session_mcp_manager_starts_initialization_task() {
        let mut cfg = VTCodeConfig::default();
        cfg.mcp.enabled = true;

        let manager = create_async_mcp_manager(Some(&cfg), None, Path::new("/tmp")).expect("manager should exist");

        assert!(manager.has_initialization_task(), "enabled session MCP manager should start in the background");
    }

    #[tokio::test]
    async fn critical_path_defers_mcp_init_until_hydrate() {
        let temp = TempDir::new().expect("temp dir");
        let mut cfg = VTCodeConfig::default();
        cfg.mcp.enabled = true;
        let cli = Cli::parse_from(["vtcode"]);
        let runtime_config = build_runtime_agent_config(
            &cli,
            &cfg,
            temp.path().to_path_buf(),
            RuntimeModelSelection {
                model: "gpt-5".to_string(),
                provider: "openai".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
                model_source: ModelSelectionSource::WorkspaceConfig,
            },
            "test-key".to_string(),
            vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        );

        let mut critical =
            initialize_session_critical(&runtime_config, Some(&cfg), false, false, None, "test-mcp-defer", None)
                .await
                .expect("critical session");
        let manager = critical.async_mcp_manager.clone().expect("MCP manager when enabled");
        assert!(
            !manager.has_initialization_task(),
            "critical path must not start MCP background init before first paint"
        );

        let mut context_manager = ContextManager::new(
            critical.base_system_prompt.clone(),
            (),
            critical.loaded_skills.clone(),
            Some(cfg.agent.clone()),
        );
        context_manager.set_workspace_root(&runtime_config.workspace);
        complete_session_registry(&mut critical, &runtime_config, Some(&cfg), false, false, None, "test-hydrate", None)
            .await
            .expect("complete registry");
        hydrate_session_runtime(
            &mut critical,
            &mut context_manager,
            &runtime_config,
            Some(&cfg),
            false,
            false,
            None,
            "test-mcp-defer",
            None,
        )
        .await
        .expect("hydrate session");
        assert!(manager.has_initialization_task(), "hydration must start MCP init after primary-agent reconfigure");
    }

    #[tokio::test]
    async fn initialize_session_restarts_mcp_background_task_after_merge() {
        let temp = TempDir::new().expect("temp dir");
        let mut cfg = VTCodeConfig::default();
        cfg.mcp.enabled = true;
        let cli = Cli::parse_from(["vtcode"]);
        let runtime_config = build_runtime_agent_config(
            &cli,
            &cfg,
            temp.path().to_path_buf(),
            RuntimeModelSelection {
                model: "gpt-5".to_string(),
                provider: "openai".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
                model_source: ModelSelectionSource::WorkspaceConfig,
            },
            "test-key".to_string(),
            vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        );

        let state = initialize_session(&runtime_config, Some(&cfg), false, false, None, "test-session", None)
            .await
            .expect("initialize session");

        let manager = state.async_mcp_manager.expect("MCP manager should exist when enabled");
        assert!(
            manager.has_initialization_task(),
            "session MCP background task must survive primary-agent merge without manual /mcp repair"
        );
    }

    #[tokio::test]
    async fn hydrate_replaces_critical_seed_prompt_and_fills_tools() {
        let temp = TempDir::new().expect("temp dir");
        let cfg = VTCodeConfig::default();
        let cli = Cli::parse_from(["vtcode"]);
        let runtime_config = build_runtime_agent_config(
            &cli,
            &cfg,
            temp.path().to_path_buf(),
            RuntimeModelSelection {
                model: "gpt-5".to_string(),
                provider: "openai".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
                model_source: ModelSelectionSource::WorkspaceConfig,
            },
            "test-key".to_string(),
            vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        );

        let mut critical =
            initialize_session_critical(&runtime_config, Some(&cfg), false, false, None, "test-hydrate", None)
                .await
                .expect("critical session");
        assert!(
            critical.tools.read().await.is_empty(),
            "critical path must leave model tools empty for first-frame deferral"
        );
        let seed_prompt = critical.base_system_prompt.clone();
        assert!(!seed_prompt.trim().is_empty(), "critical path seeds a fallback system prompt");

        let mut context_manager = ContextManager::new(
            critical.base_system_prompt.clone(),
            (),
            critical.loaded_skills.clone(),
            Some(cfg.agent.clone()),
        );
        context_manager.set_workspace_root(&runtime_config.workspace);
        complete_session_registry(&mut critical, &runtime_config, Some(&cfg), false, false, None, "test-hydrate", None)
            .await
            .expect("complete registry");
        hydrate_session_runtime(
            &mut critical,
            &mut context_manager,
            &runtime_config,
            Some(&cfg),
            false,
            false,
            None,
            "test-hydrate",
            None,
        )
        .await
        .expect("hydrate session");
        assert!(
            !critical.tools.read().await.is_empty(),
            "hydration must project model tools before the first model turn"
        );
        assert_ne!(
            critical.base_system_prompt, seed_prompt,
            "hydration must replace the seed system prompt with the composed workspace prompt"
        );
    }

    #[tokio::test]
    async fn hydrate_reuses_critical_discovery_for_controller() {
        let temp = TempDir::new().expect("temp dir");
        let cfg = VTCodeConfig::default();
        let cli = Cli::parse_from(["vtcode"]);
        let runtime_config = build_runtime_agent_config(
            &cli,
            &cfg,
            temp.path().to_path_buf(),
            RuntimeModelSelection {
                model: "gpt-5".to_string(),
                provider: "openai".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
                model_source: ModelSelectionSource::WorkspaceConfig,
            },
            "test-key".to_string(),
            vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        );

        let mut critical =
            initialize_session_critical(&runtime_config, Some(&cfg), false, false, None, "test-reuse", None)
                .await
                .expect("critical session");
        complete_session_registry(&mut critical, &runtime_config, Some(&cfg), false, false, None, "test-reuse", None)
            .await
            .expect("complete registry");
        let critical_names = critical
            .discovered_subagents
            .as_ref()
            .expect("complete_session_registry must cache discovery")
            .effective
            .iter()
            .map(|spec| spec.name.clone())
            .collect::<Vec<_>>();

        let mut context_manager = ContextManager::new(
            critical.base_system_prompt.clone(),
            (),
            critical.loaded_skills.clone(),
            Some(cfg.agent.clone()),
        );
        context_manager.set_workspace_root(&runtime_config.workspace);
        hydrate_session_runtime(
            &mut critical,
            &mut context_manager,
            &runtime_config,
            Some(&cfg),
            false,
            false,
            None,
            "test-reuse",
            None,
        )
        .await
        .expect("hydrate session");

        let controller = critical
            .tool_registry
            .as_ref()
            .expect("registry")
            .subagent_controller()
            .expect("controller when subagents enabled");
        let controller_names = controller
            .effective_specs()
            .await
            .into_iter()
            .map(|spec| spec.name)
            .collect::<Vec<_>>();
        for name in critical_names {
            assert!(
                controller_names.contains(&name),
                "hydrated controller must reuse critical discovery, missing {name}"
            );
        }
    }

    #[tokio::test]
    async fn critical_path_defers_policy_manager_to_hydrate() {
        let temp = TempDir::new().expect("temp dir");
        let cfg = VTCodeConfig::default();
        let cli = Cli::parse_from(["vtcode"]);
        let runtime_config = build_runtime_agent_config(
            &cli,
            &cfg,
            temp.path().to_path_buf(),
            RuntimeModelSelection {
                model: "gpt-5".to_string(),
                provider: "openai".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
                model_source: ModelSelectionSource::WorkspaceConfig,
            },
            "test-key".to_string(),
            vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        );

        let mut critical =
            initialize_session_critical(&runtime_config, Some(&cfg), false, false, None, "test-defer-policy", None)
                .await
                .expect("critical session");
        assert!(critical.tool_registry.is_none(), "critical path must not build a tool registry before first paint");

        let mut context_manager = ContextManager::new(
            critical.base_system_prompt.clone(),
            (),
            critical.loaded_skills.clone(),
            Some(cfg.agent.clone()),
        );
        context_manager.set_workspace_root(&runtime_config.workspace);
        complete_session_registry(&mut critical, &runtime_config, Some(&cfg), false, false, None, "test-hydrate", None)
            .await
            .expect("complete registry");
        hydrate_session_runtime(
            &mut critical,
            &mut context_manager,
            &runtime_config,
            Some(&cfg),
            false,
            false,
            None,
            "test-defer-policy",
            None,
        )
        .await
        .expect("hydrate session");
        assert!(
            critical
                .tool_registry
                .as_ref()
                .expect("hydrated registry")
                .has_policy_manager()
                .await,
            "hydration must attach the policy manager before the first turn"
        );
    }

    #[tokio::test]
    async fn critical_path_defers_update_and_release_notes_to_hydrate() {
        let temp = TempDir::new().expect("temp dir");
        let cfg = VTCodeConfig::default();
        let cli = Cli::parse_from(["vtcode"]);
        let runtime_config = build_runtime_agent_config(
            &cli,
            &cfg,
            temp.path().to_path_buf(),
            RuntimeModelSelection {
                model: "gpt-5".to_string(),
                provider: "openai".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
                model_source: ModelSelectionSource::WorkspaceConfig,
            },
            "test-key".to_string(),
            vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        );

        let mut critical =
            initialize_session_critical(&runtime_config, Some(&cfg), false, false, None, "test-defer-update", None)
                .await
                .expect("critical session");
        // First-paint path must not pay update-cache / release-notes file I/O.
        // Only an in-memory preflight notice (if present) may appear here.
        let preflight = crate::updater::get_preflight_notice();
        assert_eq!(
            critical.startup_update_check.cached_notice, preflight,
            "critical update check must be preflight-only, no disk read"
        );
        assert!(
            critical.session_bootstrap.release_highlights.is_none(),
            "critical path must defer release-notes read to hydration"
        );

        let mut context_manager = ContextManager::new(
            critical.base_system_prompt.clone(),
            (),
            critical.loaded_skills.clone(),
            Some(cfg.agent.clone()),
        );
        context_manager.set_workspace_root(&runtime_config.workspace);
        complete_session_registry(&mut critical, &runtime_config, Some(&cfg), false, false, None, "test-hydrate", None)
            .await
            .expect("complete registry");
        hydrate_session_runtime(
            &mut critical,
            &mut context_manager,
            &runtime_config,
            Some(&cfg),
            false,
            false,
            None,
            "test-defer-update",
            None,
        )
        .await
        .expect("hydrate session");
        // Hydration completes the deferred reads without breaking parity:
        // cached notice (if any) must have a matching header highlight.
        if let Some(notice) = critical.startup_update_check.cached_notice.as_ref() {
            assert!(
                !critical.session_bootstrap.header_highlights.is_empty(),
                "hydrated update notice must surface a header highlight"
            );
            let _ = notice;
        }
    }

    #[tokio::test]
    async fn async_mcp_manager_uses_primary_agent_merged_mcp_config() {
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
            create_async_mcp_manager(Some(&cfg), Some(&active), Path::new("/tmp")).expect("manager should exist");
        let manager_config = manager.config();
        let provider_names = manager_config
            .providers
            .iter()
            .map(|provider| provider.name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(provider_names, vec!["global", "local"]);
        assert!(!matches!(manager.get_status().await, McpInitStatus::Disabled));
    }

    #[test]
    fn session_mcp_config_includes_plugin_providers() {
        use std::fs;

        let tmp = TempDir::new().expect("temp dir");
        let workspace = tmp.path();
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

        // A plugin with an mcp.json stdio server under the workspace plugin root.
        let plugin_root = workspace.join(".agents/plugins/test-plugin");
        fs::create_dir_all(plugin_root.join("skills")).expect("create plugin dirs");
        fs::write(
            plugin_root.join("plugin.json"),
            r#"{
                "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                "name": "test-plugin",
                "version": "1.0.0",
                "description": "Test plugin"
            }"#,
        )
        .expect("write plugin.json");
        fs::write(
            plugin_root.join("mcp.json"),
            r#"{
                "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
                "mcpServers": {
                    "search": {
                        "type": "stdio",
                        "command": "./bin/search",
                        "args": []
                    }
                }
            }"#,
        )
        .expect("write mcp.json");

        let config = session_mcp_config(Some(&cfg), None, workspace);
        let provider_names = config
            .providers
            .iter()
            .map(|provider| provider.name.as_str())
            .collect::<Vec<_>>();

        assert!(
            provider_names.contains(&"test-plugin.search"),
            "expected plugin provider in session config, got: {provider_names:?}"
        );
        assert!(provider_names.contains(&"global"), "expected configured provider retained, got: {provider_names:?}");
    }

    #[test]
    fn startup_primary_agent_defaults_to_build_without_config() {
        let active = active_primary_agent_from_specs(&[test_primary_agent_spec("builder")], None)
            .expect("default primary agent");

        assert_eq!(active.active().identity.name, "build");
        assert_eq!(active.active().identity.source, SubagentSource::Builtin);
    }

    #[test]
    fn startup_primary_agent_uses_default_primary_agent_config() {
        let mut cfg = VTCodeConfig {
            default_primary_agent: "builder".to_string(),
            ..VTCodeConfig::default()
        };
        let active = active_primary_agent_from_specs(&[test_primary_agent_spec("builder")], Some(&cfg))
            .expect("configured primary agent");

        assert_eq!(active.active().identity.name, "builder");

        cfg.default_primary_agent = "missing".to_string();
        let fallback = active_primary_agent_from_specs(&[test_primary_agent_spec("builder")], Some(&cfg))
            .expect("fallback primary agent");

        assert_eq!(fallback.active().identity.name, "build");
        assert_eq!(fallback.active().identity.source, SubagentSource::Builtin);
    }

    #[test]
    fn full_auto_with_defaulted_primary_agent_selects_effective_auto() {
        let mut auto = test_primary_agent_spec("auto");
        auto.prompt = "Custom auto instructions".to_string();

        let active = active_primary_agent_from_specs_for_mode(&[auto], None, true, false, None).expect("auto");

        assert_eq!(active.active().identity.name, "auto");
        assert_eq!(active.active().instructions, "Custom auto instructions");
    }

    #[test]
    fn resumed_session_mode_wins_over_config_default() {
        let cfg = VTCodeConfig {
            default_primary_agent: "builder".to_string(),
            ..VTCodeConfig::default()
        };
        // A resumed "plan" session should restore "plan", not the config default "builder".
        let active = active_primary_agent_from_specs_for_mode(
            &[test_primary_agent_spec("plan"), test_primary_agent_spec("builder")],
            Some(&cfg),
            false,
            false,
            Some("plan".to_string()),
        )
        .expect("resumed primary agent");

        assert_eq!(active.active().identity.name, "plan");
    }

    #[test]
    fn explicit_configured_primary_agent_overrides_resumed_mode() {
        // When the user explicitly configures a primary agent, it wins even on resume.
        let cfg = VTCodeConfig {
            default_primary_agent: "builder".to_string(),
            ..VTCodeConfig::default()
        };
        let active = active_primary_agent_from_specs_for_mode(
            &[test_primary_agent_spec("plan"), test_primary_agent_spec("builder")],
            Some(&cfg),
            false,
            true,
            Some("plan".to_string()),
        )
        .expect("configured primary agent");

        assert_eq!(active.active().identity.name, "builder");
    }

    #[test]
    fn full_auto_honours_explicit_primary_agent_config() {
        let cfg = VTCodeConfig {
            default_primary_agent: "builder".to_string(),
            ..VTCodeConfig::default()
        };
        let specs = [test_primary_agent_spec("auto"), test_primary_agent_spec("builder")];

        let active =
            active_primary_agent_from_specs_for_mode(&specs, Some(&cfg), true, true, None).expect("explicit builder");

        assert_eq!(active.active().identity.name, "builder");
    }

    #[test]
    fn full_auto_honours_explicit_build_primary_agent_config() {
        let cfg = VTCodeConfig {
            default_primary_agent: "build".to_string(),
            ..VTCodeConfig::default()
        };
        let specs = [test_primary_agent_spec("auto")];

        let active =
            active_primary_agent_from_specs_for_mode(&specs, Some(&cfg), true, true, None).expect("explicit build");

        assert_eq!(active.active().identity.name, "build");
        assert_eq!(active.active().identity.source, SubagentSource::Builtin);
    }

    #[test]
    fn full_auto_missing_defaulted_auto_fails_fast() {
        let err =
            active_primary_agent_from_specs_for_mode(&[test_primary_agent_spec("builder")], None, true, false, None)
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
            tool_policy_overrides: BTreeMap::new(),
        }
    }
}
