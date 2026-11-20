//! Canonical wire `type` values for [`crate::ThreadEvent`].
//!
//! Kept in its own module so the wire vocabulary and its invariant test stay
//! together without pushing `lib.rs` past the 1000-line ceiling.

/// Canonical wire `type` values of [`crate::ThreadEvent`].
///
/// Consumers that *compare* recorded event types by string use these consts;
/// producers that *emit* the name use [`crate::ThreadEvent::wire_name`]. Both
/// resolve to the same spellings, which `wire_name_matches_serialized_type_tag`
/// keeps aligned with the serde tags.
pub mod wire_type {
    pub const MATRIX_UPDATED: &str = "matrix.updated";
    pub const TRACKER_UPDATED: &str = "tracker.updated";
    pub const GOAL_UPDATED: &str = "goal.updated";
    pub const THREAD_STARTED: &str = "thread.started";
    pub const THREAD_COMPLETED: &str = "thread.completed";
    pub const THREAD_COMPACT_BOUNDARY: &str = "thread.compact_boundary";
    pub const CONTEXT_RESET: &str = "context.reset";
    pub const TURN_STARTED: &str = "turn.started";
    pub const TURN_COMPLETED: &str = "turn.completed";
    pub const TURN_FAILED: &str = "turn.failed";
    pub const TURN_BLOCKED: &str = "turn.blocked";
    pub const ITEM_STARTED: &str = "item.started";
    pub const ITEM_UPDATED: &str = "item.updated";
    pub const ITEM_COMPLETED: &str = "item.completed";
    pub const APP_SANDBOX_DENIED: &str = "app.sandbox_denied";
    pub const PERMISSION_REQUESTED: &str = "permission.requested";
    pub const PERMISSION_RESOLVED: &str = "permission.resolved";
    pub const INTERJECTED: &str = "interjected";
    pub const PLAN_DELTA: &str = "plan.delta";
    pub const PLAN_APPROVAL_REQUESTED: &str = "plan.approval.requested";
    pub const PLAN_APPROVAL_RESOLVED: &str = "plan.approval.resolved";
    pub const ERROR: &str = "error";
    /// `Unknown` is a serde catch-all with no serde rename, so its serialized
    /// type tag is the variant name — capital U. Keep this aligned with
    /// `wire_name_matches_serialized_type_tag`.
    pub const UNKNOWN: &str = "Unknown";
}

impl crate::ThreadEvent {
    /// Canonical wire `type` value for this event.
    ///
    /// Consumers that record, filter, or compare event types by string (the
    /// share-log timeline, log viewer, and OTel emitter) reference this
    /// instead of re-spelling a `#[serde(rename)]` at their own call site.
    #[must_use]
    pub const fn wire_name(&self) -> &'static str {
        use crate::ThreadEvent as E;
        use wire_type as w;
        match self {
            E::MatrixUpdated(_) => w::MATRIX_UPDATED,
            E::TrackerUpdated(_) => w::TRACKER_UPDATED,
            E::GoalUpdated(_) => w::GOAL_UPDATED,
            E::ThreadStarted(_) => w::THREAD_STARTED,
            E::ThreadCompleted(_) => w::THREAD_COMPLETED,
            E::ThreadCompactBoundary(_) => w::THREAD_COMPACT_BOUNDARY,
            E::ContextReset(_) => w::CONTEXT_RESET,
            E::TurnStarted(_) => w::TURN_STARTED,
            E::TurnCompleted(_) => w::TURN_COMPLETED,
            E::TurnFailed(_) => w::TURN_FAILED,
            E::TurnBlocked(_) => w::TURN_BLOCKED,
            E::ItemStarted(_) => w::ITEM_STARTED,
            E::ItemUpdated(_) => w::ITEM_UPDATED,
            E::ItemCompleted(_) => w::ITEM_COMPLETED,
            E::AppSandboxDenied(_) => w::APP_SANDBOX_DENIED,
            E::PermissionRequested(_) => w::PERMISSION_REQUESTED,
            E::PermissionResolved(_) => w::PERMISSION_RESOLVED,
            E::Interjected(_) => w::INTERJECTED,
            E::PlanDelta(_) => w::PLAN_DELTA,
            E::PlanApprovalRequested(_) => w::PLAN_APPROVAL_REQUESTED,
            E::PlanApprovalResolved(_) => w::PLAN_APPROVAL_RESOLVED,
            E::Error(_) => w::ERROR,
            E::Unknown => w::UNKNOWN,
        }
    }
}
