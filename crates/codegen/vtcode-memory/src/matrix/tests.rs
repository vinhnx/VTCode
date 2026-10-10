use super::*;
use crate::SessionEventLog;
use vtcode_exec_events::{TurnCompletedEvent, TurnStartedEvent, Usage};

fn task(id: &str, access: WorkspaceAccess) -> MatrixTaskSpec {
    MatrixTaskSpec {
        id: id.into(),
        instructions: format!("perform {id}"),
        dependencies: vec![],
        workspace: ".".into(),
        access,
        checks: vec![format!("check-{id}")],
        resources: BTreeMap::new(),
        timeout_secs: 30,
        replay_safe: false,
        inputs: vec![],
    }
}
fn state(tasks: Vec<MatrixTaskSpec>, resources: BTreeMap<String, u32>) -> MatrixState {
    let root = tempfile::tempdir().unwrap();
    MatrixState::create(MatrixSpec { id: "matrix-one".into(), tasks, resources }, root.path()).unwrap()
}
fn finish(state: &mut MatrixState, assignment: &MatrixAssignment) {
    state
        .report(&assignment.attempt_id, &assignment.worker_id, MatrixOutcome::Success, vec![], true)
        .unwrap();
}

#[test]
fn matrix_validation_rejects_cycles_missing_checks_and_escape() {
    let root = tempfile::tempdir().unwrap();
    let mut a = task("a", WorkspaceAccess::Read);
    let mut b = task("b", WorkspaceAccess::Read);
    a.dependencies.push("b".into());
    b.dependencies.push("a".into());
    let mut spec = MatrixSpec {
        id: "x".into(),
        tasks: vec![a, b],
        resources: BTreeMap::new(),
    };
    assert!(MatrixState::create(spec.clone(), root.path()).is_err());
    spec.tasks[0].dependencies.clear();
    spec.tasks[1].dependencies.clear();
    assert!(MatrixState::create(spec.clone(), root.path()).is_ok());
    spec.tasks[1].checks.clear();
    assert!(MatrixState::create(spec.clone(), root.path()).is_err());
    spec.tasks[1].checks.push("ok".into());
    spec.tasks[0].workspace = "../".into();
    assert!(MatrixState::create(spec, root.path()).is_err());
}

#[test]
fn matrix_admission_cap_and_atomic_resources_skip_blocked_tasks() {
    let mut a = task("a", WorkspaceAccess::Read);
    let mut b = task("b", WorkspaceAccess::Read);
    let mut c = task("c", WorkspaceAccess::Read);
    a.resources.insert("cpu".into(), 2);
    b.resources.insert("cpu".into(), 1);
    b.resources.insert("device".into(), 1);
    c.resources.insert("device".into(), 1);
    let mut state = state(
        vec![a, b, c, task("d", WorkspaceAccess::Read)],
        BTreeMap::from([("cpu".into(), 2), ("device".into(), 1)]),
    );
    state.start().unwrap();
    let assignments = state.reserve_ready(3).unwrap();
    assert_eq!(assignments.iter().map(|a| a.task_id.as_str()).collect::<Vec<_>>(), vec!["a", "c", "d"]);
    assert!(state.reserve_ready(3).unwrap().is_empty());
    state.rollback_launch(&assignments[0].attempt_id).unwrap();
    assert_eq!(state.reserve_ready(3).unwrap().len(), 1);
}

#[test]
fn matrix_writes_are_exclusive_and_dependencies_use_execution() {
    let a = task("reader", WorkspaceAccess::Read);
    let mut b = task("writer", WorkspaceAccess::Write);
    b.dependencies.push("reader".into());
    let mut state = state(vec![a, b, task("independent", WorkspaceAccess::Read)], BTreeMap::new());
    state.start().unwrap();
    let first = state.reserve_ready(5).unwrap();
    assert_eq!(first.iter().map(|a| a.task_id.as_str()).collect::<Vec<_>>(), vec!["reader", "independent"]);
    finish(&mut state, &first[0]);
    assert!(state.reserve_ready(5).unwrap().is_empty());
    finish(&mut state, &first[1]);
    let writer = state.reserve_ready(5).unwrap();
    assert_eq!(writer[0].task_id, "writer");
    assert_eq!(writer.len(), 1);
}

