//! Plan-approval agent handoff for the session interaction loop.
//!
//! Switches the primary agent after plan approval (or plan entry): selects a
//! write-capable execution agent, restores the build runtime, refreshes the
//! TUI handle, and persists the selection. Selection failure checkpoints the
//! finished turn tail and restarts the iteration.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use hashbrown::HashMap;
use tokio::sync::RwLock;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runtime::AgentRuntime;
use vtcode_core::core::agent::snapshots::SnapshotTurnDiagnostics;
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
use super::super::super::turn_tail::{TurnPersistenceTail, complete_turn_persistence_tail};
use super::super::session_bootstrap::persist_primary_agent;
use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::planning_workflow::PlanExecutionTarget;
use crate::agent::runloop::unified::run_loop_context::TurnRunId;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::unified::turn::primary_agent_runtime::{
    PrimaryAgentRuntimeSyncContext, sync_primary_agent_permissions, sync_primary_agent_runtime,
};

/// Mutable plan-handoff state threaded through the agent switch.
pub(super) struct PlanHandoff<'a> {
    pub active_primary_agent: &'a mut ActivePrimaryAgentState,
    pub renderer: &'a mut AnsiRenderer,
    pub tool_registry: &'a mut ToolRegistry,
    pub lifecycle_hooks: &'a mut Option<vtcode_core::hooks::LifecycleHookEngine>,
    pub mcp_catalog_initialized: &'a mut bool,
    pub pending_mcp_refresh: &'a mut bool,
    pub session_archive: &'a mut Option<SessionArchive>,
    pub runtime: &'a mut AgentRuntime,
    pub session_skip_confirmations: &'a mut bool,
    pub vt_cfg: &'a mut Option<VTCodeConfig>,
    pub config: &'a CoreAgentConfig,
    pub turn_run_id: &'a TurnRunId,
    pub tools: &'a Arc<RwLock<Vec<ToolDefinition>>>,
    pub tool_catalog: &'a Arc<ToolCatalogState>,
    pub async_mcp_manager: &'a Option<Arc<AsyncMcpManager>>,
    pub provider_client: &'a dyn LLMProvider,
    pub handle: &'a InlineHandle,
    pub session_stats: &'a SessionStats,
    pub loaded_skills: &'a Arc<RwLock<HashMap<String, Skill>>>,
    pub turn_diagnostics: &'a SnapshotTurnDiagnostics,
    pub harness_emitter: &'a Option<HarnessEventEmitter>,
    pub harness_session_id: &'a str,
    pub plan_execution_target: Option<PlanExecutionTarget>,
    pub switch_primary_agent: Option<String>,
    pub history_snapshot_bytes: usize,
    pub max_tool_wall_clock_secs: u64,
    pub turn_elapsed: Duration,
    pub next_checkpoint_turn: usize,
    pub plan_skip_confirmations: bool,
}

