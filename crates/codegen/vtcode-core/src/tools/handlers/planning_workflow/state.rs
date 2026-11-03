//! Shared planning-workflow state across the start/finish tools.
//!
//! This is the only piece of state that spans the tool boundaries; all file
//! I/O and tool wiring live elsewhere (`persistence.rs`, `start.rs`), with
//! workflow exit owned runloop-side (`finish_planning_workflow`). The struct
//! intentionally exposes a narrow, `pub` method surface and keeps its fields
//! private.
//!
//! # State model
//!
//! All mutable fields live in one [`PlanningInner`] behind a single lock, so
//! compound transitions (activate with a file, allocate-if-missing, read the
//! flag and the pointer together) are atomic. The reachable combinations are:
//!
//! | `active` | `plan_file` | Meaning |
//! |----------|-------------|---------|
//! | `false` | `None` | Idle; no plan in progress. |
//! | `false` | `Some` | Deliberate: an execution-mode draft allocated a file
//! | | | location without activating enforcement, or an approved plan was
//! | | | retained across `disable()` for post-planning seeding. |
//! | `true` | `None` | Active; the file is allocated lazily on first persist. |
//! | `true` | `Some` | Normal active planning with a known plan file. |
//!
//! In particular `disable()` preserves the file pointer and baseline; entry
//! paths that need a clean slate clear them explicitly.

use crate::core::agent::types::AgentType;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Immutable point-in-time view of the planning workflow state.
///
/// Returned by [`PlanningWorkflowState::snapshot`] so callers that decide on
/// the flag and the pointer together never observe a torn combination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningSnapshot {
    /// Whether planning enforcement is currently active.
    pub active: bool,
    /// Path to the current plan file (if any).
    pub plan_file: Option<PathBuf>,
    /// Baseline time to require plan updates after initial creation.
    pub baseline: Option<SystemTime>,
}

/// The full mutable state, always read or written under one lock.
#[derive(Debug, Clone, Default)]
struct PlanningInner {
    active: bool,
    plan_file: Option<PathBuf>,
    baseline: Option<SystemTime>,
}

/// Shared state for planning workflow across tools
#[derive(Debug, Clone)]
pub struct PlanningWorkflowState {
    /// Single lock over flag + pointer so compound transitions are atomic.
    /// `parking_lot` keeps every accessor synchronous; critical sections only
    /// clone small values and never block on I/O.
    inner: Arc<RwLock<PlanningInner>>,
    /// Workspace root for plan directory
    workspace_root: PathBuf,
}

impl PlanningWorkflowState {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            inner: Arc::new(RwLock::new(PlanningInner::default())),
            workspace_root,
        }
    }

    /// Check if planning workflow is active
    pub fn is_active(&self) -> bool {
        self.inner.read().active
    }

    /// Enable planning workflow
    pub fn enable(&self) {
        self.inner.write().active = true;
    }

    /// Disable planning workflow.
    ///
    /// Preserves the plan file pointer and baseline: approved plans are
    /// retained for post-planning seeding and execution-mode drafts keep
    /// their allocated location. Callers that need a clean slate clear the
    /// pointer explicitly via [`Self::set_plan_file`] / [`Self::set_plan_baseline`].
    pub fn disable(&self) {
        self.inner.write().active = false;
    }

    /// Atomically set the plan pointer and activate the workflow.
    ///
    /// Replaces the previous set-file/set-baseline/enable sequence, which
    /// could be interleaved with reads from other tasks.
    pub fn activate(&self, plan_file: Option<PathBuf>, baseline: Option<SystemTime>) {
        let mut inner = self.inner.write();
        inner.plan_file = plan_file;
        inner.baseline = baseline;
        inner.active = true;
    }

    /// Atomically replace the plan pointer (file + baseline) together.
    pub fn set_plan_pointer(&self, plan_file: Option<PathBuf>, baseline: Option<SystemTime>) {
        let mut inner = self.inner.write();
        inner.plan_file = plan_file;
        inner.baseline = baseline;
    }

    /// Set the plan pointer only when no file is set; return the winning path.
    ///
    /// The check and the set happen under one lock, so concurrent allocators
    /// agree on a single path instead of each allocating its own.
    #[must_use]
    pub fn set_plan_file_if_missing(&self, plan_file: PathBuf, baseline: SystemTime) -> PathBuf {
        let mut inner = self.inner.write();
        if let Some(existing) = inner.plan_file.clone() {
            return existing;
        }
        inner.plan_file = Some(plan_file.clone());
        inner.baseline = Some(baseline);
        plan_file
    }

    /// Read the flag and the pointer together under one lock.
    #[must_use]
    pub fn snapshot(&self) -> PlanningSnapshot {
        let inner = self.inner.read();
        PlanningSnapshot {
            active: inner.active,
            plan_file: inner.plan_file.clone(),
            baseline: inner.baseline,
        }
    }

    /// Returns the agent type that corresponds to an active planning workflow.
    ///
    /// When planning workflow is active, the effective agent type is `AgentType::Plan`
    /// (the read-only research specialist).  When inactive, returns `None` to signal
    /// the caller should use its own default agent type.
    pub fn effective_agent_type(&self) -> Option<AgentType> {
        if self.is_active() { Some(AgentType::Plan) } else { None }
    }

    /// Get the workspace root path
    pub fn workspace_root(&self) -> Option<PathBuf> {
        if self.workspace_root.as_os_str().is_empty() {
            None
        } else {
            Some(self.workspace_root.clone())
        }
    }

    /// Get the default plans directory path.
    pub fn plans_dir(&self) -> PathBuf {
        if self.workspace_root.as_os_str().is_empty() {
            crate::tools::tool_intent::tmp_plans_dir().join(workspace_slug_for_tmp(&self.workspace_root))
        } else {
            self.workspace_root.join(".vtcode").join("plans")
        }
    }

    /// Set the current plan file
    pub fn set_plan_file(&self, path: Option<PathBuf>) {
        self.inner.write().plan_file = path;
    }

    /// Set the baseline time for plan readiness checks
    pub fn set_plan_baseline(&self, baseline: Option<SystemTime>) {
        self.inner.write().baseline = baseline;
    }

    /// Get the baseline time for plan readiness checks
    pub fn plan_baseline(&self) -> Option<SystemTime> {
        self.inner.read().baseline
    }

    /// Get the current plan file path
    pub fn get_plan_file(&self) -> Option<PathBuf> {
        self.inner.read().plan_file.clone()
    }
}

