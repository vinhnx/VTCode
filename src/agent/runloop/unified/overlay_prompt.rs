use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;
use tokio::task;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_ui::tui::app::{InlineEvent, InlineHandle, TransientEvent, TransientRequest, TransientSubmission};

use super::state::CtrlCState;

pub(crate) enum OverlayWaitOutcome<T> {
    Submitted(T),
    Cancelled,
    Interrupted,
    Exit,
}

pub(crate) async fn show_overlay_and_wait<S, T, F>(
    handle: &InlineHandle,
    session: &mut S,
    request: TransientRequest,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    map_submission: F,
) -> Result<OverlayWaitOutcome<T>>
where
    S: UiSession + ?Sized,
    F: FnMut(TransientSubmission) -> Option<T>,
{
    handle.show_transient(request);
    handle.force_redraw();
    task::yield_now().await;
    wait_for_overlay_submission(handle, session, ctrl_c_state, ctrl_c_notify, map_submission).await
}

pub(crate) async fn wait_for_overlay_submission<S, T, F>(
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    mut map_submission: F,
) -> Result<OverlayWaitOutcome<T>>
where
    S: UiSession + ?Sized,
    F: FnMut(TransientSubmission) -> Option<T>,
{
    loop {
        if ctrl_c_state.is_cancel_requested() {
            close_overlay(handle).await;
            return Ok(OverlayWaitOutcome::Interrupted);
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            event = session.next_event() => event,
        };

        let Some(event) = maybe_event else {
            close_overlay(handle).await;
            if ctrl_c_state.is_cancel_requested() {
                return Ok(OverlayWaitOutcome::Interrupted);
            }
            return Ok(OverlayWaitOutcome::Exit);
        };

        match event {
            InlineEvent::Interrupt => {
                // Esc / Ctrl+C from the TUI.  `request_local_stop()` sets
                // CancelRequested so the turn loop detects the interruption.
                // Single Ctrl+C closes the overlay; double Ctrl+C exits.
                crate::agent::runloop::unified::stop_requests::request_local_stop(ctrl_c_state, ctrl_c_notify);
                close_overlay(handle).await;
                return Ok(OverlayWaitOutcome::Interrupted);
            }
            InlineEvent::Transient(TransientEvent::Submitted(submission)) => {
                ctrl_c_state.reset();
                if let Some(mapped) = map_submission(submission) {
                    return Ok(OverlayWaitOutcome::Submitted(mapped));
                }
            }
            InlineEvent::Transient(TransientEvent::Cancelled) | InlineEvent::Cancel => {
                ctrl_c_state.reset();
                return Ok(OverlayWaitOutcome::Cancelled);
            }
            InlineEvent::Exit => {
                ctrl_c_state.reset();
                close_overlay(handle).await;
                return Ok(OverlayWaitOutcome::Exit);
            }
            InlineEvent::Submit(_) | InlineEvent::QueueSubmit(_) => continue,
            InlineEvent::Transient(_) => {}
            _ => {}
        }
    }
}

async fn close_overlay(handle: &InlineHandle) {
    handle.close_transient();
    handle.force_redraw();
    task::yield_now().await;
}
