use std::future::Future;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{Notify, RwLock};

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::hooks::{LifecycleHookEngine, SessionEndReason};
use vtcode_core::llm::provider as uni;
use vtcode_core::primary_agent::ActivePrimaryAgentState;
use vtcode_core::tools::ToolRegistry;
use vtcode_ui::tui::app::{InlineHandle, InlineHeaderContext, InlineSession};

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::model_picker::ModelPickerState;
pub(crate) use crate::agent::runloop::slash_commands::SlashCommandOutcome;
use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::session_setup::IdeContextBridge;
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::agent::runloop::welcome::SessionBootstrap;
use vtcode_core::utils::ansi::AnsiRenderer;

mod handlers;
mod outcome_router;

pub(crate) enum SlashCommandControl {
    Continue,
    SubmitPrompt(String),
    ReplaceInput(String),
    BreakWithReason(SessionEndReason),
}

pub(crate) struct SlashCommandContext<'a> {
    pub(crate) thread_id: &'a str,
    pub(crate) active_thread_label: &'a str,
    pub(crate) thread_handle: &'a vtcode_core::core::threads::ThreadRuntimeHandle,
    pub(crate) renderer: &'a mut AnsiRenderer,
    pub(crate) handle: &'a InlineHandle,
    pub(crate) session: &'a mut InlineSession,
    pub(crate) header_context: &'a mut InlineHeaderContext,
    pub(crate) ide_context_bridge: &'a mut Option<IdeContextBridge>,
    pub(crate) config: &'a mut CoreAgentConfig,
    pub(crate) vt_cfg: &'a mut Option<VTCodeConfig>,
    pub(crate) provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub(crate) session_bootstrap: &'a SessionBootstrap,
    pub(crate) model_picker_state: &'a mut Option<ModelPickerState>,
    pub(crate) palette_state: &'a mut Option<ActivePalette>,
    pub(crate) tool_registry: &'a mut ToolRegistry,
    pub(crate) conversation_history: &'a mut Vec<uni::Message>,
    pub(crate) decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
    pub(crate) context_manager: &'a mut ContextManager,
    pub(crate) active_primary_agent: &'a mut ActivePrimaryAgentState,
    pub(crate) session_stats: &'a mut SessionStats,
    pub(crate) plan_session: &'a mut PlanningWorkflowSessionState,
    pub(crate) input_status_state: &'a mut InputStatusState,
    pub(crate) tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub(crate) tool_catalog: &'a Arc<ToolCatalogState>,
    pub(crate) async_mcp_manager: Option<&'a Arc<AsyncMcpManager>>,
    pub(crate) mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub(crate) linked_directories: &'a mut Vec<LinkedDirectory>,
    pub(crate) ctrl_c_state: &'a Arc<CtrlCState>,
    pub(crate) ctrl_c_notify: &'a Arc<Notify>,
    pub(crate) full_auto: bool,
    pub(crate) loaded_skills:
        &'a Arc<RwLock<hashbrown::HashMap<String, vtcode_core::skills::types::Skill>>>,
    pub(crate) checkpoint_manager: Option<&'a vtcode_core::core::agent::snapshots::SnapshotManager>,
    pub(crate) lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    pub(crate) harness_emitter: Option<&'a HarnessEventEmitter>,
}

impl<'a> SlashCommandContext<'a> {
    pub(crate) fn reborrow(&mut self) -> SlashCommandContext<'_> {
        SlashCommandContext {
            thread_id: self.thread_id,
            active_thread_label: self.active_thread_label,
            thread_handle: self.thread_handle,
            renderer: self.renderer,
            handle: self.handle,
            session: self.session,
            header_context: self.header_context,
            ide_context_bridge: self.ide_context_bridge,
            config: self.config,
            vt_cfg: self.vt_cfg,
            provider_client: self.provider_client,
            session_bootstrap: self.session_bootstrap,
            model_picker_state: self.model_picker_state,
            palette_state: self.palette_state,
            tool_registry: self.tool_registry,
            conversation_history: self.conversation_history,
            decision_ledger: self.decision_ledger,
            context_manager: self.context_manager,
            active_primary_agent: &mut *self.active_primary_agent,
            session_stats: self.session_stats,
            plan_session: self.plan_session,
            input_status_state: self.input_status_state,
            tools: self.tools,
            tool_catalog: self.tool_catalog,
            async_mcp_manager: self.async_mcp_manager,
            mcp_panel_state: self.mcp_panel_state,
            linked_directories: self.linked_directories,
            ctrl_c_state: self.ctrl_c_state,
            ctrl_c_notify: self.ctrl_c_notify,
            full_auto: self.full_auto,
            loaded_skills: self.loaded_skills,
            checkpoint_manager: self.checkpoint_manager,
            lifecycle_hooks: self.lifecycle_hooks,
            harness_emitter: self.harness_emitter,
        }
    }
}

pub(crate) fn run_with_event_loop_suspended<'a, T: 'a, F>(
    handle: &'a InlineHandle,
    suspend_tui: bool,
    launch: F,
) -> impl Future<Output = Result<T>> + 'a
where
    F: FnOnce() -> Result<T> + 'a,
{
    handlers::run_with_event_loop_suspended(handle, suspend_tui, launch)
}

pub(crate) async fn handle_outcome(
    outcome: SlashCommandOutcome,
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    outcome_router::route_outcome(outcome, ctx).await
}