#[test]
fn matrix_verification_requires_owned_complete_current_generation_evidence() {
    let mut state = state(vec![task("a", WorkspaceAccess::Read)], BTreeMap::new());
    state.start().unwrap();
    let execution = state.reserve_ready(3).unwrap().remove(0);
    finish(&mut state, &execution);
    state.begin_verification("generation-1".into()).unwrap();
    let check = state.reserve_ready(3).unwrap().remove(0);
    assert!(
        state
            .report(&check.attempt_id, &execution.worker_id, MatrixOutcome::Success, vec![], true)
            .is_err()
    );
    assert!(
        state
            .report(&check.attempt_id, &check.worker_id, MatrixOutcome::Success, vec![], true)
            .is_err()
    );
    let mut evidence = MatrixCommandEvidence {
        command: "check-a".into(),
        attempt_id: check.attempt_id.clone(),
        worker_id: check.worker_id.clone(),
        generation: "old".into(),
        event_id: "owned-event".into(),
        exit_code: Some(0),
        cancelled: false,
    };
    assert!(
        state
            .report(&check.attempt_id, &check.worker_id, MatrixOutcome::Success, vec![evidence.clone()], true)
            .is_err()
    );
    evidence.generation = "generation-1".into();
    evidence.cancelled = true;
    assert!(
        state
            .report(&check.attempt_id, &check.worker_id, MatrixOutcome::Success, vec![evidence.clone()], true)
            .is_err()
    );
    evidence.cancelled = false;
    state
        .report(&check.attempt_id, &check.worker_id, MatrixOutcome::Success, vec![evidence.clone()], true)
        .unwrap();
    assert_eq!(state.snapshot().lifecycle, MatrixLifecycle::Verifying);
    assert!(state.finalize_verification("wrong").is_err());
    state.finalize_verification("generation-1").unwrap();
    assert_eq!(state.snapshot().lifecycle, MatrixLifecycle::Succeeded);
    assert!(
        state
            .report(&check.attempt_id, &check.worker_id, MatrixOutcome::Success, vec![evidence], true)
            .is_err()
    );
}

#[test]
fn matrix_retry_once_cleanup_uncertainty_and_terminal_cancel() {
    let mut safe = task("safe", WorkspaceAccess::Read);
    safe.replay_safe = true;
    let mut state = state(vec![safe], BTreeMap::new());
    state.start().unwrap();
    let first = state.reserve_ready(3).unwrap().remove(0);
    state
        .report(&first.attempt_id, &first.worker_id, MatrixOutcome::TimedOut, vec![], true)
        .unwrap();
    let second = state.reserve_ready(3).unwrap().remove(0);
    state
        .report(&second.attempt_id, &second.worker_id, MatrixOutcome::Interrupted, vec![], false)
        .unwrap();
    assert_eq!(state.active_assignments().len(), 1);
    assert!(state.retry("safe").is_err());
    state.confirm_cleanup(&second.attempt_id, &second.worker_id).unwrap();
    assert_eq!(state.snapshot().tasks[0].automatic_retries, 1);
    state.retry("safe").unwrap();
    state.cancel().unwrap();
    assert!(state.reserve_ready(3).unwrap().is_empty());
    assert!(state.retry("safe").is_err());
    assert!(state.resume().is_err());
}

#[test]
fn matrix_pause_allows_completion_and_invalidated_generation_reruns_all() {
    let mut state = state(vec![task("a", WorkspaceAccess::Read), task("b", WorkspaceAccess::Read)], BTreeMap::new());
    state.start().unwrap();
    let first = state.reserve_ready(1).unwrap().remove(0);
    state.pause().unwrap();
    finish(&mut state, &first);
    assert!(state.reserve_ready(3).unwrap().is_empty());
    state.resume().unwrap();
    let second = state.reserve_ready(1).unwrap().remove(0);
    finish(&mut state, &second);
    state.begin_verification("one".into()).unwrap();
    state.invalidate_verification("two".into()).unwrap();
    assert_eq!(state.reserve_ready(5).unwrap().len(), 2);
}

