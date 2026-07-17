//! Monotonic progress monitoring for long-horizon sessions.
//!
//! Agent capability is only useful if the agent *keeps getting closer to
//! completion over time*. [`ProgressMonitor`] externalizes that invariant into
//! a durable [`ProgressLedger`] persisted via `vtcode-session-store`, so the
//! harness can:
//!
//! - refuse to declare a session complete while tracked milestones are open,
//! - detect stalls (turns with no forward progress) and trigger
//!   compaction → replan → escalation, and
//! - resume a long task with an accurate picture of what is done.
//!
//! Persistence is best-effort: a failed disk write is logged, not fatal, so the
//! live run is never blocked by the progress side-channel.

use std::path::PathBuf;

use tracing::warn;
use vtcode_session_store::progress::{
    Milestone, MilestoneStatus, ProgressLedger, load_progress, save_progress,
};

/// Guard-rail interface isolating the [`ProgressMonitor`] from persistence IO.
///
/// The monitor owns *only* the progress domain logic; every side effect
/// (ledger persistence, memory checkpointing) is delegated through this trait.
/// This keeps the monitor unit-testable with an in-memory sink and prevents the
/// long-horizon progress logic from coupling to the filesystem or to
/// `vtcode-session-store` internals.
pub trait ProgressLedgerSink: Send + Sync {
    /// Persist the authoritative ledger. Best-effort; errors are the sink's
    /// concern (typically logged, not propagated).
    fn persist(&self, ledger: &ProgressLedger);

    /// Flush a human-readable checkpoint (e.g. `memories/progress.md`).
    /// Default: no-op, so sinks that do not support checkpoints opt out cleanly.
    fn checkpoint(&self, _ledger: &ProgressLedger) {}
}

/// A sink that discards everything — for tests and in-memory sessions.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullProgressSink;

impl ProgressLedgerSink for NullProgressSink {
    fn persist(&self, _ledger: &ProgressLedger) {}
}

/// Filesystem-backed sink: persists the ledger via `vtcode-session-store` and
/// checkpoints a markdown summary into the workspace's durable memory.
#[derive(Debug, Clone)]
pub struct SessionProgressSink {
    workspace: PathBuf,
}

impl SessionProgressSink {
    /// Create a sink rooted at `workspace`.
    #[must_use]
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

impl ProgressLedgerSink for SessionProgressSink {
    fn persist(&self, ledger: &ProgressLedger) {
        if let Err(e) = save_progress(&self.workspace, &ledger.session_id, ledger) {
            warn!(session = %ledger.session_id, error = %e, "failed to persist progress ledger");
        }
    }

    fn checkpoint(&self, ledger: &ProgressLedger) {
        let path = self.workspace.join("memories").join("progress.md");
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!(path = %parent.display(), error = %e, "failed to create memories dir");
                return;
            }
        }
        if let Err(e) = std::fs::write(&path, ledger.to_markdown()) {
            warn!(path = %path.display(), error = %e, "failed to write progress memory");
        }
    }
}

/// Observes and persists goal progress for a single session.
///
/// The monitor holds the domain state ([`ProgressLedger`]) and defers all IO to
/// an injected [`ProgressLedgerSink`], so the progress invariant logic is fully
/// testable in isolation.
pub struct ProgressMonitor {
    ledger: ProgressLedger,
    sink: Box<dyn ProgressLedgerSink>,
    /// Consecutive turns with no forward progress. Reset to 0 on advance.
    consecutive_stalls: u32,
}

impl ProgressMonitor {
    /// Create an in-memory monitor (no persistence) for `session_id`.
    #[must_use]
    pub fn new(session_id: &str, goal: &str) -> Self {
        Self::with_sink(
            ProgressLedger::new(session_id, goal),
            Box::new(NullProgressSink),
        )
    }

    /// Create a monitor from an explicit ledger and sink (primary constructor
    /// for testing and custom persistence backends).
    #[must_use]
    pub fn with_sink(ledger: ProgressLedger, sink: Box<dyn ProgressLedgerSink>) -> Self {
        Self {
            ledger,
            sink,
            consecutive_stalls: 0,
        }
    }

    /// Create a monitor bound to a workspace, loading any previously persisted
    /// ledger so a resumed session continues from its real progress state.
    pub fn with_persistence(workspace: PathBuf, session_id: &str, goal: &str) -> Self {
        let ledger = load_progress(&workspace, session_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| ProgressLedger::new(session_id, goal));
        Self::with_sink(ledger, Box::new(SessionProgressSink::new(workspace)))
    }

    /// Borrow the current ledger snapshot.
    #[must_use]
    pub fn ledger(&self) -> &ProgressLedger {
        &self.ledger
    }

    /// Whether the monitor is currently reporting a stall.
    #[must_use]
    pub fn is_stalled(&self) -> bool {
        self.ledger.is_stalled()
    }

