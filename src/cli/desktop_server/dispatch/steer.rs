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

/// Identifies the submitted steer across turn completion and session replacement.
struct PendingSteer {
    session_id: String,
    turn_id: String,
    receipt_id: String,
    entry_id: String,
}

/// Resolve the receipt before any queued turn can take the reserved prompt.
async fn resolve_steer_receipt(
    state: std::sync::Arc<Mutex<ServerState>>,
    notifications: mpsc::Sender<protocol::Notification>,
    turn_finished: std::sync::Arc<Notify>,
    PendingSteer { session_id, turn_id, receipt_id, entry_id }: PendingSteer,
    outcome: SteerOutcome,
) {
    let (accepted, reason, intent_id) = match outcome {
        SteerOutcome::Accepted { intent_id } => (true, None, intent_id),
        SteerOutcome::Rejected { reason } => (false, Some(map_steer_rejection(reason)), String::new()),
    };
    // Reserve notification capacity before taking the state lock. Applying
    // the result and publishing it together keeps later queue snapshots from
    // exposing the removal before the client learns which entry it was.
    let permit = notifications.reserve().await.ok();
    let drained = {
        let mut state = state.lock().await;
        let same_session = state.session_id.as_deref() == Some(session_id.as_str());
        let reserved = same_session && state.queued_steer_entry_id.as_deref() == Some(entry_id.as_str());
        let current = same_session
            && (state.active_turn_id.as_deref() == Some(turn_id.as_str())
                || (reserved && state.active_turn_id.is_none()));
        let accepted = current && accepted;
        if accepted
            && reserved
            && let Some(position) = state.queue.iter().position(|queued| queued.entry_id() == entry_id)
        {
            state.queue.remove(position);
        }
        let notification = protocol::TurnSteerResultNotification {
            session_id: session_id.clone(),
            turn_id,
            receipt_id,
            entry_id,
            accepted,
            reason: if current {
                reason
            } else {
                Some(protocol::TurnSteerRejection::Superseded)
            },
            intent_id,
            queue_depth: state.queue.len(),
        };
        if let Some(permit) = permit {
            permit.send(protocol::Notification {
                jsonrpc: "2.0".into(),
                method: protocol::method::TURN_STEER_RESULT.into(),
                params: serde_json::to_value(notification).unwrap_or(Value::Null),
            });
        }
        if accepted && reserved {
            publish_queue_status_sync(&notifications, &state.queue_status(&session_id));
        }
        if reserved {
            state.queued_steer_entry_id = None;
            state.pop_next_queued_turn()
        } else {
            None
        }
    };
    if let Some(drained) = drained
        && let Err(error) = dispatch_drained_turn(&state, &notifications, &turn_finished, &session_id, drained).await
    {
        tracing::warn!(target: "vtcode.desktop", %error, "could not resume queue after steering");
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
        let mut state = context.state.lock().await;
        if state.session_id.as_deref() != Some(params.session_id.as_str()) {
            bail!("session is not selected");
        }
        let queue_position = params
            .entry_id
            .as_deref()
            .and_then(|entry_id| state.queue.iter().position(|queued| queued.entry_id() == entry_id));
        let queued_input_matches = match params.entry_id.as_deref() {
            Some(_) => {
                state.queued_steer_entry_id.is_none()
                    && queue_position.is_some_and(|position| {
                        state
                            .queue
                            .get(position)
                            .is_some_and(|queued| queued.input.text == params.text && !queued.input.has_attachments())
                    })
            }
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
                        if params.entry_id.is_some() {
                            state.queued_steer_entry_id.clone_from(&params.entry_id);
                        }
                        let pending = PendingSteer {
                            session_id: params.session_id.clone(),
                            turn_id: turn_id.clone().unwrap_or_default(),
                            receipt_id: receipt_id.clone(),
                            entry_id: params.entry_id.clone().unwrap_or_default(),
                        };
                        let state = context.state.clone();
                        let notifications = context.notifications.clone();
                        let turn_finished = context.turn_finished.clone();
                        tokio::spawn(async move {
                            let outcome = receipt
                                .await
                                .unwrap_or(SteerOutcome::Rejected { reason: SteerRejection::Superseded });
                            resolve_steer_receipt(state, notifications, turn_finished, pending, outcome).await;
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
