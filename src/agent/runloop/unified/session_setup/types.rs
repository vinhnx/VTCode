use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::welcome::SessionBootstrap;
use crate::hooks::lifecycle::LifecycleHookEngine;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use tokio::sync::{Notify, RwLock};
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ApprovalRecorder;
use vtcode_core::tools::{SearchMetrics, ToolRegistry, ToolResultCache};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::session_archive::SessionArchive;
use vtcode_tui::{InlineHandle, InlineSession};

pub(crate) struct SessionState {
    pub session_bootstrap: SessionBootstrap,
    pub provider_client: Box<dyn uni::LLMProvider>,
    pub tool_registry: ToolRegistry,
    pub tools: Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub tool_catalog: Arc<ToolCatalogState>,
    pub conversation_history: Vec<uni::Message>,
    pub decision_ledger: Arc<RwLock<DecisionTracker>>,
    pub trajectory: TrajectoryLogger,
    pub base_system_prompt: String,
    pub full_auto_allowlist: Option<Vec<String>>,
    pub async_mcp_manager: Option<Arc<AsyncMcpManager>>,
    pub mcp_panel_state: mcp_events::McpPanelState,
    pub tool_result_cache: Arc<RwLock<ToolResultCache>>,
    pub tool_permission_cache: Arc<RwLock<ToolPermissionCache>>,
    #[allow(dead_code)]
    pub search_metrics: Arc<RwLock<SearchMetrics>>,
    pub loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    pub approval_recorder: Arc<ApprovalRecorder>,
    pub safety_validator: Arc<RwLock<ToolCallSafetyValidator>>,
    pub circuit_breaker: Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub validation_cache: Arc<vtcode_core::tools::validation_cache::ValidationCache>,
    pub telemetry: Arc<vtcode_core::core::telemetry::TelemetryManager>,
    pub autonomous_executor: Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
    pub error_recovery:
        Arc<StdRwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
}

#[allow(dead_code)]
pub(crate) struct SessionUISetup {
    pub renderer: AnsiRenderer,
    pub session: InlineSession,
    pub handle: InlineHandle,
    pub ctrl_c_state: Arc<CtrlCState>,
    pub ctrl_c_notify: Arc<Notify>,
    pub checkpoint_manager: Option<vtcode_core::core::agent::snapshots::SnapshotManager>,
    pub session_archive: Option<SessionArchive>,
    pub lifecycle_hooks: Option<LifecycleHookEngine>,
    pub session_end_reason: crate::hooks::lifecycle::SessionEndReason,
    pub context_manager: ContextManager,
    pub default_placeholder: Option<String>,
    pub follow_up_placeholder: Option<String>,
    pub next_checkpoint_turn: usize,
    pub ui_redraw_batcher: crate::agent::runloop::unified::turn::utils::UIRedrawBatcher,
}

#[allow(dead_code)]
pub(crate) async fn build_conversation_history_from_resume(
    resume: Option<&ResumeSession>,
) -> Vec<uni::Message> {
    resume
        .map(|session| session.history.clone())
        .unwrap_or_default()
}
