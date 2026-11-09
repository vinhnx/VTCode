use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState};

/// Request a stop through the Ctrl+C state machine and notify waiters.
///
/// # Priority Guarantee
///
/// This function is used by OS signal handling and explicit stop commands
/// whose double-press exit semantics are intentional. TUI key callbacks must
/// use [`request_local_cancel`] instead. It ensures that:
///
/// 1. The CtrlCState is atomically set to CancelRequested or ExitRequested
/// 2. At least one waiter is notified (using notify_one to store a permit)
/// 3. The notification is not lost even if no task is currently waiting
///
/// # Why notify_one instead of notify_waiters?
///
/// `notify_waiters()` only wakes tasks that are CURRENTLY waiting. If no task
/// is waiting, the notification is lost entirely. This causes Ctrl+C to be
/// unresponsive during tool execution and agent thinking when no task is
/// polling the notification.
///
/// `notify_one()` stores a permit even when no task is waiting. The next task
/// that calls `.notified()` will immediately receive the notification. This
/// ensures Ctrl+C is always responsive.
pub(crate) fn request_local_stop(ctrl_c_state: &Arc<CtrlCState>, ctrl_c_notify: &Arc<Notify>) -> CtrlCSignal {
    let signal = ctrl_c_state.register_signal();
    // Use notify_one instead of notify_waiters to store a permit.
    // This ensures the notification is not lost when no task is waiting.
    ctrl_c_notify.notify_one();
    signal
}

/// Request cancellation from a TUI key without enabling double-press exit.
pub(crate) fn request_local_cancel(ctrl_c_state: &Arc<CtrlCState>, ctrl_c_notify: &Arc<Notify>) {
    ctrl_c_state.request_local_cancel();
    ctrl_c_notify.notify_one();
}

/// Out-of-band stop handles for host adapters (the desktop JSON-RPC service).
///
/// A running turn only watches [`CtrlCState`]/[`Notify`]; `InlineEvent`s
/// queued in the session channel are not polled mid-turn. The TUI bridges
/// this gap with a send-time event callback
/// (`session_setup::build_session_event_callback`), but host sessions have no
/// TUI worker to invoke one — so the runloop publishes its handles here and
/// the host fires [`request_local_cancel`] directly.
pub(crate) type StopHandles = (Arc<CtrlCState>, Arc<Notify>);

/// Host stop handles and the point at which a submitted turn can consume a
/// cancellation. Exit may use the handles during initialization; turn stops
/// wait until initialization has finished.
#[derive(Default)]
pub(crate) struct HostStopHandles {
    handles: std::sync::OnceLock<StopHandles>,
    ready: std::sync::atomic::AtomicBool,
    ready_notify: Notify,
}

pub(crate) type StopHandleSlot = Arc<HostStopHandles>;

/// Create an empty stop-handle slot for one host-owned runloop.
#[must_use]
pub(crate) fn new_stop_handle_slot() -> StopHandleSlot {
    Arc::new(HostStopHandles::default())
}

/// Publish the runloop's stop handles. Called once after shell setup; a
/// repeated publish keeps the first handles.
pub(crate) fn publish_stop_handles(slot: &StopHandleSlot, state: &Arc<CtrlCState>, notify: &Arc<Notify>) {
    let _kept_first = slot.handles.set((Arc::clone(state), Arc::clone(notify)));
}

pub(crate) fn mark_host_turns_ready(slot: &StopHandleSlot) {
    slot.ready.store(true, std::sync::atomic::Ordering::Release);
    slot.ready_notify.notify_waiters();
}

pub(crate) async fn wait_for_host_turns_ready(slot: &StopHandleSlot) {
    loop {
        let notified = slot.ready_notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if slot.ready.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        notified.await;
    }
}

/// Fire a local cancel through published handles.
///
/// Returns `false` until initialization has finished. The host can await
/// readiness while retaining the turn identity; `InlineEvent::Cancel` only
/// dismisses overlay waits.
pub(crate) fn fire_stop_signal(slot: &StopHandleSlot) -> bool {
    if slot.ready.load(std::sync::atomic::Ordering::Acquire)
        && let Some((state, notify)) = slot.handles.get()
    {
        request_local_cancel(state, notify);
        true
    } else {
        false
    }
}

