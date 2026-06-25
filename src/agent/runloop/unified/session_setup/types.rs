use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::session_setup::IdeContextBridge;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::welcome::SessionBootstrap;
use hashbrown::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use tokio::sync::{Notify, RwLock};
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::PermissionsConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::hooks::{LifecycleHookEngine, SessionEndReason};
use vtcode_core::llm::provider as uni;
use vtcode_core::primary_agent::ActivePrimaryAgentState;
use vtcode_core::tools::ApprovalRecorder;
use vtcode_core::tools::{ToolRegistry, ToolResultCache};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::session_archive::SessionArchive;
use vtcode_ui::tui::app::{InlineHandle, InlineHeaderContext, InlineSession};

use crate::updater::{StartupUpdateCheck, StartupUpdateNotice};

pub(crate) struct BackgroundTaskGuard {
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl BackgroundTaskGuard {
    pub(crate) fn new(handle: tokio::task::JoinHandle<()>) -> Self {
        Self {
            handle: Some(handle),
        }
    }
}

impl Drop for BackgroundTaskGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

pub(crate) struct ToolExecutionContext {
    pub tool_result_cache: Arc<RwLock<ToolResultCache>>,
    pub tool_permission_cache: Arc<RwLock<ToolPermissionCache>>,
    pub permissions_state: Arc<RwLock<PermissionsConfig>>,
    pub approval_recorder: Arc<ApprovalRecorder>,
    pub safety_validator: Arc<ToolCallSafetyValidator>,
    pub circuit_breaker: Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub validation_cache: Arc<vtcode_core::tools::validation_cache::ValidationCache>,
    pub autonomous_executor: Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
}

pub(crate) struct SessionMetadataContext {
    pub decision_ledger: Arc<RwLock<DecisionTracker>>,
    pub trajectory: TrajectoryLogger,
    pub telemetry: Arc<vtcode_core::core::telemetry::TelemetryManager>,
    pub error_recovery: Arc<RwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
}

pub(crate) struct SessionState {
    pub session_bootstrap: SessionBootstrap,
    pub startup_update_check: StartupUpdateCheck,
    pub provider_client: Box<dyn uni::LLMProvider>,
    pub tool_registry: ToolRegistry,
    pub tools: Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub tool_catalog: Arc<ToolCatalogState>,
    pub conversation_history: Vec<uni::Message>,
    pub execution: ToolExecutionContext,
    pub metadata: SessionMetadataContext,
    pub base_system_prompt: String,
    pub full_auto_allowlist: Option<Vec<String>>,
    pub async_mcp_manager: Option<Arc<AsyncMcpManager>>,
    pub mcp_panel_state: mcp_events::McpPanelState,
    pub loaded_skills: Arc<RwLock<HashMap<String, vtcode_core::skills::types::Skill>>>,
    pub active_primary_agent: ActivePrimaryAgentState,
}

pub(crate) struct SessionUISetup {
    pub renderer: AnsiRenderer,
    pub session: InlineSession,
    pub handle: InlineHandle,
    pub header_context: InlineHeaderContext,
    pub ide_context_bridge: Option<IdeContextBridge>,
    pub ctrl_c_state: Arc<CtrlCState>,
    pub ctrl_c_notify: Arc<Notify>,
    pub input_activity_counter: Arc<AtomicU64>,
    pub checkpoint_manager: Option<vtcode_core::core::agent::snapshots::SnapshotManager>,
    pub session_archive: Option<SessionArchive>,
    pub lifecycle_hooks: Option<LifecycleHookEngine>,
    pub session_end_reason: SessionEndReason,
    pub context_manager: ContextManager,
    pub default_placeholder: Option<String>,
    pub follow_up_placeholder: Option<String>,
    pub next_checkpoint_turn: usize,
    pub file_palette_task_guard: BackgroundTaskGuard,
    pub background_subprocess_task_guard: Option<BackgroundTaskGuard>,
    pub startup_update_cached_notice: Option<StartupUpdateNotice>,
    pub startup_update_notice_rx: Option<tokio::sync::mpsc::UnboundedReceiver<StartupUpdateNotice>>,
    pub startup_update_task_guard: Option<BackgroundTaskGuard>,
}

pub(crate) async fn build_conversation_history_from_resume(
    resume: Option<&ResumeSession>,
) -> Vec<uni::Message> {
    let Some(session) = resume else {
        return Vec::new();
    };

    session.history().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use vtcode_core::core::threads::ArchivedSessionIntent;
    use vtcode_core::llm::provider::MessageRole;
    use vtcode_core::utils::session_archive::{
        SessionArchiveMetadata, SessionListing, SessionMessage, SessionSnapshot,
    };

    #[tokio::test]
    async fn plain_resume_with_full_history_does_not_inject_latest_memory_envelope() {
        let temp = tempdir().expect("tempdir");
        let history_dir = temp.path().join(".vtcode").join("history");
        fs::create_dir_all(&history_dir).expect("history dir");
        fs::write(
            history_dir.join("resume-session.memory.json"),
            serde_json::json!({
                "session_id": "resume-session",
                "schema_version": 2,
                "summary": "Persisted summary that must not be prepended",
                "task_summary": null,
                "spec_summary": null,
                "evaluation_summary": null,
                "grounded_facts": [],
                "touched_files": [],
                "history_artifact_path": null,
                "generated_at": "2026-03-14T00:00:00Z"
            })
            .to_string(),
        )
        .expect("write envelope");

        let listing = SessionListing {
            path: PathBuf::from("/tmp/resume-session.json"),
            snapshot: SessionSnapshot {
                metadata: SessionArchiveMetadata::new(
                    "workspace",
                    temp.path().to_string_lossy(),
                    "model",
                    "provider",
                    "theme",
                    "medium",
                ),
                started_at: Utc::now(),
                ended_at: Utc::now(),
                total_messages: 2,
                distinct_tools: Vec::new(),
                transcript: Vec::new(),
                messages: vec![
                    SessionMessage::new(MessageRole::User, "saved request"),
                    SessionMessage::new(MessageRole::Assistant, "saved answer"),
                ],
                progress: None,
                error_logs: Vec::new(),
            },
        };
        let resume = ResumeSession::from_listing(&listing, ArchivedSessionIntent::ResumeInPlace);

        let history = build_conversation_history_from_resume(Some(&resume)).await;

        assert_eq!(history.as_slice(), resume.history());
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].role, MessageRole::User);
        assert_eq!(history[0].content.as_text(), "saved request");
        assert_eq!(history[1].role, MessageRole::Assistant);
        assert_eq!(history[1].content.as_text(), "saved answer");
        assert!(history.iter().all(|message| {
            !message
                .content
                .as_text()
                .contains("[Session Memory Envelope]")
        }));
    }
}
