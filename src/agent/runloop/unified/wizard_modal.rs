use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;

use vtcode_tui::app::{
    InlineHandle, InlineListSearchConfig, InlineListSelection, InlineSession, TransientRequest,
    TransientSubmission, WizardModalMode, WizardOverlayRequest, WizardStep,
};

use super::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use super::state::CtrlCState;

pub(crate) enum WizardModalOutcome {
    Submitted(Vec<InlineListSelection>),
    Cancelled { signal: Option<&'static str> },
}

pub(crate) async fn show_wizard_modal_and_wait(
    handle: &InlineHandle,
    session: &mut InlineSession,
    title: String,
    steps: Vec<WizardStep>,
    current_step: usize,
    search: Option<InlineListSearchConfig>,
    mode: WizardModalMode,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<WizardModalOutcome> {
    let outcome = show_overlay_and_wait(
        handle,
        session,
        TransientRequest::Wizard(WizardOverlayRequest {
            title,
            steps,
            current_step,
            search,
            mode,
        }),
        ctrl_c_state,
        ctrl_c_notify,
        |submission| match submission {
            TransientSubmission::Wizard(selections) => Some(selections),
            _ => None,
        },
    )
    .await?;

    Ok(match outcome {
        OverlayWaitOutcome::Submitted(selections) => WizardModalOutcome::Submitted(selections),
        OverlayWaitOutcome::Cancelled => WizardModalOutcome::Cancelled { signal: None },
        OverlayWaitOutcome::Interrupted => WizardModalOutcome::Cancelled {
            signal: Some("cancel"),
        },
        OverlayWaitOutcome::Exit => WizardModalOutcome::Cancelled {
            signal: Some("exit"),
        },
    })
}