    /// Fraction of milestones complete, `0.0..=1.0`.
    #[must_use]
    pub fn completion_ratio(&self) -> f32 {
        self.ledger.completion_ratio()
    }

    /// Whether all tracked milestones are complete (or none are tracked).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.ledger.is_complete()
    }

    /// Update the session goal and persist.
    pub fn set_goal(&mut self, goal: &str) {
        self.ledger.set_goal(goal);
        self.persist();
    }

    /// Replace the milestone set from the live task tracker/plan and persist.
    pub fn set_milestones(&mut self, milestones: Vec<Milestone>) {
        self.ledger.set_milestones(milestones);
        self.persist();
    }

    /// Record that this turn made forward progress (clears any stall).
    pub fn record_advance(&mut self) {
        self.ledger.note_advance();
        self.consecutive_stalls = 0;
        self.persist();
    }

    /// Record that this turn made no forward progress (may set a stall).
    pub fn record_stall(&mut self) {
        self.ledger.note_stall();
        self.consecutive_stalls = self.consecutive_stalls.saturating_add(1);
        self.persist();
    }

    /// Number of consecutive turns with no forward progress.
    /// Used by the context reset logic to decide when to trigger a reset.
    #[must_use]
    pub fn consecutive_stalls(&self) -> u32 {
        self.consecutive_stalls
    }

    fn persist(&self) {
        self.sink.persist(&self.ledger);
    }

    /// Flush a human-readable progress checkpoint through the sink (e.g. to
    /// `memories/progress.md`) so a resumed or forked session can re-ground on
    /// what is actually done without waiting for compaction. This is the
    /// proactive-context-grounding half of long-horizon support (the other half
    /// is the compaction-time [`ProgressLedger`]).
    ///
    /// Best-effort: for the default filesystem sink a write failure is logged,
    /// not fatal, and the [`NullProgressSink`] is a no-op.
    pub fn checkpoint(&self) {
        self.sink.checkpoint(&self.ledger);
    }
}

/// Map a free-form tracker status string onto a [`MilestoneStatus`].
#[must_use]
pub fn milestone_status_from_str(status: &str) -> MilestoneStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "done" | "complete" | "completed" | "pass" | "passed" | "success" => MilestoneStatus::Done,
        "blocked" | "stuck" | "waiting" => MilestoneStatus::Blocked,
        "in_progress" | "in progress" | "active" | "running" => MilestoneStatus::InProgress,
        _ => MilestoneStatus::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Sink that counts persist/checkpoint calls without touching disk.
    #[derive(Default)]
    struct CountingSink {
        persists: Arc<AtomicUsize>,
        checkpoints: Arc<AtomicUsize>,
    }

    impl ProgressLedgerSink for CountingSink {
        fn persist(&self, _ledger: &ProgressLedger) {
            self.persists.fetch_add(1, Ordering::Relaxed);
        }
        fn checkpoint(&self, _ledger: &ProgressLedger) {
            self.checkpoints.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn mutations_persist_through_injected_sink() {
        let persists = Arc::new(AtomicUsize::new(0));
        let checkpoints = Arc::new(AtomicUsize::new(0));
        let sink = CountingSink {
            persists: persists.clone(),
            checkpoints: checkpoints.clone(),
        };
        let mut monitor =
            ProgressMonitor::with_sink(ProgressLedger::new("s1", "goal"), Box::new(sink));

        monitor.set_milestones(vec![Milestone {
            id: "1".into(),
            description: "step".into(),
            status: MilestoneStatus::InProgress,
        }]);
        monitor.record_advance();
        monitor.record_stall();
        monitor.checkpoint();

        // set_milestones + record_advance + record_stall = 3 persists.
        assert_eq!(persists.load(Ordering::Relaxed), 3);
        assert_eq!(checkpoints.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn null_sink_monitor_is_pure_in_memory() {
        let mut monitor = ProgressMonitor::new("s2", "goal");
        assert!(monitor.is_complete()); // no milestones tracked yet
        monitor.set_milestones(vec![Milestone {
            id: "1".into(),
            description: "step".into(),
            status: MilestoneStatus::Pending,
        }]);
        assert!(!monitor.is_complete());
        assert!((monitor.completion_ratio() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn consecutive_stalls_increment_and_reset() {
        let mut monitor = ProgressMonitor::new("s3", "goal");
        assert_eq!(monitor.consecutive_stalls(), 0);

        monitor.record_stall();
        assert_eq!(monitor.consecutive_stalls(), 1);

        monitor.record_stall();
        assert_eq!(monitor.consecutive_stalls(), 2);

        // Advance resets the counter.
        monitor.record_advance();
        assert_eq!(monitor.consecutive_stalls(), 0);

        monitor.record_stall();
        assert_eq!(monitor.consecutive_stalls(), 1);
    }
}
