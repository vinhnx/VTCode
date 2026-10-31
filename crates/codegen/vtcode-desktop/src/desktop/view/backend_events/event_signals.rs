//! Derived signals computed from backend events without view state.

use super::*;

/// CLI-equivalent background count: matrix tasks whose projected delegated
/// status is still loading. Mirrors the `matrix_projection_entries` statuses
/// (`Queued`/`Running`/`Waiting`) through `LocalAgentEntry::is_loading`
/// (`queued`/`running`/`waiting`), so both surfaces feed the shared loading
/// model from the same signal.
pub(super) fn matrix_loading_count(snapshot: &vtcode_exec_events::matrix::MatrixSnapshot) -> usize {
    use vtcode_exec_events::matrix::{MatrixLifecycle, MatrixTaskStatus};
    if snapshot.lifecycle == MatrixLifecycle::Cancelled {
        return 0;
    }
    snapshot
        .tasks
        .iter()
        .filter(|task| match task.status {
            MatrixTaskStatus::Queued | MatrixTaskStatus::Assigned | MatrixTaskStatus::Executed => true,
            MatrixTaskStatus::Verified => snapshot.lifecycle != MatrixLifecycle::Succeeded,
            MatrixTaskStatus::Failed
            | MatrixTaskStatus::Interrupted
            | MatrixTaskStatus::TimedOut
            | MatrixTaskStatus::CleanupUncertain => false,
        })
        .count()
}

pub(super) fn item_update_identity(event: &BackendEvent) -> Option<(&str, Option<&str>, &str)> {
    let BackendEvent::ThreadEvent(notification) = event else {
        return None;
    };
    let vtcode_exec_events::ThreadEvent::ItemUpdated(update) = notification.event.event() else {
        return None;
    };
    Some((&notification.session_id, notification.turn_id.as_deref(), &update.item.id))
}

pub(super) fn is_item_update_event(event: &BackendEvent) -> bool {
    item_update_identity(event).is_some()
}

pub(in super::super) fn can_coalesce_item_updates(previous: &BackendEvent, next: &BackendEvent) -> bool {
    matches!(
        (item_update_identity(previous), item_update_identity(next)),
        (Some((previous_session, previous_turn, previous_item)), Some((next_session, next_turn, next_item)))
            if previous_session == next_session && previous_turn == next_turn && previous_item == next_item
    )
}
