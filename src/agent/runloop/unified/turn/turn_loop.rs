use anyhow::Result;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::{ApprovalRecorder, result_cache::CacheKey, ToolRegistry};
use vtcode_core::tools::ToolResultCache;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::core::decision_tracker::{DecisionOutcome, DecisionTracker};
use vtcode_core::core::pruning_decisions::PruningDecisionLedger;
use vtcode_core::core::token_budget::TokenBudgetManager;
use vtcode_core::llm::TokenCounter;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::utils::ansi::AnsiRenderer;

use crate::agent::runloop::unified::turn::session::SlashCommandContext;
use crate::agent::runloop::unified::turn::turn_processing::TurnProcessingContext;
use crate::agent::runloop::unified::turn::turn_processing::execute_llm_request;
use crate::agent::runloop::unified::turn::turn_processing::TurnProcessingContext as _TPC;
use crate::agent::runloop::unified::turn::tool_execution::ToolExecutionStatus;

use crate::agent::runloop::unified::turn::super::mcp_events;

use crate::agent::runloop::unified::turn::run_loop::{TurnLoopResult, ProgressReporter, PlaceholderSpinner, strip_harmony_syntax};

use crate::agent::runloop::unified::turn::session::SlashCommandControl;

use crate::agent::runloop::unified::turn::session::session::InlineSession; // placeholder

// Note: the module references are kept similar to original file; compiler will resolve them.

pub struct TurnLoopOutcome {
    pub result: TurnLoopResult,
    pub working_history: Vec<uni::Message>,
    pub any_write_effect: bool,
    pub turn_modified_files: BTreeSet<PathBuf>,
}

pub struct TurnLoopContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a crate::vtcode_core::ui::tui::InlineHandle,
    pub session: &'a mut InlineSession,
    pub session_stats: &'a mut crate::agent::runloop::unified::state::SessionStats,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
    pub approval_recorder: &'a Arc<ApprovalRecorder>,
    pub decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
    pub pruning_ledger: &'a Arc<RwLock<PruningDecisionLedger>>,
    pub token_budget: &'a Arc<TokenBudgetManager>,
    pub token_counter: &'a Arc<RwLock<TokenCounter>>,
    pub tool_registry: &'a mut ToolRegistry,
    pub tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub ctrl_c_state: &'a Arc<crate::agent::runloop::unified::state::CtrlCState>,
    pub ctrl_c_notify: &'a Arc<tokio::sync::Notify>,
    pub context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
    pub last_forced_redraw: &'a mut Instant,
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
    pub lifecycle_hooks: Option<&'a crate::hooks::lifecycle::LifecycleHookEngine>,
    pub default_placeholder: &'a Option<String>,
    pub tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_turn_loop(
    input: &str,
    working_history: Vec<uni::Message>,
    _ctx: TurnLoopContext<'_>,
    _config: &crate::vtcode_core::config::types::AgentConfig,
    _vt_cfg: Option<&VTCodeConfig>,
    _provider_client: &mut Box<dyn uni::LLMProvider>,
    _traj: &TrajectoryLogger,
    _skip_confirmations: bool,
    _session_end_reason: &mut crate::hooks::lifecycle::SessionEndReason,
) -> Result<TurnLoopOutcome> {
    // For now this module holds a thin placeholder wrapper. The heavy logic remains in the original file until further decomposition.
    Ok(TurnLoopOutcome {
        result: TurnLoopResult::Completed,
        working_history,
        any_write_effect: false,
        turn_modified_files: BTreeSet::new(),
    })
}