#[test]
fn matrix_replay_cap_retains_checkpoint_and_turn_offsets() {
    let root = tempfile::tempdir().unwrap();
    let log = SessionEventLog::open(root.path(), "cap", 3).unwrap();
    let mut state = state(vec![task("a", WorkspaceAccess::Read)], BTreeMap::new());
    log.append(&state.event()).unwrap();
    state.start().unwrap();
    log.append(&state.event()).unwrap();
    for _ in 0..4 {
        log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default())).unwrap();
        log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
            usage: Usage::default(),
            completed_at: None,
            in_progress_exec_sessions: vec![],
        }))
        .unwrap();
    }
    let mut events = Vec::new();
    log.visit_snapshot(|_, line| {
        if let Ok(event) = serde_json::from_slice::<vtcode_exec_events::VersionedThreadEvent>(line) {
            events.push(event.into_event());
        }
    })
    .unwrap();
    let replayed = replay(events).unwrap();
    assert_eq!(replayed["matrix-one"].snapshot(), state.snapshot());
    assert_eq!(log.reconstruct_turn(4).unwrap().len(), 2);
}

#[cfg(unix)]
#[test]
fn matrix_rejects_symlink_workspace_escape() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(other.path(), root.path().join("escape")).unwrap();
    let mut a = task("a", WorkspaceAccess::Read);
    a.workspace = "escape".into();
    assert!(
        MatrixState::create(
            MatrixSpec {
                id: "escape".into(),
                tasks: vec![a],
                resources: BTreeMap::new()
            },
            root.path()
        )
        .is_err()
    );
}

#[test]
fn matrix_recovery_distinguishes_prepared_from_ambiguous_launch() {
    let mut state = state(
        vec![
            task("prepared", WorkspaceAccess::Read),
            task("launched", WorkspaceAccess::Read),
        ],
        BTreeMap::new(),
    );
    state.start().unwrap();
    let assigned = state.reserve_ready(3).unwrap();
    state.mark_launch_requested(&assigned[1].attempt_id).unwrap();
    let mut restored = MatrixState::from_snapshot(state.snapshot().clone()).unwrap();
    restored.recover().unwrap();
    assert_eq!(restored.snapshot().tasks[0].status, MatrixTaskStatus::Queued);
    assert_eq!(restored.snapshot().tasks[1].status, MatrixTaskStatus::CleanupUncertain);
    assert_eq!(restored.snapshot().lifecycle, MatrixLifecycle::Blocked);
    assert_eq!(restored.active_assignments().len(), 1);
    assert!(restored.reserve_ready(3).unwrap().is_empty());
    assert!(restored.retry("launched").is_err());
}

#[test]
fn matrix_replay_rejects_changed_specs_duplicate_revisions_and_false_success() {
    let mut state = state(vec![task("a", WorkspaceAccess::Read)], BTreeMap::new());
    let created = state.event();
    state.start().unwrap();
    assert!(replay(vec![created.clone(), state.event()]).is_ok());
    assert!(replay(vec![created.clone(), created.clone()]).is_err());
    let mut changed = state.snapshot().clone();
    changed.spec.tasks[0].instructions = "other task".into();
    assert!(replay(vec![created, ThreadEvent::MatrixUpdated(Box::new(changed))]).is_err());
    let mut false_success = state.snapshot().clone();
    false_success.lifecycle = MatrixLifecycle::Succeeded;
    false_success.tasks[0].status = MatrixTaskStatus::Verified;
    assert!(MatrixState::from_snapshot(false_success).is_err());
}

#[test]
fn matrix_worker_cap_five_and_unique_runtime_identities() {
    let mut state = state(
        (0..8)
            .map(|index| task(&format!("task-{index}"), WorkspaceAccess::Read))
            .collect(),
        BTreeMap::new(),
    );
    state.start().unwrap();
    let assigned = state.reserve_ready(usize::MAX).unwrap();
    assert_eq!(assigned.len(), 5);
    assert_eq!(assigned.iter().map(|a| &a.attempt_id).collect::<BTreeSet<_>>().len(), 5);
    assert_eq!(assigned.iter().map(|a| &a.worker_id).collect::<BTreeSet<_>>().len(), 5);
    assert!(state.reserve_ready(usize::MAX).unwrap().is_empty());
}

