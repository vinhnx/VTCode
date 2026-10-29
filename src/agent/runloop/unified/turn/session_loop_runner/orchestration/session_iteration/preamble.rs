//! Approved-plan loop-head preamble for the session interaction loop.
//!
//! Re-establishes the plan-execution boundary when the approval turn ended
//! before the handoff state was fully applied: completes the planning
//! handoff, selects a write-capable execution agent, reloads workspace
//! configuration, and syncs the primary-agent runtime.

use std::sync::Arc;

use anyhow::Result;
use hashbrown::HashMap;
use tokio::sync::RwLock;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runtime::AgentRuntime;
use vtcode_core::llm::provider::{LLMProvider, ToolDefinition};
use vtcode_core::primary_agent::ActivePrimaryAgentState;
use vtcode_core::skills::types::Skill;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionArchive;
use vtcode_ui::tui::app::InlineHandle;

use super::super::super::handoff::{
    apply_primary_agent_tool_policy_overrides, report_plan_approval_selection_failure,
    select_approved_plan_execution_agent,
};
use super::super::super::support::force_reload_workspace_config_for_execution;
use super::super::session_bootstrap::record_plan_selection_failure_tail;
use super::skip_confirmations::effective_session_skip_confirmations;
use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::run_loop_context::TurnRunId;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::unified::turn::primary_agent_runtime::{
    PrimaryAgentRuntimeSyncContext, sync_primary_agent_permissions, sync_primary_agent_runtime,
};

/// Mutable loop-head preamble state threaded through boundary repair.
pub(super) struct Preamble<'a> {
    pub pending_approved_plan_execution_input: &'a mut bool,
    pub renderer: &'a mut AnsiRenderer,
    pub plan_session: &'a mut PlanningWorkflowSessionState,
    pub active_primary_agent: &'a mut ActivePrimaryAgentState,
    pub tool_registry: &'a mut ToolRegistry,
    pub runtime: &'a mut AgentRuntime,
    pub session_archive: &'a mut Option<SessionArchive>,
    pub lifecycle_hooks: &'a mut Option<vtcode_core::hooks::LifecycleHookEngine>,
    pub mcp_catalog_initialized: &'a mut bool,
    pub pending_mcp_refresh: &'a mut bool,
    pub vt_cfg: &'a mut Option<VTCodeConfig>,
    pub config: &'a mut CoreAgentConfig,
    pub handle: &'a InlineHandle,
    pub turn_run_id: &'a TurnRunId,
    pub tools: &'a Arc<RwLock<Vec<ToolDefinition>>>,
    pub tool_catalog: &'a Arc<ToolCatalogState>,
    pub async_mcp_manager: &'a Option<Arc<AsyncMcpManager>>,
    pub provider_client: &'a dyn LLMProvider,
    pub session_stats: &'a SessionStats,
    pub loaded_skills: &'a Arc<RwLock<HashMap<String, Skill>>>,
    pub session_skip_confirmations: &'a bool,
    pub harness_emitter: &'a Option<HarnessEventEmitter>,
    pub executing_approved_plan: bool,
    pub next_checkpoint_turn: usize,
    pub max_tool_wall_clock_secs: u64,
}

