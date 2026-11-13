//! Approved-plan execution handoff for the session interaction loop.
//!
//! Drives the `PlanApproved` interaction outcome: loads the approved plan,
//! completes the planning handoff, selects a write-capable execution agent,
//! restores the build runtime, and seeds the implementation turn input.

use hashbrown::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;
use vtcode_commons::ui_protocol::ActivityState;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runtime::AgentRuntime;
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_core::hooks::{LifecycleHookEngine, SessionEndReason};
use vtcode_core::llm::provider::{LLMProvider, ToolDefinition};
use vtcode_core::primary_agent::ActivePrimaryAgentState;
use vtcode_core::skills::types::Skill;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::session_archive::SessionArchive;
use vtcode_ui::tui::app::InlineHandle;

use super::super::super::handoff::{
    append_approved_plan_execution_input, apply_primary_agent_tool_policy_overrides,
    build_approved_plan_execution_prompt, report_plan_approval_selection_failure, select_approved_plan_execution_agent,
};
use super::super::super::plan_seed::load_active_plan_seed;
use super::super::super::support::force_reload_workspace_config_for_execution;
use super::super::session_bootstrap::record_plan_selection_failure_tail;
use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::planning_workflow::PlanExecutionTarget;
use crate::agent::runloop::unified::planning_workflow_state::{
    PlanningWorkflowSessionState, transition_to_planning_workflow,
};
use crate::agent::runloop::unified::run_loop_context::TurnRunId;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::unified::turn::primary_agent_runtime::{
    PrimaryAgentRuntimeSyncContext, sync_primary_agent_permissions, sync_primary_agent_runtime,
};

/// Mutable plan-approval state threaded through the execution handoff.
pub(super) struct PlanApprovalGate<'a> {
    pub executing_approved_plan: &'a mut bool,
    pub session_end_reason: &'a mut SessionEndReason,
    pub session_skip_confirmations: &'a mut bool,
    pub runtime: &'a mut AgentRuntime,
    pub tool_registry: &'a mut ToolRegistry,
    pub active_primary_agent: &'a mut ActivePrimaryAgentState,
    pub plan_session: &'a mut PlanningWorkflowSessionState,
    pub session_stats: &'a mut SessionStats,
    pub session_archive: &'a mut Option<SessionArchive>,
    pub context_manager: &'a mut ContextManager,
    pub lifecycle_hooks: &'a mut Option<LifecycleHookEngine>,
    pub renderer: &'a mut AnsiRenderer,
    pub vt_cfg: &'a mut Option<VTCodeConfig>,
    pub config: &'a mut CoreAgentConfig,
    pub mcp_catalog_initialized: &'a mut bool,
    pub pending_mcp_refresh: &'a mut bool,
    pub handle: &'a InlineHandle,
    pub turn_id: &'a str,
    pub turn_run_id: &'a TurnRunId,
    pub provider_client: &'a dyn LLMProvider,
    pub tools: &'a Arc<RwLock<Vec<ToolDefinition>>>,
    pub tool_catalog: &'a Arc<ToolCatalogState>,
    pub async_mcp_manager: &'a Option<Arc<AsyncMcpManager>>,
    pub loaded_skills: &'a Arc<RwLock<HashMap<String, Skill>>>,
    pub safety_validator: &'a Arc<ToolCallSafetyValidator>,
    pub harness_emitter: &'a Option<HarnessEventEmitter>,
    pub next_checkpoint_turn: usize,
    pub max_tool_wall_clock_secs: u64,
    pub target: PlanExecutionTarget,
}

/// How the interaction loop proceeds after the plan-approval handoff.
pub(super) enum PlanApprovalFlow {
    ContinueLoop,
    BreakLoop,
    Proceed {
        input: String,
        prompt_message_index: Option<usize>,
    },
}