#[test]
fn matrix_denied_unsafe_and_failed_automatic_retries() {
    for (safe, outcome) in [
        (false, MatrixOutcome::Interrupted),
        (true, MatrixOutcome::Failed),
        (true, MatrixOutcome::PermissionDenied),
        (true, MatrixOutcome::BudgetExhausted),
    ] {
        let mut spec = task("a", WorkspaceAccess::Read);
        spec.replay_safe = safe;
        let mut state = state(vec![spec], BTreeMap::new());
        state.start().unwrap();
        let assigned = state.reserve_ready(3).unwrap().remove(0);
        state
            .report(&assigned.attempt_id, &assigned.worker_id, outcome, vec![], true)
            .unwrap();
        assert_eq!(state.snapshot().lifecycle, MatrixLifecycle::Blocked);
        assert_eq!(state.snapshot().tasks[0].automatic_retries, 0);
        assert!(state.reserve_ready(3).unwrap().is_empty());
        state.retry("a").unwrap();
        assert_eq!(state.reserve_ready(3).unwrap().len(), 1);
    }
}

#[test]
fn matrix_cap_rewrite_prefers_newer_retained_checkpoint() {
    let root = tempfile::tempdir().unwrap();
    let log = SessionEventLog::open(root.path(), "newer", 4).unwrap();
    let mut state = state(vec![task("a", WorkspaceAccess::Read)], BTreeMap::new());
    log.append(&state.event()).unwrap();
    log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default())).unwrap();
    state.start().unwrap();
    log.append(&state.event()).unwrap();
    log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
        usage: Usage::default(),
        completed_at: None,
        in_progress_exec_sessions: vec![],
    }))
    .unwrap();
    log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default())).unwrap();
    log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
        usage: Usage::default(),
        completed_at: None,
        in_progress_exec_sessions: vec![],
    }))
    .unwrap();
    log.append(&ThreadEvent::TurnStarted(TurnStartedEvent::default())).unwrap();
    state.pause().unwrap();
    log.append(&state.event()).unwrap();
    log.append(&ThreadEvent::TurnCompleted(TurnCompletedEvent {
        usage: Usage::default(),
        completed_at: None,
        in_progress_exec_sessions: vec![],
    }))
    .unwrap();
    let mut events = Vec::new();
    log.visit_snapshot(|_, line| {
        if let Ok(event) = serde_json::from_slice::<vtcode_exec_events::VersionedThreadEvent>(line) {
            events.push(event.into_event());
        }
    })
    .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ThreadEvent::MatrixUpdated(_)))
            .count(),
        1
    );
    assert_eq!(replay(events).unwrap()["matrix-one"].snapshot(), state.snapshot());
    assert_eq!(log.reconstruct_turn(3).unwrap().len(), 3);
}

#[test]
fn matrix_generation_change_keeps_paused_dispatch_stopped() {
    let mut state = state(vec![task("a", WorkspaceAccess::Read)], BTreeMap::new());
    state.start().unwrap();
    let execution = state.reserve_ready(3).unwrap().remove(0);
    finish(&mut state, &execution);
    state.begin_verification("before-edit".into()).unwrap();
    state.pause().unwrap();
    state.invalidate_verification("after-edit".into()).unwrap();
    assert_eq!(state.snapshot().lifecycle, MatrixLifecycle::Paused);
    assert!(state.reserve_ready(3).unwrap().is_empty());
    state.resume().unwrap();
    assert_eq!(state.reserve_ready(3).unwrap().len(), 1);
}

#[test]
fn matrix_late_owned_cleanup_permits_one_safe_retry() {
    let mut safe = task("safe", WorkspaceAccess::Read);
    safe.replay_safe = true;
    let mut state = state(vec![safe], BTreeMap::new());
    state.start().unwrap();
    let first = state.reserve_ready(3).unwrap().remove(0);
    state
        .report(&first.attempt_id, &first.worker_id, MatrixOutcome::TimedOut, vec![], false)
        .unwrap();
    assert!(state.reserve_ready(3).unwrap().is_empty());
    assert!(state.confirm_cleanup(&first.attempt_id, "wrong-owner").is_err());
    state.confirm_cleanup(&first.attempt_id, &first.worker_id).unwrap();
    assert_eq!(state.snapshot().tasks[0].automatic_retries, 1);
    let second = state.reserve_ready(3).unwrap().remove(0);
    state
        .report(&second.attempt_id, &second.worker_id, MatrixOutcome::TimedOut, vec![], true)
        .unwrap();
    assert_eq!(state.snapshot().lifecycle, MatrixLifecycle::Blocked);
    assert!(state.reserve_ready(3).unwrap().is_empty());
}

