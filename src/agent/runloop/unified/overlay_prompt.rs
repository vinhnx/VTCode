use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;
use tokio::task;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_tui::{InlineEvent, InlineHandle, OverlayEvent, OverlayRequest, OverlaySubmission};

use super::state::{CtrlCSignal, CtrlCState};
use super::stop_requests::request_local_stop;

pub(crate) enum OverlayWaitOutcome<T> {
    Submitted(T),
    Cancelled,
    Interrupted,
    Exit,
}

pub(crate) async fn show_overlay_and_wait<S, T, F>(
    handle: &InlineHandle,
    session: &mut S,
    request: OverlayRequest,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    map_submission: F,
) -> Result<OverlayWaitOutcome<T>>
where
    S: UiSession + ?Sized,
    F: FnMut(OverlaySubmission) -> Option<T>,
{
    handle.show_overlay(request);
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
    F: FnMut(OverlaySubmission) -> Option<T>,
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
                let signal = if ctrl_c_state.is_exit_requested() {
                    CtrlCSignal::Exit
                } else if ctrl_c_state.is_cancel_requested() {
                    CtrlCSignal::Cancel
                } else {
                    request_local_stop(ctrl_c_state, ctrl_c_notify)
                };
                close_overlay(handle).await;
                return Ok(match signal {
                    CtrlCSignal::Exit => OverlayWaitOutcome::Exit,
                    CtrlCSignal::Cancel => OverlayWaitOutcome::Interrupted,
                });
            }
            InlineEvent::Overlay(OverlayEvent::Submitted(submission)) => {
                ctrl_c_state.reset();
                if let Some(mapped) = map_submission(submission) {
                    return Ok(OverlayWaitOutcome::Submitted(mapped));
                }
            }
            InlineEvent::Overlay(OverlayEvent::Cancelled) | InlineEvent::Cancel => {
                ctrl_c_state.reset();
                return Ok(OverlayWaitOutcome::Cancelled);
            }
            InlineEvent::Exit => {
                ctrl_c_state.reset();
                close_overlay(handle).await;
                return Ok(OverlayWaitOutcome::Exit);
            }
            InlineEvent::Submit(_) | InlineEvent::QueueSubmit(_) => continue,
            InlineEvent::Overlay(_) => {}
            _ => {}
        }
    }
}

async fn close_overlay(handle: &InlineHandle) {
    handle.close_overlay();
    handle.force_redraw();
    task::yield_now().await;
}
