use anyhow::Result;
use std::collections::VecDeque;
use std::io::Write;

use std::path::Path;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use tokio::time::{Duration, sleep, timeout};
use vtcode::config_watcher::SimpleConfigWatcher;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::resolve_timeout;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

/// Optimization: Pre-computed idle detection thresholds to avoid repeated config lookups
#[derive(Clone, Copy)]
struct IdleDetectionConfig {
    timeout_ms: u64,
    backoff_ms: u64,
    max_cycles: usize,
    enabled: bool,
}

use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, resolve_event_log_path,
};
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use chrono::Utc;
use vtcode_core::exec::events::{ThreadEvent, ThreadStartedEvent};
use vtcode_core::session::{SessionId, session_path};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::session_archive::{SessionMessage, SessionProgressArgs};

use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::unified::plan_mode_state::transition_to_plan_mode;

use super::super::context::TurnLoopResult as RunLoopTurnLoopResult;
use super::super::finalization::finalize_session;
// use super::finalization::finalize_session;
use vtcode_core::core::agent::steering::SteeringMessage;

use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::session_setup::{
    SessionState, initialize_session, initialize_session_ui, spawn_signal_handler,
};
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::hooks::lifecycle::{SessionEndReason, SessionStartTrigger};

#[path = "session_loop_runner.rs"]
mod session_loop_runner;

const RECENT_MESSAGE_LIMIT: usize = 16;

/// Optimization: Extract idle detection config once to avoid repeated Option unwrapping
#[inline]
fn extract_idle_config(vt_cfg: Option<&VTCodeConfig>) -> IdleDetectionConfig {
    vt_cfg
        .map(|cfg| {
            let idle_config = &cfg.optimization.agent_execution;
            IdleDetectionConfig {
                timeout_ms: idle_config.idle_timeout_ms,
                backoff_ms: idle_config.idle_backoff_ms,
                max_cycles: idle_config.max_idle_cycles,
                enabled: idle_config.idle_timeout_ms > 0,
            }
        })
        .unwrap_or(IdleDetectionConfig {
            timeout_ms: 0,
            backoff_ms: 0,
            max_cycles: 0,
            enabled: false,
        })
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_single_agent_loop_unified(
    config: &CoreAgentConfig,
    _vt_cfg: Option<VTCodeConfig>,
    _skip_confirmations: bool,
    full_auto: bool,
    plan_mode: bool,
    team_context: Option<vtcode_core::agent_teams::TeamContext>,
    resume: Option<ResumeSession>,
    mut steering_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
) -> Result<()> {
    session_loop_runner::run_single_agent_loop_unified_impl(
        config,
        _vt_cfg,
        _skip_confirmations,
        full_auto,
        plan_mode,
        team_context,
        resume,
        &mut steering_receiver,
    )
    .await
}

/// Guard that ensures terminal is restored to a clean state when dropped
/// This handles cases where the TUI doesn't shutdown cleanly or the session
/// exits early (e.g., due to Ctrl+C or other signals)
struct TerminalCleanupGuard;

impl TerminalCleanupGuard {
    fn new() -> Self {
        Self
    }
}

impl Drop for TerminalCleanupGuard {
    fn drop(&mut self) {
        // Minimal terminal cleanup as last resort using centralized logic
        // The TUI's run_inline_tui should handle full cleanup, this is just a safety net
        let _ = vtcode_tui::panic_hook::restore_tui();

        // Ensure stdout is also flushed
        let mut stdout = std::io::stdout();
        let _ = stdout.flush();

        // Wait for terminal to finish processing any pending operations
        // This prevents incomplete writes from corrupting the terminal
        let delay_ms = std::env::var("VT_TERMINAL_CLEANUP_DELAY_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(50);
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }
}

/// Guard that ensures a CancellationToken is cancelled when dropped
struct CancelGuard(CancellationToken);

impl Drop for CancelGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
