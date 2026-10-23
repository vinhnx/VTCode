use std::collections::VecDeque;
use std::ops::ControlFlow;
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;
use vtcode_commons::ui_protocol::ActivityState;
use vtcode_config::loader::SimpleConfigWatcher;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::agent::runtime::AgentRuntime;
use vtcode_core::core::agent::session::AgentSessionState;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_core::exec::events::ThreadCompletionSubtype;
use vtcode_core::hooks::{SessionEndReason, SessionStartTrigger};
use vtcode_core::llm::provider::MessageRole;
use vtcode_core::session::SessionId;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::session_archive;
use vtcode_core::utils::session_archive::SessionMessage;
use vtcode_ui::tui::app::ArchivedPromptEntry;
use vtcode_ui::tui::app::InlineSession;

use super::super::{CancelGuard, TerminalCleanupGuard};
use super::archive::refresh_runtime_debug_context_for_next_session;
use super::blocked_handoff::write_blocked_handoff_after_checkpoint;
use super::handoff::{
    append_approved_plan_execution_input, apply_primary_agent_tool_policy_overrides,
    build_approved_plan_execution_prompt, report_plan_approval_selection_failure, select_approved_plan_execution_agent,
};
use super::metrics::{capture_code_change_snapshot, estimate_history_bytes};
use super::plan_seed::load_active_plan_seed;
use super::support::{
    ExecutionSummaryStatus, RefusedTurnRollback, approved_plan_execution_summary, build_unrelated_dirty_worktree_note,
    build_withdrawn_turn_changes_note, checkpoint_session_archive_start, checkpoint_unavailable_notice,
    force_reload_workspace_config_for_execution, format_workspace_relative_paths, latest_assistant_result_text,
    prepare_transient_turn_notes, prompt_startup_planning_workflow, remove_transient_system_notes,
};
use super::turn_tail::{TurnPersistenceTail, complete_turn_persistence_tail};
use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::git::{compute_session_code_change_delta, normalize_workspace_path};
use crate::agent::runloop::model_picker::ModelPickerState;
use crate::agent::runloop::unified::inline_events::harness::harness_event;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::planning_workflow_state::{
    render_planning_workflow_next_step_hint, transition_to_planning_workflow,
};
use crate::agent::runloop::unified::postamble::{ExitData, print_exit_summary};
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use crate::agent::runloop::unified::session_setup::{
    SessionState, initialize_session_critical, initialize_session_ui, spawn_signal_handler,
};
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::turn::background_completion::PendingBackgroundCompletions;
use crate::agent::runloop::unified::turn::context::TurnLoopResult as RunLoopTurnLoopResult;
use crate::agent::runloop::unified::turn::finalization::finalize_session;
use crate::agent::runloop::unified::turn::primary_agent_runtime::{
    PrimaryAgentRuntimeSyncContext, sync_primary_agent_permissions, sync_primary_agent_runtime,
};
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopOutcome;
use crate::agent::runloop::unified::turn::turn_loop_helpers::{
    effective_max_tool_calls_for_approved_plan_execution, effective_max_tool_calls_for_turn,
    resolve_safety_tool_call_limits,
};
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::updater::{InlineUpdateOutcome, display_update_notice, run_inline_update_prompt};

mod session_bootstrap;
mod session_iteration;
mod session_teardown;

use session_iteration::SessionLoopState;

pub(crate) use session_bootstrap::BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX;
pub(super) use session_bootstrap::resolve_thread_completion_status;
use session_bootstrap::{
    apply_startup_plan_agent_selection, background_completion_continuation_prompt, load_archived_prompts_for_history,
    persist_primary_agent, record_plan_selection_failure_tail,
};

#[cfg_attr(feature = "profiling", hotpath::measure)]
#[allow(
    clippy::too_many_arguments,
    reason = "These values are the unified runloop's existing startup contract; the host adapter supplies its session and canonical event sender."
)]
pub(crate) async fn run_single_agent_loop_unified_impl(
    config: &CoreAgentConfig,
    initial_vt_cfg: Option<VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
    primary_agent_explicitly_configured: bool,
    planning_entry_source: PlanningEntrySource,
    resume: Option<ResumeSession>,
    steering_receiver: &mut Option<mpsc::UnboundedReceiver<SteeringMessage>>,
    mut host_session: Option<InlineSession>,
    event_sender: Option<vtcode_desktop_protocol::LiveEventSender>,
) -> Result<()> {
    let terminal_enabled = host_session.is_none();
    let _terminal_cleanup_guard = TerminalCleanupGuard::new(terminal_enabled);

    let mut config = config.clone();
    let mut session_skip_confirmations = skip_confirmations;
    let mut resume_state = resume;
    let mut config_watcher = SimpleConfigWatcher::new_with_user_config_paths(config.workspace.clone());
    config_watcher.set_check_interval(15);
    config_watcher.set_debounce_duration(500);
    if let Some(initial_config) = initial_vt_cfg.as_ref() {
        config_watcher.set_last_known_config(initial_config.clone());
    }
    let mut vt_cfg = initial_vt_cfg.or_else(|| config_watcher.load_config());
    let mut pending_session_start_trigger = None;
    let mut next_session_primary_agent: Option<String> = None;

    loop {
        let state = SessionLoopState {
            config: &mut config,
            session_skip_confirmations: &mut session_skip_confirmations,
            resume_state: &mut resume_state,
            config_watcher: &mut config_watcher,
            vt_cfg: &mut vt_cfg,
            pending_session_start_trigger: &mut pending_session_start_trigger,
            next_session_primary_agent: &mut next_session_primary_agent,
            steering_receiver: &mut *steering_receiver,
            host_session: &mut host_session,
            event_sender: event_sender.clone(),
            terminal_enabled,
            skip_confirmations,
            full_auto,
            primary_agent_explicitly_configured,
            planning_entry_source,
        };
        match session_iteration::run_session_iteration(state).await? {
            ControlFlow::Break(()) => break,
            ControlFlow::Continue(()) if !terminal_enabled => {
                anyhow::bail!("the desktop host cannot start a replacement session from this runloop instance")
            }
            ControlFlow::Continue(()) => {}
        }
    }
    Ok(())
}
