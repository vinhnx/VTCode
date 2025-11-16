use crate::agent::runloop::unified::state::SessionStats;
use std::sync::Arc;
use tokio::sync::RwLock;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::pruning_decisions::PruningDecisionLedger;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::ToolResultCache;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::ui::tui::InlineSession;
use vtcode_core::utils::ansi::AnsiRenderer;
// use crate::agent::runloop::unified::mcp_tool_manager::McpToolManager; // unused
use crate::agent::runloop::mcp_events::McpPanelState;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::tools::ApprovalRecorder;

pub(crate) struct RunLoopContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub tool_registry: &'a mut ToolRegistry,
    pub tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
    pub tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
    pub decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
    pub pruning_ledger: &'a Arc<RwLock<PruningDecisionLedger>>,
    pub session_stats: &'a mut SessionStats,
    pub mcp_panel_state: &'a mut McpPanelState,
    pub approval_recorder: &'a ApprovalRecorder,
    pub session: &'a mut InlineSession,
    pub traj: &'a TrajectoryLogger,
}

// Lightweight adapter that provides a smaller context for per-turn operations.
pub(crate) struct TurnExecutionContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session_stats: &'a mut SessionStats,
    pub mcp_panel_state: &'a mut McpPanelState,
    pub approval_recorder: &'a ApprovalRecorder,
}