/// Drive the `PlanApproved` handoff and seed the implementation turn input.
pub(super) async fn handle_plan_approval(gate: PlanApprovalGate<'_>) -> Result<PlanApprovalFlow> {
    // Same early-exit contract as the iteration root: finalize canonical
    // persistence before leaving on errors.
    let PlanApprovalGate {
        executing_approved_plan,
        session_end_reason,
        session_skip_confirmations,
        runtime,
        tool_registry,
        active_primary_agent,
        plan_session,
        session_stats,
        session_archive,
        context_manager,
        lifecycle_hooks,
        renderer,
        vt_cfg,
        config,
        mcp_catalog_initialized,
        pending_mcp_refresh,
        handle,
        turn_id,
        turn_run_id,
        provider_client,
        tools,
        tool_catalog,
        async_mcp_manager,
        loaded_skills,
        safety_validator,
        harness_emitter,
        next_checkpoint_turn,
        max_tool_wall_clock_secs,
        target,
    } = gate;

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
    // This approval path starts the implementation turn in
    // the same outer iteration, so mark it before the
    // HarnessTurnState is constructed below. The queued
    // approval path sets the equivalent flag on the next
    // iteration.
    *executing_approved_plan = true;
    let plan = match crate::agent::runloop::unified::planning_workflow::load_plan_text_for_approval(tool_registry).await
    {
        Ok(plan) => plan,
        Err(error) => {
            handle.set_activity_state(ActivityState::Idle);
            harness_try!(renderer.line(
                MessageStyle::Error,
                &format!("Approved-plan execution is blocked: {error}. The plan was retained; please retry approval."),
            ));
            transition_to_planning_workflow(
                tool_registry,
                session_stats,
                plan_session,
                handle,
                PlanningEntrySource::AgentSelection,
                Some(active_primary_agent.active().name().to_string()),
                vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
                false,
                false,
            )
            .await;
            return Ok(PlanApprovalFlow::ContinueLoop);
        }
    };
    if let Err(error) = crate::agent::runloop::unified::planning_workflow::complete_approved_plan_handoff(
        tool_registry,
        plan_session,
        handle,
        plan,
        target,
    )
    .await
    {
        handle.set_activity_state(ActivityState::Idle);
        harness_try!(renderer.line(
            MessageStyle::Error,
            &format!("Approved-plan execution is blocked: {error}. The plan was retained; please retry approval."),
        ));
        transition_to_planning_workflow(
            tool_registry,
            session_stats,
            plan_session,
            handle,
            PlanningEntrySource::AgentSelection,
            Some(active_primary_agent.active().name().to_string()),
            vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
            false,
            false,
        )
        .await;
        return Ok(PlanApprovalFlow::ContinueLoop);
    }
    crate::agent::runloop::unified::planning_workflow::resolve_plan_approval(
        plan_session,
        harness_emitter.as_ref(),
        &turn_run_id.0,
        turn_id,
        crate::agent::runloop::unified::planning_workflow::plan_approval_decision_for_target(target),
        false,
    );
    let execution_context = target.execution_context;
    let skip_confirmations = target.skip_confirmations;
    let fresh_context =
        matches!(execution_context, crate::agent::runloop::unified::planning_workflow::PlanExecutionContext::Fresh);
    if fresh_context {
        handle.set_activity_state(ActivityState::PreparingFreshExecutionThread);
    }
    let plan_seed = load_active_plan_seed(tool_registry).await;
    if fresh_context && plan_seed.is_none() {
        handle.set_activity_state(ActivityState::Idle);
        harness_try!(renderer.line(
                    MessageStyle::Error,
                    "Fresh execution could not start because the approved plan was not found. The plan was retained; please retry approval.",
                ));
        transition_to_planning_workflow(
            tool_registry,
            session_stats,
            plan_session,
            handle,
            PlanningEntrySource::AgentSelection,
            Some(active_primary_agent.active().name().to_string()),
            vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
            false,
            false,
        )
        .await;
        return Ok(PlanApprovalFlow::ContinueLoop);
    }
    let plan_seed = if fresh_context {
        load_active_plan_seed(tool_registry).await.or(plan_seed)
    } else {
        plan_seed
    };
    let current_context_budget = vtcode_core::compaction::effective_context_budget(
        vt_cfg.as_ref(),
        provider_client,
        &crate::agent::runloop::unified::turn::turn_processing::resolve_effective_request_model(
            &config.model,
            active_primary_agent.active(),
        ),
    );
    let previous_context_usage_percent = context_manager.context_usage_percent(current_context_budget);
    let configured_default = vt_cfg
        .as_ref()
        .map(|cfg| cfg.default_primary_agent.as_str())
        .filter(|name| !name.trim().is_empty());
    let requested_agent = target.agent_name();
    let resolved_execution_agent = match select_approved_plan_execution_agent(
        active_primary_agent,
        tool_registry,
        &config.workspace,
        Some(requested_agent),
        configured_default,
    )
    .await
    {
        Ok(agent) => agent,
        Err(err) if fresh_context => {
            handle.set_activity_state(ActivityState::Idle);
            harness_try!(
                renderer.line(MessageStyle::Error, &format!("Fresh execution could not select a build agent: {err}"),)
            );
            transition_to_planning_workflow(
                tool_registry,
                session_stats,
                plan_session,
                handle,
                PlanningEntrySource::AgentSelection,
                Some(active_primary_agent.active().name().to_string()),
                vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
                false,
                false,
            )
            .await;
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
            return Ok(PlanApprovalFlow::ContinueLoop);
        }
        Err(err) => {
            let msg = report_plan_approval_selection_failure(requested_agent, &err);
            harness_try!(renderer.line(MessageStyle::Error, &msg));
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
            return Ok(PlanApprovalFlow::ContinueLoop);
        }
    };
    if requested_agent != resolved_execution_agent.as_str() {
        tracing::warn!(
            requested_agent = ?requested_agent,
            resolved_agent = ?resolved_execution_agent,
            "Approved plan requested a non-executable primary agent; using a write-capable agent"
        );
        harness_try!(renderer.line(
            MessageStyle::Info,
            &format!("Approved plan requires a write-capable agent; switching to {}.", resolved_execution_agent),
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
        tracing::warn!("Failed to reload workspace configuration at plan approval: {}", err);
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
    if let Err(err) = sync_primary_agent_runtime(&mut runtime_sync).await {
        if fresh_context {
            handle.set_activity_state(ActivityState::Idle);
            harness_try!(
                renderer
                    .line(MessageStyle::Error, &format!("Fresh execution could not restore the build runtime: {err}"),)
            );
            transition_to_planning_workflow(
                tool_registry,
                session_stats,
                plan_session,
                handle,
                PlanningEntrySource::AgentSelection,
                Some(active_primary_agent.active().name().to_string()),
                vt_cfg.as_ref().map(|cfg| cfg.default_primary_agent.clone()),
                false,
                false,
            )
            .await;
            return Ok(PlanApprovalFlow::ContinueLoop);
        }
        tracing::error!(error = %err, "approved-plan runtime restoration failed");
        *session_end_reason = SessionEndReason::Error;
        return Ok(PlanApprovalFlow::BreakLoop);
    }
    if fresh_context {
        handle.set_activity_state(ActivityState::RestoringApprovedPlan);
        runtime.clear_pending_follow_up_inputs();
        runtime.state.clear_conversation_history();
        context_manager.reset_for_fresh_execution();
        session_stats.reset_for_fresh_execution();
        safety_validator.reset_for_fresh_execution();
        crate::agent::runloop::unified::planning_workflow::emit_context_reset(
            harness_emitter.as_ref(),
            turn_run_id.0.clone(),
            turn_id.to_owned(),
            previous_context_usage_percent,
        );
    }
    let execution_display = active_primary_agent.active().display_name.clone();
    let execution_color = active_primary_agent.active().color.clone().filter(|c| !c.trim().is_empty());
    handle.set_primary_agent(Some(execution_display), execution_color);
    *session_skip_confirmations = skip_confirmations;
    handle.set_skip_confirmations(skip_confirmations);
    if fresh_context {
        handle.set_activity_state(ActivityState::StartingBuild);
    }
    harness_try!(renderer.line(MessageStyle::Info, "Executing approved plan..."));

    let execution_directive = build_approved_plan_execution_prompt(execution_context, plan_seed.as_deref());
    runtime
        .state
        .messages_mut()
        .push(vtcode_core::llm::provider::Message::system(execution_directive));
    handle.set_activity_state(ActivityState::Building);
    let (input, prompt_message_index) = append_approved_plan_execution_input(runtime);
    Ok(PlanApprovalFlow::Proceed {
        input,
        prompt_message_index: Some(prompt_message_index),
    })
}