/// Slugify a workspace root into a filesystem-safe segment for the temp plans dir.
fn workspace_slug_for_tmp(workspace_root: &Path) -> String {
    let fallback = "workspace".to_string();
    let candidate = workspace_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_string())
        .unwrap_or(fallback);
    let sanitized = candidate
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches('-').is_empty() {
        "workspace".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> PlanningWorkflowState {
        PlanningWorkflowState::new(PathBuf::from("/tmp/planning-state-test"))
    }

    #[test]
    fn activate_sets_flag_and_pointer_together() {
        let state = test_state();
        let file = PathBuf::from("/tmp/planning-state-test/plan.md");
        let baseline = SystemTime::UNIX_EPOCH;

        state.activate(Some(file.clone()), Some(baseline));

        assert_eq!(
            state.snapshot(),
            PlanningSnapshot {
                active: true,
                plan_file: Some(file),
                baseline: Some(baseline),
            }
        );
    }

    #[test]
    fn activate_without_pointer_stays_active_with_no_file() {
        let state = test_state();

        state.activate(None, None);

        assert!(state.is_active());
        assert_eq!(state.get_plan_file(), None);
        assert_eq!(state.plan_baseline(), None);
    }

    #[test]
    fn disable_preserves_pointer_for_post_planning_reads() {
        let state = test_state();
        let file = PathBuf::from("/tmp/planning-state-test/approved.md");
        state.activate(Some(file.clone()), Some(SystemTime::UNIX_EPOCH));

        state.disable();

        assert!(!state.is_active());
        assert_eq!(state.get_plan_file(), Some(file));
        assert_eq!(state.plan_baseline(), Some(SystemTime::UNIX_EPOCH));
    }

    #[test]
    fn set_plan_pointer_replaces_file_and_baseline_together() {
        let state = test_state();
        state.activate(Some(PathBuf::from("/tmp/old.md")), Some(SystemTime::UNIX_EPOCH));

        let replacement = PathBuf::from("/tmp/new.md");
        state.set_plan_pointer(Some(replacement.clone()), None);

        assert_eq!(
            state.snapshot(),
            PlanningSnapshot {
                active: true,
                plan_file: Some(replacement),
                baseline: None,
            }
        );
    }

    #[tokio::test]
    async fn concurrent_allocate_if_missing_agrees_on_one_path() {
        let state = test_state();
        let mut handles = Vec::new();
        for index in 0..16u32 {
            let state = state.clone();
            handles.push(tokio::spawn(async move {
                state.set_plan_file_if_missing(
                    PathBuf::from(format!("/tmp/racer-{index}.md")),
                    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(u64::from(index)),
                )
            }));
        }

        let mut winners = Vec::new();
        for handle in handles {
            winners.push(handle.await.expect("allocator task panicked"));
        }

        let first = winners.first().expect("at least one winner");
        assert!(winners.iter().all(|path| path == first), "all racers must agree: {winners:?}");
        assert_eq!(state.get_plan_file(), Some(first.clone()));
        assert!(state.plan_baseline().is_some());
    }

    #[tokio::test]
    async fn snapshots_never_observe_torn_combinations() {
        let first = PlanningSnapshot {
            active: true,
            plan_file: Some(PathBuf::from("/tmp/torn-a.md")),
            baseline: Some(SystemTime::UNIX_EPOCH),
        };
        let second = PlanningSnapshot {
            active: false,
            plan_file: first.plan_file.clone(),
            baseline: first.baseline,
        };

        let state = test_state();
        let writer = state.clone();
        let writer_first = first.clone();
        let writer_handle = tokio::spawn(async move {
            for _ in 0..200 {
                // Both transitions are single critical sections; readers must
                // only ever observe one of the two fully-written states.
                writer.activate(writer_first.plan_file.clone(), writer_first.baseline);
                writer.disable();
            }
        });

        let mut readers = Vec::new();
        for _ in 0..4 {
            let state = state.clone();
            let first = first.clone();
            let second = second.clone();
            readers.push(tokio::spawn(async move {
                for _ in 0..200 {
                    let snapshot = state.snapshot();
                    assert!(snapshot == first || snapshot == second, "torn snapshot observed: {snapshot:?}");
                }
            }));
        }

        writer_handle.await.expect("writer task panicked");
        for reader in readers {
            reader.await.expect("reader task panicked");
        }
    }
}
