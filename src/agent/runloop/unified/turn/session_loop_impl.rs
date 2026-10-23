use std::io::Write as _;

use anyhow::Result;

use tokio_util::sync::CancellationToken;
use vtcode_ui::tui::app::InlineSession;

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
        None,
        None,
    )
    .await
}

/// Run the unified agent loop through a host-owned event session.
///
/// The session transports user input and presentation commands without
/// initializing a terminal. Runtime execution, policy, persistence, and
/// verification remain owned by the unified runloop.
#[allow(
    clippy::too_many_arguments,
    reason = "Preserve the unified runloop's established typed startup inputs while replacing only its session and event endpoints."
)]
pub(crate) async fn run_single_agent_loop_unified_with_host(
    config: &CoreAgentConfig,
    vt_cfg: Option<VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
    primary_agent_explicitly_configured: bool,
    planning_entry_source: PlanningEntrySource,
    resume: Option<ResumeSession>,
    mut steering_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
    session: InlineSession,
    event_sender: vtcode_desktop_protocol::LiveEventSender,
) -> Result<()> {
    session_loop_runner::run_single_agent_loop_unified_impl(
        config,
        vt_cfg,
        skip_confirmations,
        full_auto,
        primary_agent_explicitly_configured,
        planning_entry_source,
        resume,
        &mut steering_receiver,
        Some(session),
        Some(event_sender),
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
    fn new(terminal_enabled: bool) -> Option<Self> {
        terminal_enabled.then_some(Self)
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