/// Run the plan-approval agent handoff. Returns whether to restart the loop.
pub(super) async fn run_plan_handoff(gate: PlanHandoff<'_>) -> Result<bool> {
    let PlanHandoff {
        active_primary_agent,
        renderer,
        tool_registry,
        lifecycle_hooks,
        mcp_catalog_initialized,
        pending_mcp_refresh,
        session_archive,
        runtime,
        session_skip_confirmations,
        vt_cfg,
        config,
        turn_run_id,
        tools,
        tool_catalog,
        async_mcp_manager,
        provider_client,
        handle,
        session_stats,
        loaded_skills,
        turn_diagnostics,
        harness_emitter,
        harness_session_id,
        plan_execution_target,
        switch_primary_agent,
        history_snapshot_bytes,
        max_tool_wall_clock_secs,
        turn_elapsed,
        next_checkpoint_turn,
        plan_skip_confirmations,
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
    let requested_agent_for_handoff = plan_execution_target
        .map(|target| target.agent_name().to_owned())
        .or(switch_primary_agent);
    if let Some(requested_agent) = requested_agent_for_handoff {
        let plan_entry = crate::agent::runloop::unified::turn::turn_loop::is_plan_entry_handoff(&requested_agent)
            && plan_execution_target.is_none();
        if plan_entry {
            use crate::agent::runloop::unified::planning_workflow_state::PLAN_PRIMARY_AGENT_NAME;
            use crate::agent::runloop::unified::turn::primary_agent_runtime::{
                builtin_primary_agent_specs, load_primary_agent_specs,
            };
            let specs = match load_primary_agent_specs(tool_registry, &config.workspace).await {
                Ok(specs) if !specs.is_empty() => specs,
                _ => builtin_primary_agent_specs(),
            };
            match active_primary_agent.select_from_specs(&specs, PLAN_PRIMARY_AGENT_NAME) {
                Ok(active) => {
                    let agent_display = active.display_name.clone();
                    let color = active.color.clone().filter(|c| !c.trim().is_empty());
                    apply_primary_agent_tool_policy_overrides(tool_registry, active_primary_agent.active()).await;
                    sync_primary_agent_permissions(&mut *vt_cfg, active_primary_agent.active());
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
                    if let Err(err) = sync_primary_agent_runtime(&mut runtime_sync).await {
                        tracing::error!(
                            target: "vtcode.planning_workflow",
                            switch_path = "plan_entry",
                            requested_agent = %requested_agent,
                            resolved_agent = %agent_display,
                            error = %err,
                            "Plan-entry runtime sync failed; header will still show Plan and planning stays active"
                        );
                        harness_try!(renderer.line(
                            MessageStyle::Warning,
                            &format!("Plan mode is active, but runtime sync failed: {err}"),
                        ));
                    }
                    handle.set_primary_agent(Some(agent_display), color);
                    tracing::info!(
                        target: "vtcode.planning_workflow",
                        switch_path = "plan_entry",
                        "Switched primary agent to plan after confirmed planning entry"
                    );
                    persist_primary_agent(session_archive, active_primary_agent);
                }
                Err(err) => {
                    tracing::warn!(
                        target: "vtcode.planning_workflow",
                        switch_path = "plan_entry",
                        requested_agent = %requested_agent,
                        error = %err,
                        "Could not select plan primary agent after planning entry; planning stays active"
                    );
                    harness_try!(renderer.line(
                        MessageStyle::Warning,
                        &format!("Could not select plan primary agent after planning entry: {err}"),
                    ));
                }
            }
        } else {
            let configured_default = vt_cfg
                .as_ref()
                .map(|cfg| cfg.default_primary_agent.as_str())
                .filter(|name| !name.trim().is_empty());
            // Selection failure must stay recoverable: the plan is
            // already approved, so aborting the session here would
            // leave a half-switched state with no retry path. Run
            // the turn-persistence tail before `continue` so a
            // finished turn is checkpointed here, not deferred.
            let execution_agent = match select_approved_plan_execution_agent(
                active_primary_agent,
                tool_registry,
                &config.workspace,
                Some(requested_agent.as_str()),
                configured_default,
            )
            .await
            {
                Ok(agent) => agent,
                Err(err) => {
                    let msg = report_plan_approval_selection_failure(&requested_agent, &err);
                    harness_try!(renderer.line(MessageStyle::Error, &msg));
                    // This site runs after the turn's work. Persist
                    // the real turn tail before abandoning the
                    // iteration so the just-finished turn is
                    // checkpointed (not deferred to the next turn).
                    persist_primary_agent(session_archive, active_primary_agent);
                    complete_turn_persistence_tail(TurnPersistenceTail {
                        outcome: "aborted",
                        history_snapshot_bytes,
                        timeout_secs: max_tool_wall_clock_secs,
                        elapsed_ms: turn_elapsed.as_millis(),
                        blocked_turn: false,
                        turn_diagnostics: Some(turn_diagnostics.clone()), // error path only; root still owns it below
                        runtime,
                        session_archive,
                        next_checkpoint_turn,
                        session_stats,
                        loaded_skills,
                        workspace: config.workspace.as_path(),
                        session_id: harness_session_id,
                        vt_cfg: vt_cfg.as_ref(),
                    })
                    .await;
                    return Ok(true);
                }
            };
            if execution_agent != requested_agent {
                tracing::warn!(
                    target: "vtcode.planning_workflow",
                    switch_path = "plan_approval",
                    requested_agent = %requested_agent,
                    resolved_agent = %execution_agent,
                    "Approved plan requested a non-executable primary agent; using a write-capable agent"
                );
                harness_try!(renderer.line(
                    MessageStyle::Info,
                    &format!("Approved plan requires a write-capable agent; switching to {}.", execution_agent),
                ));
            }
            // The approval choice, rather than the destination agent
            // name, owns confirmation policy. This keeps a manual
            // Execute/Switch Build handoff prompting even if an
            // earlier agent or fallback happens to be named `auto`.
            *session_skip_confirmations = plan_skip_confirmations;
            handle.set_skip_confirmations(*session_skip_confirmations);
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
            if let Err(err) = sync_primary_agent_runtime(&mut runtime_sync).await {
                tracing::error!(
                    target: "vtcode.planning_workflow",
                    switch_path = "plan_approval",
                    agent = %execution_agent,
                    error = %err,
                    "Approved-plan runtime sync failed; plan remains approved and can be retried"
                );
                harness_try!(
                    renderer
                        .line(MessageStyle::Error, &format!("Approved plan is ready, but mode switch failed: {err}"),)
                );
            }
            let agent_display = active_primary_agent.active().display_name.clone();
            let color = active_primary_agent.active().color.clone().filter(|c| !c.trim().is_empty());
            handle.set_primary_agent(Some(agent_display), color);
            tracing::info!(
                target: "vtcode.planning_workflow",
                switch_path = "plan_approval",
                agent = %execution_agent,
                "Switched primary agent after plan approval"
            );
            persist_primary_agent(session_archive, active_primary_agent);
        }
    }
    Ok(false)
}