#[test]
fn matrix_retry_completion_preserves_other_coordinator_decisions() {
    let mut state = state(vec![task("a", WorkspaceAccess::Read), task("b", WorkspaceAccess::Read)], BTreeMap::new());
    state.start().unwrap();
    let first = state.reserve_ready(3).unwrap();
    for assignment in &first {
        state
            .report(&assignment.attempt_id, &assignment.worker_id, MatrixOutcome::Failed, vec![], true)
            .unwrap();
    }
    state.retry("a").unwrap();
    let retry = state.reserve_ready(3).unwrap().remove(0);
    finish(&mut state, &retry);
    assert_eq!(state.snapshot().lifecycle, MatrixLifecycle::Blocked);
    assert!(state.reserve_ready(3).unwrap().is_empty());
    state.retry("b").unwrap();
    assert_eq!(state.reserve_ready(3).unwrap().len(), 1);
}

#[test]
fn matrix_failed_checks_retry_execution_and_permission_budget_remain_blocked() {
    for outcome in [
        MatrixOutcome::Failed,
        MatrixOutcome::PermissionDenied,
        MatrixOutcome::BudgetExhausted,
    ] {
        let mut state = state(vec![task("repair", WorkspaceAccess::Write)], BTreeMap::new());
        state.start().unwrap();
        let execution = state.reserve_ready(3).unwrap().remove(0);
        finish(&mut state, &execution);
        state.begin_verification("before".into()).unwrap();
        let verification = state.reserve_ready(3).unwrap().remove(0);
        state
            .report(&verification.attempt_id, &verification.worker_id, outcome, vec![], true)
            .unwrap();
        assert!(state.invalidate_verification("changed".into()).is_err());
        assert!(state.reserve_ready(3).unwrap().is_empty());
        state.retry("repair").unwrap();
        let repair = state.reserve_ready(3).unwrap().remove(0);
        assert_eq!(repair.phase, MatrixPhase::Execute);
        assert!(repair.generation.is_none());
    }
}

#[test]
fn matrix_replay_rejects_overcommitted_leases_and_missing_attempts() {
    let mut a = task("a", WorkspaceAccess::Read);
    let mut b = task("b", WorkspaceAccess::Read);
    a.resources.insert("device".into(), 1);
    b.resources.insert("device".into(), 1);
    let mut state = state(vec![a, b], BTreeMap::from([("device".into(), 2)]));
    state.start().unwrap();
    assert_eq!(state.reserve_ready(3).unwrap().len(), 2);
    let mut snapshot = state.snapshot().clone();
    snapshot.spec.resources.insert("device".into(), 1);
    assert!(MatrixState::from_snapshot(snapshot).is_err());
    let mut snapshot = state.snapshot().clone();
    snapshot.spec.tasks[0].access = WorkspaceAccess::Write;
    assert!(MatrixState::from_snapshot(snapshot).is_err());
    let mut snapshot = state.snapshot().clone();
    snapshot.tasks[0].attempts.clear();
    assert!(MatrixState::from_snapshot(snapshot).is_err());
}

#[test]
fn matrix_resource_quantity_sum_does_not_overflow_capacity() {
    let mut large = task("large", WorkspaceAccess::Read);
    large.resources.insert("pool".into(), u32::MAX);
    let mut small = task("small", WorkspaceAccess::Read);
    small.resources.insert("pool".into(), 1);
    let mut state = state(vec![large, small], BTreeMap::from([("pool".into(), u32::MAX)]));
    state.start().unwrap();
    let ready = state.reserve_ready(3).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].task_id, "large");
    finish(&mut state, &ready[0]);
    assert_eq!(state.reserve_ready(3).unwrap()[0].task_id, "small");
}
