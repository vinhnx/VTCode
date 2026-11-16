//! Quiet `/goal` execution for desktop clients.
//!
//! Typed slash commands submitted as turns would wedge: a non-turn slash
//! produces no terminal turn event, so the client's pending turn never
//! resolves. `goal/execute` instead applies the shared runloop core directly
//! to the published tracker and returns render lines plus the fresh snapshot;
//! a `submit` prompt starts a real turn when set.

use super::*;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_desktop_protocol::slash_command::{GoalSlashCommandParseError, parse_goal_slash_command};

use crate::agent::runloop::unified::stop_requests;
use crate::agent::runloop::unified::turn::session::slash_commands::{GoalSeed, apply_goal_command};

/// Identity bound for session ids, mirroring turn undo.
const MAX_ID_BYTES: usize = 256;
/// Argument bound after `/goal`.
const MAX_GOAL_ARGS_BYTES: usize = 8 * 1024;

pub(super) async fn handle_goal_execute(context: &ServiceContext, request: &Request) -> Result<(Value, bool)> {
    let params: protocol::GoalExecuteParams = parse_params(&request.params)?;
    if params.session_id.is_empty() || params.session_id.len() > MAX_ID_BYTES {
        bail!("invalid goal command identity");
    }
    if params.args.len() > MAX_GOAL_ARGS_BYTES {
        bail!("goal command arguments are too long");
    }
    let (tracker, emitter, turn_active, steering) = {
        let state = context.state.lock().await;
        if state.session_id.as_deref() != Some(params.session_id.as_str()) {
            bail!("goal commands are only available for the selected session");
        }
        let runtime = state
            .runtime
            .as_ref()
            .ok_or_else(|| anyhow!("selected session is not running"))?;
        let tracker =
            stop_requests::host_goal_tracker(&runtime.stop).ok_or_else(|| anyhow!("session is still starting"))?;
        (
            tracker,
            stop_requests::host_goal_emitter(&runtime.stop),
            state.active_turn_id.is_some(),
            stop_requests::host_steering_sender(&runtime.stop),
        )
    };
    let command = match parse_goal_slash_command(&params.args) {
        Ok(command) => command,
        Err(GoalSlashCommandParseError::InvalidBudget) => {
            bail!("invalid budget: expected /goal budget [<tokens>|0 to clear]");
        }
    };
    let now = chrono::Utc::now();
    let id = format!("goal-{}", now.timestamp_millis());
    let created_at = now.to_rfc3339();
    let seed = GoalSeed { id: &id, created_at: &created_at };

    let mut guard = tracker.lock().await;
    // Dispatch has no session-stats view; the runloop's turn-boundary
    // accounting owns token attribution, so the baseline starts at zero and
    // the first observation records the session total.
    let effect = apply_goal_command(&mut guard, &command, 0, &seed);
    if effect.mutated {
        guard.persist().context("goal snapshot persist failed")?;
        if let Some(emitter) = &emitter
            && let Err(err) = emitter.emit(guard.to_goal_updated_event())
        {
            tracing::warn!("goal event emit failed: {err:#}");
        }
    }
    let snapshot = guard.goal_snapshot();
    drop(guard);
    let lines = effect.lines;

    // A follow-up prompt while a turn runs steers the live turn instead of
    // queueing behind it, so the goal takes effect immediately and no phantom
    // queued turn appears. Without a steering channel, fall back to submit.
    let submit = match effect.submit {
        Some(prompt) if turn_active => match steering {
            Some(sender) => match sender.send(SteeringMessage::FollowUpInput(prompt.clone())) {
                Ok(()) => None,
                Err(_) => Some(prompt),
            },
            None => Some(prompt),
        },
        submit => submit,
    };

    let result = protocol::GoalExecuteResult {
        session_id: params.session_id,
        lines: lines.into_iter().map(|(_, line)| line).collect(),
        mutated: effect.mutated,
        submit,
        goal: snapshot,
    };
    Ok((serde_json::to_value(result)?, false))
}
