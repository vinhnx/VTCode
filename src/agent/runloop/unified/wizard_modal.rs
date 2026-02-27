use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::Notify;
use tokio::task;

use vtcode_tui::{InlineEvent, InlineHandle, InlineListSelection, InlineSession};

use super::state::{CtrlCSignal, CtrlCState};

pub(crate) enum WizardModalOutcome {
    Submitted(Vec<InlineListSelection>),
    Cancelled { signal: Option<&'static str> },
}

pub(crate) async fn wait_for_wizard_modal(
    handle: &InlineHandle,
    session: &mut InlineSession,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<WizardModalOutcome> {
    loop {
        if ctrl_c_state.is_cancel_requested() {
            close_modal(handle).await;
            return Ok(WizardModalOutcome::Cancelled { signal: None });
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            event = session.next_event() => event,
        };

        let Some(event) = maybe_event else {
            close_modal(handle).await;
            return Ok(WizardModalOutcome::Cancelled { signal: None });
        };

        match event {
            InlineEvent::Interrupt => {
                let signal = if ctrl_c_state.is_exit_requested() {
                    CtrlCSignal::Exit
                } else if ctrl_c_state.is_cancel_requested() {
                    CtrlCSignal::Cancel
                } else {
                    ctrl_c_state.register_signal()
                };
                ctrl_c_notify.notify_waiters();
                close_modal(handle).await;
                let signal = match signal {
                    CtrlCSignal::Exit => Some("exit"),
                    CtrlCSignal::Cancel => Some("cancel"),
                };
                return Ok(WizardModalOutcome::Cancelled { signal });
            }
            InlineEvent::WizardModalSubmit(selections) => {
                ctrl_c_state.disarm_exit();
                close_modal(handle).await;
                return Ok(WizardModalOutcome::Submitted(selections));
            }
            InlineEvent::WizardModalCancel | InlineEvent::ListModalCancel | InlineEvent::Cancel => {
                ctrl_c_state.disarm_exit();
                close_modal(handle).await;
                return Ok(WizardModalOutcome::Cancelled { signal: None });
            }
            InlineEvent::Exit => {
                ctrl_c_state.disarm_exit();
                close_modal(handle).await;
                return Ok(WizardModalOutcome::Cancelled {
                    signal: Some("exit"),
                });
            }
            InlineEvent::Submit(_) | InlineEvent::QueueSubmit(_) => {
                continue;
            }
            _ => {}
        }
    }
}

async fn close_modal(handle: &InlineHandle) {
    handle.close_modal();
    handle.force_redraw();
    task::yield_now().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
}
