//! Restore durable steering and interrupted inputs without changing provenance.

use std::path::Path;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::runtime::AgentRuntime;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::super::super::support::take_pending_resumed_user_prompt;
use crate::agent::runloop::unified::state::CtrlCState;

pub(super) struct FollowUpRestoreContext<'a> {
    pub(super) workspace_root: &'a Path,
    pub(super) vt_cfg: Option<&'a VTCodeConfig>,
    pub(super) resumed: bool,
    pub(super) ctrl_c_state: &'a CtrlCState,
    pub(super) renderer: &'a mut AnsiRenderer,
}

/// Restore saved intents first, then interrupted input or an adopted tracker.
/// Fresh sessions must not consume a trailing prompt as resumed input.
pub(super) async fn restore_session_follow_ups(
    runtime: &mut AgentRuntime,
    tool_registry: &ToolRegistry,
    context: FollowUpRestoreContext<'_>,
) -> anyhow::Result<()> {
    let FollowUpRestoreContext {
        workspace_root,
        vt_cfg,
        resumed,
        ctrl_c_state,
        renderer,
    } = context;
    let durable_session_id = tool_registry.harness_context_snapshot().session_id;
    let mut saved_envelope = vtcode_core::compaction::memory_envelope::load_latest_memory_envelope_async(
        workspace_root,
        &durable_session_id,
    )
    .await;
    if let Some(envelope) = saved_envelope.as_ref() {
        runtime.restore_follow_up_state(envelope.pending_intents.clone(), envelope.applied_intent_ids.clone())?;
    }
    let original_history = resumed.then(|| runtime.state.messages.clone());
    if resumed && let Some(pending_prompt) = take_pending_resumed_user_prompt(runtime.state.messages_mut()) {
        let (_, runtime_steering) = runtime.split_mut();
        let already_queued = runtime_steering
            .pending_follow_up_intents_snapshot()
            .iter()
            .any(|intent| intent.id() == pending_prompt.id());
        if let Err(error) = if already_queued {
            Ok(())
        } else {
            runtime_steering.try_queue_recovered_follow_up(pending_prompt)
        } {
            if let Some(history) = original_history {
                runtime.state.messages = history;
            }
            return Err(error.into());
        } else if let Some(envelope) = saved_envelope.as_mut() {
            envelope.pending_intents = runtime.split_mut().1.pending_follow_up_intents_snapshot();
            let path = vtcode_core::compaction::memory_envelope::latest_memory_envelope_path_for_session_async(
                workspace_root,
                &durable_session_id,
            )
            .await
            .unwrap_or_else(|| {
                vtcode_core::compaction::memory_envelope::default_memory_envelope_path_for_session(
                    workspace_root,
                    &durable_session_id,
                )
            });
            vtcode_core::compaction::memory_envelope::write_memory_envelope_to_path_async(&path, envelope).await?;
        }
    } else if resumed {
        use crate::agent::runloop::unified::turn::tool_outcomes::helpers as tracker_continue;
        let auto_continue_enabled = crate::agent::runloop::unified::stop_requests::stop_outcome(ctrl_c_state).is_none()
            && tracker_continue::tracker_auto_continue_enabled(vt_cfg);
        let cross_turn_turns = tracker_continue::tracker_cross_turn_turns(vt_cfg);
        let tracker_adoption_allowed = tool_registry.tracker_adopted_for_request();
        let incomplete = if auto_continue_enabled && tracker_adoption_allowed && cross_turn_turns > 0 {
            tracker_continue::incomplete_tracker_items(tool_registry).await
        } else {
            None
        };
        if !tool_registry.is_planning_active()
            && tracker_continue::should_queue_tracker_resume_continuation(
                auto_continue_enabled,
                tracker_adoption_allowed,
                cross_turn_turns,
                incomplete.as_deref(),
            )
        {
            let incomplete = incomplete.unwrap_or_default();
            // Resume with open TODO/tracker steps: auto-queue one continuation
            // turn instead of waiting for the user to type continue.
            let follow_up = tracker_continue::tracker_continue_follow_up(&incomplete);
            let directive = tracker_continue::tracker_continue_directive(
                tracker_continue::TRACKER_RESUME_DIRECTIVE_LABEL,
                &incomplete,
            );
            let (_, runtime_steering) = runtime.split_mut();
            match runtime_steering.try_queue_synthetic_follow_up_input(follow_up) {
                Ok(()) => {
                    std::sync::Arc::make_mut(&mut runtime.state.messages)
                        .push(vtcode_core::llm::provider::Message::system(directive));
                    let _ = renderer.line(
                            MessageStyle::Info,
                            "[i] Resumed session has incomplete task_tracker steps; auto-continuing without manual `continue`.",
                        );
                }
                Err(error) => {
                    tracing::warn!(%error, "Unable to queue tracker resume continuation");
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