/// Repair the plan-execution boundary. Returns whether to restart the loop.
pub(super) async fn run_approved_plan_preamble(gate: Preamble<'_>) -> Result<bool> {
    let Preamble {
        pending_approved_plan_execution_input,
        renderer,
        plan_session,
        active_primary_agent,
        tool_registry,
        runtime,
        session_archive,
        lifecycle_hooks,
        mcp_catalog_initialized,
        pending_mcp_refresh,
        vt_cfg,
        config,
        handle,
        turn_run_id,
        tools,
        tool_catalog,
        async_mcp_manager,
        provider_client,
        session_stats,
        loaded_skills,
        session_skip_confirmations,
        harness_emitter,
        executing_approved_plan,
        next_checkpoint_turn,
        max_tool_wall_clock_secs,
    } = gate;
    // Same early-exit contract as the iteration root: finalize canonical
    // persistence before leaving on errors.
    macro_rules! harness_try {
        ($expression:expr) => {{
            match $expression {
                Ok(value) => value,
                Err(error) => {
                    let error: anyhow::Error = error.into();
                    if let Some(emitter) = harness_emitter.as_ref() {
                        emitter.finish_after_unexpected_exit().await;
                    }
                    return Err(error);
                }
            }
        }};
    }
    // The approval turn may have ended before the handoff state
    // was fully applied (for example after recovery or a stale
    // primary-agent catalog). Re-establish the execution boundary
    // before the queued implementation turn can build its request.
    // This prevents a read-only `plan` agent from surviving the
    // approval and denying the first shell/edit call.
    if executing_approved_plan {
        if tool_registry.is_planning_active() {
            let plan =
                match crate::agent::runloop::unified::planning_workflow::load_plan_text_for_approval(tool_registry)
                    .await
                {
                    Ok(plan) => plan,
                    Err(error) => {
                        *pending_approved_plan_execution_input = false;
                        harness_try!(
                            renderer
                                .line(MessageStyle::Error, &format!("Approved-plan execution is blocked: {error}"),)
                        );
                        return Ok(true);
                    }
                };
            if let Err(error) = crate::agent::runloop::unified::planning_workflow::complete_approved_plan_handoff(
                tool_registry,
                plan_session,
                handle,
                plan,
                crate::agent::runloop::unified::planning_workflow::PlanExecutionTarget::build(
                    crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Current,
                    effective_session_skip_confirmations(*session_skip_confirmations, tool_registry),
                ),
            )
            .await
            {
                harness_try!(
                    renderer.line(MessageStyle::Error, &format!("Approved-plan execution is blocked: {error}"))
                );
                *pending_approved_plan_execution_input = false;
                return Ok(true);
            }
        }
        let configured_default = vt_cfg
            .as_ref()
            .map(|cfg| cfg.default_primary_agent.as_str())
            .filter(|name| !name.trim().is_empty());
        let current_agent = active_primary_agent.active().name().to_string();
        // Selection failure must stay recoverable so an approved
        // plan is not lost to a hard session abort.
        let execution_agent = match select_approved_plan_execution_agent(
            active_primary_agent,
            tool_registry,
            &config.workspace,
            Some(current_agent.as_str()),
            configured_default,
        )
        .await
        {
            Ok(agent) => agent,
            Err(err) => {
                let msg = report_plan_approval_selection_failure(&current_agent, &err);
                harness_try!(renderer.line(MessageStyle::Error, &msg));
                *pending_approved_plan_execution_input = false;
                let session_id = tool_registry.harness_context_snapshot().session_id;
                record_plan_selection_failure_tail(
                    runtime,
                    session_archive,
                    session_stats,
                    loaded_skills,
                    next_checkpoint_turn,
                    config.workspace.as_path(),
                    &session_id,
                    vt_cfg.as_ref(),
                    max_tool_wall_clock_secs,
                )
                .await;
                return Ok(true);
            }
        };
        if current_agent != execution_agent {
            harness_try!(renderer.line(
                MessageStyle::Info,
                &format!("Approved plan requires a write-capable agent; switching to {execution_agent}."),
            ));
        }
        if let Err(err) = force_reload_workspace_config_for_execution(
            config.workspace.as_path(),
            config,
            &mut *vt_cfg,
            tool_registry,
            async_mcp_manager.as_deref(),
        )
        .await
        {
            tracing::warn!(error = %err, "Failed to reload workspace configuration before approved-plan execution");
            harness_try!(renderer.line(MessageStyle::Error, &format!("Failed to reload configuration: {err}")));
        }
        sync_primary_agent_permissions(&mut *vt_cfg, active_primary_agent.active());
        apply_primary_agent_tool_policy_overrides(tool_registry, active_primary_agent.active()).await;
        let mut runtime_sync = PrimaryAgentRuntimeSyncContext {
            config,
            vt_cfg: vt_cfg.as_ref(),
            thread_id: &turn_run_id.0,
            active_primary_agent: active_primary_agent.active(),
            lifecycle_hooks: &mut *lifecycle_hooks,
            async_mcp_manager: async_mcp_manager.as_ref(),
            tool_registry: &mut *tool_registry,
            tools,
            tool_catalog,
            mcp_catalog_initialized: &mut *mcp_catalog_initialized,
            pending_mcp_refresh: &mut *pending_mcp_refresh,
            provider_client,
        };
        harness_try!(sync_primary_agent_runtime(&mut runtime_sync).await);
        let display = active_primary_agent.active().display_name.clone();
        let color = active_primary_agent.active().color.clone().filter(|c| !c.trim().is_empty());
        handle.set_primary_agent(Some(display), color);
    }
    Ok(false)
}