/// Fire a local exit through published handles.
///
/// Mirrors the TUI exit callback for host adapters: the queued
/// `InlineEvent::Exit` is only consumed once the interaction loop runs, so a
/// runloop paused in [`await_initialization`] needs this signal to end
/// gracefully instead of waiting for the host's abort timeout.
pub(crate) fn fire_exit_signal(slot: &StopHandleSlot) -> bool {
    if let Some((state, notify)) = slot.handles.get() {
        request_local_exit(state, notify);
        true
    } else {
        false
    }
}

/// Clear a handled cancellation when a fresh submission arrives.
///
/// Mirror of the TUI submit callback (`Submit`/`QueueSubmit` while handled
/// resets and wakes): without it, a stop pressed during initialization pauses
/// [`await_initialization`] forever, because host sessions have no callback to
/// deliver the reset. Returns `false` when the handles are unpublished or no
/// handled cancellation exists.
pub(crate) fn reset_handled_stop(slot: &StopHandleSlot) -> bool {
    if let Some((state, notify)) = slot.handles.get() {
        if state.reset_handled_cancel() {
            notify.notify_one();
            return true;
        }
    }
    false
}

/// Accept an explicit TUI exit immediately, even while initialization or a
/// provider request owns the asynchronous event consumer.
pub(crate) fn request_local_exit(ctrl_c_state: &Arc<CtrlCState>, ctrl_c_notify: &Arc<Notify>) {
    ctrl_c_state.request_exit();
    tracing::debug!(target: "vtcode.shutdown", boundary = "exit_accepted", elapsed_ms = 0u64);
    ctrl_c_notify.notify_one();
}

pub(crate) fn stop_outcome(state: &CtrlCState) -> Option<super::turn::context::TurnLoopResult> {
    if state.is_exit_requested() {
        Some(super::turn::context::TurnLoopResult::Exit)
    } else if state.is_cancel_requested() {
        Some(super::turn::context::TurnLoopResult::Cancelled)
    } else {
        None
    }
}

/// Wait until authoritative stop state is set. Notifications also wake fresh
/// submissions after a handled cancellation, so a permit alone is not a stop.
pub(crate) async fn wait_for_stop(state: &CtrlCState, notify: &Notify) {
    loop {
        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if stop_outcome(state).is_some() || state.is_cancel_handled() {
            return;
        }
        notified.await;
    }
}

/// Wait for a cancellation-safe future, with stop requests taking priority
/// over a simultaneously ready result. Blocking workers must own their
/// cleanup independently of this future.
pub(crate) async fn await_with_stop<T>(
    state: &CtrlCState,
    notify: &Notify,
    future: impl Future<Output = T>,
) -> Option<T> {
    tokio::pin!(future);
    loop {
        if stop_outcome(state).is_some() {
            return None;
        }
        tokio::select! {
            biased;
            _ = notify.notified() => {}
            value = &mut future => {
                return stop_outcome(state).is_none().then_some(value);
            }
        }
    }
}

/// Initialization has no active turn to discard. First cancellation pauses
/// initialization without dispatching tools; a fresh submission resumes it.
/// The TUI continues to own drafts and its event queue during this wait.
pub(crate) async fn await_initialization<T>(
    state: &CtrlCState,
    notify: &Notify,
    future: impl Future<Output = T>,
) -> Option<T> {
    tokio::pin!(future);
    let mut paused = state.is_cancel_handled();
    let mut prepared = None;
    loop {
        if state.is_exit_requested() {
            return None;
        }
        if state.is_cancel_requested() {
            state.mark_cancel_handled();
            paused = true;
        }
        if paused {
            notify.notified().await;
            if state.check_cancellation().is_ok() && !state.is_cancel_handled() {
                paused = false;
            }
            continue;
        }
        if let Some(result) = prepared.take() {
            return (!state.is_exit_requested()).then_some(result);
        }
        tokio::select! {
            biased;
            _ = notify.notified() => {}
            result = &mut future => prepared = Some(result),
        }
    }
}

