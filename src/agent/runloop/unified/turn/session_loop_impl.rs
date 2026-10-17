use std::io::Write as _;

use anyhow::Result;

use tokio_util::sync::CancellationToken;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::core::interfaces::session::PlanningEntrySource;

use crate::agent::runloop::ResumeSession;

#[path = "session_loop_runner/mod.rs"]
mod session_loop_runner;

pub(crate) use session_loop_runner::{
    BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX, VERIFICATION_AUTO_RECOVERY_PREFIX,
};

const RECENT_MESSAGE_LIMIT: usize = 16;

#[cfg_attr(feature = "profiling", hotpath::measure)]
pub(crate) async fn run_single_agent_loop_unified(
    config: &CoreAgentConfig,
    _vt_cfg: Option<VTCodeConfig>,
    _skip_confirmations: bool,
    full_auto: bool,
    primary_agent_explicitly_configured: bool,
    planning_entry_source: PlanningEntrySource,
    resume: Option<ResumeSession>,
    mut steering_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
) -> Result<()> {
    session_loop_runner::run_single_agent_loop_unified_impl(
        config,
        _vt_cfg,
        _skip_confirmations,
        full_auto,
        primary_agent_explicitly_configured,
        planning_entry_source,
        resume,
        &mut steering_receiver,
    )
    .await
}

/// Guard that ensures terminal is restored to a clean state when dropped.
/// Backstop for paths where the TUI doesn't shut down cleanly or the session
/// exits early (Ctrl+C, SIGTERM, panic). Delegates to the canonical
/// `restore_tui()` (idempotent via `RESTORE_DONE`) so fullscreen teardown
/// emits each escape sequence once and never leaks alternate-buffer frames
/// into the shell scrollback.
struct TerminalCleanupGuard;

impl TerminalCleanupGuard {
    fn new() -> Self {
        Self
    }
}

impl Drop for TerminalCleanupGuard {
    fn drop(&mut self) {
        let _ = vtcode_ui::tui::panic_hook::restore_tui();
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
    }
}

/// Guard that ensures a CancellationToken is cancelled when dropped
struct CancelGuard(CancellationToken);

impl Drop for CancelGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
