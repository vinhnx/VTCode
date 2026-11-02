//! Deferred initialization borrows the session owners; cancellation and errors
//! return to the iteration before its guards are dropped.

use std::ops::ControlFlow;
use std::time::{Duration, Instant};

use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::hooks::SessionStartTrigger;

use super::super::super::harness;
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::run_loop_context::TurnRunId;
use crate::agent::runloop::unified::session_setup::{
    SessionState, SessionUISetup, apply_post_hydration_ui, hydrate_session_runtime, run_session_start_hooks,
};

pub(super) struct SessionInitializationContext<'a> {
    pub(super) config: &'a CoreAgentConfig,
    pub(super) vt_cfg: Option<&'a VTCodeConfig>,
    pub(super) full_auto: bool,
    pub(super) primary_agent_explicitly_configured: bool,
    pub(super) resume_ref: Option<&'a ResumeSession>,
    pub(super) session_id: &'a str,
    pub(super) session_primary_agent_override: Option<&'a str>,
    pub(super) session_trigger: SessionStartTrigger,
    pub(super) harness_emitter: Option<&'a HarnessEventEmitter>,
    pub(super) session_started_at: Instant,
}

/// Complete registry, hydration and hooks after first paint, in that order.
/// The caller owns progress and canonical error finalization.
pub(super) async fn complete_session_initialization(
    session_state: &mut SessionState,
    ui_setup: &mut SessionUISetup,
    context: SessionInitializationContext<'_>,
) -> Result<ControlFlow<()>> {
    let SessionInitializationContext {
        config,
        vt_cfg,
        full_auto,
        primary_agent_explicitly_configured,
        resume_ref,
        session_id,
        session_primary_agent_override,
        session_trigger,
        harness_emitter,
        session_started_at,
    } = context;
    macro_rules! initialization_wait {
        ($future:expr) => {{
            let state = ui_setup.ctrl_c_state.clone();
            let notify = ui_setup.ctrl_c_notify.clone();
            match crate::agent::runloop::unified::stop_requests::await_initialization(&state, &notify, $future).await {
                Some(result) => result?,
                None => {
                    crate::agent::runloop::unified::stop_requests::finish_initialization_exit(
                        &ui_setup.handle,
                        &mut ui_setup.session,
                        &state,
                        crate::agent::runloop::unified::stop_requests::InitializationExitContext {
                            emitter: harness_emitter,
                            session_id,
                            config,
                            full_auto,
                            started: session_started_at,
                        },
                    )
                    .await;
                    return Ok(ControlFlow::Break(()));
                }
            }
        }};
    }
    initialization_wait!(crate::agent::runloop::unified::session_setup::complete_session_registry(
        session_state,
        config,
        vt_cfg,
        full_auto,
        primary_agent_explicitly_configured,
        resume_ref,
        session_id,
        session_primary_agent_override,
    ));

    // Retention walks the session store and may rmtree dozens of dirs.
    // Scheduled only after first paint is available so a large archive
    // cannot delay the first frame; still best-effort background.
    {
        let workspace = config.workspace.clone();
        let vt_cfg = vt_cfg.cloned();
        let turn_run_id = TurnRunId(session_id.to_owned());
        tokio::spawn(async move {
            harness::run_harness_retention(&workspace, vt_cfg.as_ref(), &turn_run_id).await;
        });
    }

    // Deferred hydration runs after the TUI first frame is available.
    // The interaction loop must not dispatch a model turn until this
    // completes; setup failures abort with the historical setup error.
    let session_hydrate_phase = vtcode_commons::startup_trace::phase_started();
    initialization_wait!(hydrate_session_runtime(
        session_state,
        &mut ui_setup.context_manager,
        config,
        vt_cfg,
        full_auto,
        primary_agent_explicitly_configured,
        resume_ref,
        session_id,
        session_primary_agent_override,
    ));
    vtcode_commons::startup_trace::record_phase("session_setup_hydrate", session_hydrate_phase);

    // Re-drive UI surfaces that depend on hydrated session state.
    let post_hydrate_guard = apply_post_hydration_ui(vt_cfg, full_auto, session_state, ui_setup)?;
    if let Some(guard) = post_hydrate_guard {
        // Prefer the post-hydrate refresh task when a controller appeared
        // after first paint.
        ui_setup.background_subprocess_task_guard = Some(guard);
    }

    // Session-start hooks run only after hydration so they observe the
    // fully initialized tool registry. For `/new` the fresh prompt is
    // waiting, so bound the total: per-command timeouts default to 60s
    // and run sequentially, which can park `/new` for 10s+ on a slow
    // hook. Timeout skips remaining hooks (warn, continue) instead of
    // aborting the session — startup keeps full budgets.
    if matches!(session_trigger, SessionStartTrigger::NewSession) {
        initialization_wait!(async {
            match tokio::time::timeout(
                Duration::from_secs(2),
                run_session_start_hooks(ui_setup.lifecycle_hooks.as_ref(), &mut ui_setup.renderer, session_state),
            )
            .await
            {
                Ok(result) => {
                    result?;
                }
                Err(_elapsed) => {
                    tracing::warn!(
                        "session-start hooks timed out on /new fast path; continuing without remaining hooks"
                    );
                }
            }
            Ok::<(), anyhow::Error>(())
        });
    } else {
        initialization_wait!(run_session_start_hooks(
            ui_setup.lifecycle_hooks.as_ref(),
            &mut ui_setup.renderer,
            session_state
        ));
    }

    Ok(ControlFlow::Continue(()))
}