pub(crate) struct InitializationExitContext<'a> {
    pub emitter: Option<&'a super::inline_events::harness::HarnessEventEmitter>,
    pub session_id: &'a str,
    pub config: &'a vtcode_core::config::types::AgentConfig,
    pub full_auto: bool,
    pub started: std::time::Instant,
}

pub(crate) async fn finish_initialization_exit(
    handle: &vtcode_ui::tui::app::InlineHandle,
    session: &mut vtcode_ui::tui::app::InlineSession,
    state: &CtrlCState,
    context: InitializationExitContext<'_>,
) {
    let InitializationExitContext { emitter, session_id, config, full_auto, started } = context;

    tracing::debug!(target: "vtcode.shutdown", boundary = "initialization_exit", accepted_exit_elapsed_ms = ?state.exit_elapsed_ms());
    handle.shutdown();
    let deadline = state
        .exit_deadline()
        .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_millis(1500));
    if let Some(emitter) = emitter {
        let _ = emitter.emit(crate::agent::runloop::unified::inline_events::harness::thread_completed_event(
            session_id,
            session_id,
            vtcode_core::exec::events::ThreadCompletionSubtype::Cancelled,
            "cancelled",
            None,
            Some("exit during initialization".to_owned()),
            Default::default(),
            None,
            0,
        ));
        let _ = tokio::time::timeout_at(deadline, emitter.finish()).await;
    }
    session
        .wait_for_exit(deadline.saturating_duration_since(tokio::time::Instant::now()))
        .await;
    let _ = vtcode_ui::tui::panic_hook::restore_tui_keep_raw_mode();
    vtcode_ui::tui::panic_hook::finish_deferred_raw_mode_restore();
    vtcode_core::utils::transcript::clear_inline_handle();
    vtcode_core::ui::set_tui_mode(false);
    tracing::debug!(target: "vtcode.shutdown", boundary = "terminal_restored", accepted_exit_elapsed_ms = ?state.exit_elapsed_ms());
    crate::agent::runloop::unified::postamble::print_exit_summary(
        crate::agent::runloop::unified::postamble::ExitData {
            app_name: vtcode_core::config::constants::app::DISPLAY_NAME,
            version: env!("CARGO_PKG_VERSION"),
            model: &config.model,
            provider: &config.provider,
            trust_label: if full_auto { "full auto" } else { "tools policy" },
            reasoning: config.reasoning_effort.as_str(),
            session_duration: started.elapsed(),
            prompt_tokens: 0,
            completion_tokens: 0,
            cached_tokens: 0,
            cache_creation_tokens: 0,
            cache_hit_rate_percent: None,
            code_additions: 0,
            code_deletions: 0,
            final_response: None,
            resume_identifier: None,
            budget_limit: None,
            total_cost_usd: None,
            end_reason_label: "exit",
            first_call_composition: None,
            session_end_reason: vtcode_core::hooks::SessionEndReason::Exit,
        },
    );
    tracing::debug!(target: "vtcode.shutdown", boundary = "postamble_finished", accepted_exit_elapsed_ms = ?state.exit_elapsed_ms());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resume_wake_requires_authoritative_stop_before_cancelling() {
        let state = CtrlCState::new();
        let notify = Notify::new();
        state.request_local_cancel();
        state.mark_cancel_handled();
        state.reset();
        // The fresh-submission callback wakes initialization after clearing cancel.
        notify.notify_one();
        let waiting = wait_for_stop(&state, &notify);
        tokio::pin!(waiting);
        assert!(futures::poll!(&mut waiting).is_pending());
        notify.notify_one();
        assert!(futures::poll!(&mut waiting).is_pending());
        state.request_local_cancel();
        notify.notify_one();
        assert!(futures::poll!(&mut waiting).is_ready());
        state.mark_cancel_handled();
        assert!(futures::poll!(std::pin::pin!(wait_for_stop(&state, &notify))).is_ready());
        state.reset();
        state.request_exit();
        assert!(futures::poll!(std::pin::pin!(wait_for_stop(&state, &notify))).is_ready());
    }

    #[tokio::test]
    async fn pending_preparation_stops_and_a_ready_result_cannot_erase_exit() {
        let state = Arc::new(CtrlCState::new());
        let notify = Arc::new(Notify::new());
        let pending = await_with_stop(&state, &notify, std::future::pending::<()>());
        let cancel = async {
            request_local_cancel(&state, &notify);
        };
        let (result, ()) = tokio::join!(pending, cancel);
        assert!(result.is_none());
        state.mark_cancel_handled();
        state.reset();
        assert_eq!(await_with_stop(&state, &notify, async { 17 }).await, Some(17));
        request_local_exit(&state, &notify);
        state.reset();
        state.request_local_cancel();
        state.mark_cancel_handled();
        assert!(state.is_exit_requested());
        assert_eq!(await_with_stop(&state, &notify, async { 29 }).await, None);
    }

    #[tokio::test]
    async fn initialization_pauses_on_cancel_and_exits_without_resuming_work() {
        let state = Arc::new(CtrlCState::new());
        let notify = Arc::new(Notify::new());
        request_local_cancel(&state, &notify);
        let polled = std::cell::Cell::new(false);
        let initializing = await_initialization(&state, &notify, async {
            polled.set(true);
            31
        });
        let exit = async {
            tokio::task::yield_now().await;
            assert!(state.is_cancel_handled());
            request_local_exit(&state, &notify);
        };
        let (result, ()) = tokio::join!(initializing, exit);
        assert!(result.is_none());
        assert!(!polled.get());
    }

    #[test]
    fn stop_handle_slot_fires_only_after_publish_and_keeps_first() {
        let slot = new_stop_handle_slot();
        assert!(!fire_stop_signal(&slot));
        assert!(!fire_exit_signal(&slot));
        assert!(!reset_handled_stop(&slot));
        let first = Arc::new(CtrlCState::new());
        let first_notify = Arc::new(Notify::new());
        publish_stop_handles(&slot, &first, &first_notify);
        assert!(!fire_stop_signal(&slot), "initialization is not a running turn");
        mark_host_turns_ready(&slot);
        assert!(fire_stop_signal(&slot));
        assert!(first.is_cancel_requested());
        let second = Arc::new(CtrlCState::new());
        publish_stop_handles(&slot, &second, &Arc::new(Notify::new()));
        assert!(fire_stop_signal(&slot));
        assert!(!second.is_cancel_requested(), "first publish wins");
    }

    #[test]
    fn exit_signal_requests_exit_and_reset_clears_only_handled_cancels() {
        let slot = new_stop_handle_slot();
        let state = Arc::new(CtrlCState::new());
        let notify = Arc::new(Notify::new());
        publish_stop_handles(&slot, &state, &notify);
        mark_host_turns_ready(&slot);
        assert!(!reset_handled_stop(&slot), "nothing handled, nothing cleared");
        state.request_local_cancel();
        assert!(!reset_handled_stop(&slot), "an unhandled request is not a fresh submission");
        assert!(state.is_cancel_requested());
        state.mark_cancel_handled();
        assert!(reset_handled_stop(&slot));
        assert!(!state.is_cancel_requested());
        assert!(!state.is_cancel_handled());
        assert!(fire_exit_signal(&slot));
        assert!(state.is_exit_requested());
    }

    #[tokio::test]
    async fn initialization_resumes_only_after_handled_cancellation_is_cleared() {
        let state = Arc::new(CtrlCState::new());
        let notify = Arc::new(Notify::new());
        request_local_cancel(&state, &notify);
        let initializing = await_initialization(&state, &notify, async { 41 });
        let submit = async {
            tokio::task::yield_now().await;
            assert!(state.is_cancel_handled());
            state.reset();
            notify.notify_one();
        };
        let (result, ()) = tokio::join!(initializing, submit);
        assert_eq!(result, Some(41));
    }
}
