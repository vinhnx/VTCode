//! Acknowledged live steering: handoff, receipts, and queue ownership.
//!
//! A `turn/steer` request only hands the prompt to the live steering channel.
//! The addressed queue row stays owned by the client until the runtime reports
//! its terminal outcome in a `turn/steer/result` notification. No state lock
//! is held while awaiting the receipt, so requests and notifications keep
//! flowing while delivery is pending.

use super::*;
use crate::agent::runloop::unified::stop_requests;
use anyhow::{Result, bail};
use vtcode_core::core::agent::steering::{FollowUpSteer, SteerOutcome, SteerRejection, SteeringMessage};

fn map_steer_rejection(reason: SteerRejection) -> protocol::TurnSteerRejection {
    match reason {
        SteerRejection::TurnClosing => protocol::TurnSteerRejection::TurnClosing,
        SteerRejection::CancellationPrecedence => protocol::TurnSteerRejection::CancellationPrecedence,
        SteerRejection::QueueFull => protocol::TurnSteerRejection::QueueFull,
        SteerRejection::NoLiveTurn => protocol::TurnSteerRejection::NoLiveTurn,
        SteerRejection::Superseded => protocol::TurnSteerRejection::Superseded,
    }
}

/// Apply a runtime steer receipt outside the request path. The state lock is
/// acquired only for the reconciliation itself, never while awaiting the
/// receipt, so notifications and other requests keep flowing.
async fn resolve_steer_receipt(
    state: std::sync::Arc<Mutex<ServerState>>,
    notifications: mpsc::Sender<protocol::Notification>,
    session_id: &str,
    turn_id: &str,
    receipt_id: &str,
    entry_id: &str,
    outcome: SteerOutcome,
) {
    let (accepted, reason, intent_id) = match outcome {
        SteerOutcome::Accepted { intent_id } => (true, None, intent_id),
        SteerOutcome::Rejected { reason } => (false, Some(map_steer_rejection(reason)), String::new()),
    };
    let (current, queue_status, queue_depth) = {
        let mut state = state.lock().await;
        let current =
            state.session_id.as_deref() == Some(session_id) && state.active_turn_id.as_deref() == Some(turn_id);
        let accepted_now = current && accepted;
        if accepted_now
            && !entry_id.is_empty()
            && let Some(position) = state.queue.iter().position(|queued| queued.entry_id() == entry_id)
        {
            state.queue.remove(position);
        }
        let queue_status = accepted_now.then(|| state.queue_status(session_id));
        (current, queue_status, state.queue.len())
    };
    if let Some(status) = queue_status {
        publish_queue_status_sync(&notifications, &status);
    }
    let accepted = current && accepted;
    let reason = if current {
        reason
    } else {
        Some(protocol::TurnSteerRejection::Superseded)
    };
    let notification = protocol::TurnSteerResultNotification {
        session_id: session_id.to_string(),
        turn_id: turn_id.to_string(),
        receipt_id: receipt_id.to_string(),
        entry_id: entry_id.to_string(),
        accepted,
        reason,
        intent_id,
        queue_depth,
    };
    if let Err(error) = send_notification(
        &notifications,
        protocol::method::TURN_STEER_RESULT,
        serde_json::to_value(notification).unwrap_or(Value::Null),
    )
    .await
    {
        tracing::warn!(target: "vtcode.desktop", %error, "could not publish steer result");
    }
}

pub(super) async fn handle_turn_steer(context: &ServiceContext, params: protocol::TurnSteerParams) -> Result<Value> {
    if params.text.is_empty() {
        bail!("steered prompt is empty");
    }
    // Registration and channel handoff share one lock acquisition. The
    // addressed row stays owned by the client until the runtime's terminal
    // acknowledgment arrives; no lock is held while awaiting it.
    let (steered, receipt_id, queue_depth) = {
        let state = context.state.lock().await;
        if state.session_id.as_deref() != Some(params.session_id.as_str()) {
            bail!("session is not selected");
        }
        let queue_position = params
            .entry_id
            .as_deref()
            .and_then(|entry_id| state.queue.iter().position(|queued| queued.entry_id() == entry_id));
        let queued_input_matches = match params.entry_id.as_deref() {
            Some(_) => queue_position.is_some_and(|position| {
                state
                    .queue
                    .get(position)
                    .is_some_and(|queued| queued.input.text == params.text && !queued.input.has_attachments())
            }),
            None => true,
        };
        let turn_id = state.active_turn_id.clone();
        let steering = state
            .runtime
            .as_ref()
            .and_then(|runtime| stop_requests::host_steering_sender(&runtime.stop));
        let receipt_id = format!("sr-{}", uuid::Uuid::new_v4());
        let steered = if queued_input_matches && turn_id.is_some() {
            match steering {
                Some(sender) => {
                    let (steer, receipt) = FollowUpSteer::channel(params.text.clone());
                    if sender.send(SteeringMessage::FollowUpInput(steer)).is_ok() {
                        let state = context.state.clone();
                        let notifications = context.notifications.clone();
                        let receipt_id_for_task = receipt_id.clone();
                        let session_id = params.session_id.clone();
                        let turn_id_for_task = turn_id.clone().unwrap_or_default();
                        let entry_id = params.entry_id.clone().unwrap_or_default();
                        tokio::spawn(async move {
                            let outcome = receipt
                                .await
                                .unwrap_or(SteerOutcome::Rejected { reason: SteerRejection::Superseded });
                            resolve_steer_receipt(
                                state,
                                notifications,
                                &session_id,
                                &turn_id_for_task,
                                &receipt_id_for_task,
                                &entry_id,
                                outcome,
                            )
                            .await;
                        });
                        true
                    } else {
                        false
                    }
                }
                None => false,
            }
        } else {
            false
        };
        (steered, receipt_id, state.queue.len())
    };
    let result = protocol::TurnSteerResult {
        session_id: params.session_id,
        steered,
        queue_depth,
        pending: steered,
        receipt_id,
    };
    Ok(serde_json::to_value(result)?)
}
