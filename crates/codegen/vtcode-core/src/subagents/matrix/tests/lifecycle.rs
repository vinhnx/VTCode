use super::*;

#[tokio::test]
async fn matrix_create_validates_requested_identity_before_persistence() {
    for (requested_id, accepted) in [(Some("other"), false), (Some("defined"), true), (None, true)] {
        let root = tempfile::tempdir().unwrap();
        let sink = SessionStoreSink::open(root.path(), "create-identity").await.unwrap();
        let controller = controller(root.path(), &sink).await;
        let mut args = json!({"action":"create", "spec": {
            "id":"defined", "tasks":[task("read", WorkspaceAccess::Read)]
        }});
        if let Some(id) = requested_id {
            args["matrix_id"] = json!(id);
        }
        let result = controller.matrix_control(args).await;
        assert_eq!(result.is_ok(), accepted);
        let snapshot = controller.matrix_snapshot().await;
        let persisted = (sink.matrix_persistence().load)().await.unwrap();
        if accepted {
            let snapshot = snapshot.unwrap();
            assert_eq!(snapshot.spec.id, "defined");
            assert_eq!(snapshot.lifecycle, MatrixLifecycle::Created);
            assert_eq!(persisted, vec![snapshot]);
        } else {
            assert!(snapshot.is_none(), "rejected creation must not install a runtime matrix");
            assert!(persisted.is_empty(), "rejected creation must not persist a checkpoint");
        }
        assert!(!controller.matrix_is_executing());
        assert!(!controller.matrix_is_driving());
        assert_eq!(controller.admission.available_permits(), 3);
        sink.close().await.unwrap();
    }
}
#[cfg(unix)]
#[tokio::test]
async fn matrix_fingerprint_tracks_declared_symlink_target_and_rejects_escape() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    std::fs::write(root.path().join("untracked-input"), "first input").unwrap();
    std::os::unix::fs::symlink("untracked-input", root.path().join("input-link")).unwrap();
    let mut spec = MatrixSpec {
        id: "linked-input".into(),
        resources: Default::default(),
        tasks: vec![task("read", WorkspaceAccess::Read)],
    };
    spec.tasks[0].inputs = vec!["input-link".into()];
    vtcode_memory::matrix::validate_workspace(&spec, root.path()).unwrap();
    let first = fingerprint::fingerprint(root.path(), &spec).await.unwrap();
    assert_eq!(first, fingerprint::fingerprint(root.path(), &spec).await.unwrap());
    std::fs::write(root.path().join("untracked-input"), "second input with a different size").unwrap();
    assert_ne!(first, fingerprint::fingerprint(root.path(), &spec).await.unwrap());

    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "outside workspace").unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret"), root.path().join("outside-link")).unwrap();
    spec.tasks[0].inputs = vec!["outside-link".into()];
    assert!(
        fingerprint::fingerprint(root.path(), &spec)
            .await
            .unwrap_err()
            .to_string()
            .contains("escaped workspace")
    );
}
#[tokio::test]
async fn matrix_mock_resources_edit_timeout_retry_and_generation_rerun() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    let sink = SessionStoreSink::open(root.path(), "matrix-mock").await.unwrap();
    let controller = controller(root.path(), &sink).await;
    let trace = Arc::new(parking_lot::Mutex::new(Vec::<(String, MatrixPhase, &'static str)>::new()));
    let active = Arc::new(parking_lot::Mutex::new(Vec::<(String, WorkspaceAccess)>::new()));
    let retry_used = Arc::new(AtomicBool::new(false));
    let changed = Arc::new(AtomicBool::new(false));
    let source = root.path().join("source.rs");
    let trace_capture = trace.clone();
    *controller.matrix.executor_override.write() = Some(Arc::new(move |assignment, task, _cancel| {
        let trace = trace_capture.clone();
        let active = active.clone();
        let retry_used = retry_used.clone();
        let changed = changed.clone();
        let source = source.clone();
        Box::pin(async move {
            {
                let mut active = active.lock();
                assert!(active.len() < 3);
                assert!(
                    active
                        .iter()
                        .all(|(_, access)| *access == WorkspaceAccess::Read && task.access == WorkspaceAccess::Read)
                );
                if task.resources.contains_key("cargo") {
                    assert!(!active.iter().any(|(id, _)| id == "slow" || id == "blocked"));
                }
                active.push((task.id.clone(), task.access));
            }
            trace.lock().push((task.id.clone(), assignment.phase, "start"));
            tokio::time::sleep(Duration::from_millis(if task.id == "slow" { 35 } else { 5 })).await;
            let mut result = success(assignment, task.clone());
            if task.id == "retry"
                && result.assignment.phase == MatrixPhase::Execute
                && !retry_used.swap(true, Ordering::AcqRel)
            {
                result.outcome = MatrixOutcome::TimedOut;
            }
            if task.id == "writer" && result.assignment.phase == MatrixPhase::Execute {
                std::fs::write(&source, "edited").unwrap();
            }
            if task.id == "slow"
                && result.assignment.phase == MatrixPhase::Verify
                && !changed.swap(true, Ordering::AcqRel)
            {
                std::fs::write(&source, "changed-during-verification").unwrap();
            }
            trace.lock().push((task.id.clone(), result.assignment.phase, "end"));
            active.lock().retain(|(id, _)| id != &task.id);
            result
        })
    }));
    let mut slow = task("slow", WorkspaceAccess::Read);
    slow.resources.insert("cargo".into(), 1);
    let mut blocked = task("blocked", WorkspaceAccess::Read);
    blocked.resources.insert("cargo".into(), 1);
    let mut retry = task("retry", WorkspaceAccess::Read);
    retry.replay_safe = true;
    let mut writer = task("writer", WorkspaceAccess::Write);
    writer.dependencies = vec!["slow".into(), "blocked".into(), "retry".into()];
    let spec = MatrixSpec {
        id: "mock".into(),
        tasks: vec![slow, blocked, task("independent", WorkspaceAccess::Read), retry, writer],
        resources: BTreeMap::from([("cargo".into(), 1)]),
    };
    controller.matrix_control(json!({"action":"create","spec":spec})).await.unwrap();
    assert!(trace.lock().is_empty());
    controller
        .matrix_control(json!({"action":"start","matrix_id":"mock"}))
        .await
        .unwrap();
    let snapshot = wait_terminal(&controller).await;
    assert_eq!(snapshot.lifecycle, MatrixLifecycle::Succeeded);
    let trace = trace.lock().clone();
    let position = |id, event| {
        trace
            .iter()
            .position(|(task, phase, action)| task == id && *phase == MatrixPhase::Execute && *action == event)
            .unwrap()
    };
    assert!(position("independent", "start") < position("slow", "end"));
    assert!(position("blocked", "start") > position("slow", "end"));
    assert!(position("writer", "start") > position("blocked", "end"));
    for task in &snapshot.tasks {
        let verification = task.attempts.last().unwrap();
        assert_eq!(verification.phase, MatrixPhase::Verify);
        assert_eq!(verification.generation, snapshot.generation);
        assert!(
            verification
                .evidence
                .iter()
                .all(|evidence| Some(&evidence.generation) == snapshot.generation.as_ref())
        );
    }
    assert!(
        snapshot.tasks[0]
            .attempts
            .iter()
            .filter(|attempt| attempt.phase == MatrixPhase::Verify)
            .count()
            >= 2
    );
    assert_eq!(snapshot.tasks.iter().find(|task| task.id == "retry").unwrap().automatic_retries, 1);
    let validate = sink.decision_validator();
    for task in &snapshot.tasks {
        let ids = task
            .attempts
            .last()
            .unwrap()
            .evidence
            .iter()
            .map(|evidence| evidence.event_id.clone())
            .collect();
        validate(format!("{}/{}", snapshot.spec.id, task.id), ids).await.unwrap();
    }
    assert!(
        validate(
            "unrelated-task".into(),
            vec![snapshot.tasks[0].attempts.last().unwrap().evidence[0].event_id.clone()]
        )
        .await
        .is_err()
    );
    let persisted = (sink.matrix_persistence().load)().await.unwrap();
    assert_eq!(persisted, vec![snapshot.clone()]);
    tokio::time::timeout(Duration::from_secs(1), async {
        while !controller.has_matrix_completion() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    controller.matrix_tracker_projection().await.unwrap();
    assert!(controller.has_matrix_completion(), "views must not consume completion delivery");
    assert_eq!(controller.take_matrix_completion(), Some(snapshot));
    assert!(!controller.has_matrix_completion());
    sink.close().await.unwrap();
}
#[tokio::test]
async fn matrix_reopened_sink_does_not_republish_historical_checks() {
    use crate::exec::events::{ThreadEvent, ThreadItemDetails, VersionedThreadEvent};

    let root = tempfile::tempdir().unwrap();
    let work = task("checked", WorkspaceAccess::Read);
    let mut state = MatrixState::create(
        MatrixSpec {
            id: "reopen".into(),
            tasks: vec![work.clone()],
            resources: Default::default(),
        },
        root.path(),
    )
    .unwrap();
    state.start().unwrap();
    let execution = state.reserve_ready(1).unwrap().remove(0);
    state.mark_launch_requested(&execution.attempt_id).unwrap();
    state
        .report(&execution.attempt_id, &execution.worker_id, MatrixOutcome::Success, vec![], true)
        .unwrap();
    state.begin_verification("workspace-generation".into()).unwrap();
    let check = state.reserve_ready(1).unwrap().remove(0);
    state.mark_launch_requested(&check.attempt_id).unwrap();
    let result = success(check, work);
    state
        .report(&result.assignment.attempt_id, &result.assignment.worker_id, result.outcome, result.evidence, true)
        .unwrap();
    state.pause().unwrap();

    let read_checks = || {
        std::fs::read_to_string(root.path().join(".vtcode/sessions/reopen-evidence/events.jsonl"))
            .unwrap()
            .lines()
            .filter_map(|line| match serde_json::from_str::<VersionedThreadEvent>(line).unwrap().into_event() {
                ThreadEvent::ItemCompleted(event)
                    if matches!(event.item.details, ThreadItemDetails::CommandExecution(_)) =>
                {
                    Some(event.item)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let turn_completed = || {
        ThreadEvent::TurnCompleted(crate::exec::events::TurnCompletedEvent {
            completed_at: None,
            usage: Default::default(),
            in_progress_exec_sessions: vec![],
        })
    };
    let sink = SessionStoreSink::open(root.path(), "reopen-evidence").await.unwrap();
    sink.event_sink().lock()(&ThreadEvent::TurnStarted(crate::exec::events::TurnStartedEvent::default()));
    (sink.matrix_persistence().persist)(state.snapshot().clone()).await.unwrap();
    sink.event_sink().lock()(&turn_completed());
    sink.close().await.unwrap();
    let original = read_checks();
    assert_eq!(original.len(), 2, "two declared checks executed once");

    let reopened = SessionStoreSink::open(root.path(), "reopen-evidence").await.unwrap();
    reopened.event_sink().lock()(&ThreadEvent::TurnStarted(crate::exec::events::TurnStartedEvent::default()));
    state.resume().unwrap();
    (reopened.matrix_persistence().persist)(state.snapshot().clone()).await.unwrap();
    reopened.event_sink().lock()(&turn_completed());
    reopened.close().await.unwrap();
    assert_eq!(read_checks(), original, "replay must preserve record count, identities, and timestamps");

    // A cap rewrite may retain the checkpoint while evicting command records.
    // Their historical evidence must still not be emitted as a new execution.
    let capped = vtcode_memory::open(root.path(), "reopen-evidence", 1).unwrap();
    for _ in 0..4 {
        capped
            .append(&ThreadEvent::TurnStarted(crate::exec::events::TurnStartedEvent::default()))
            .unwrap();
        capped.append(&turn_completed()).unwrap();
    }
    capped.flush().unwrap();
    drop(capped);
    assert!(read_checks().is_empty());
    let reopened = SessionStoreSink::open(root.path(), "reopen-evidence").await.unwrap();
    state.pause().unwrap();
    (reopened.matrix_persistence().persist)(state.snapshot().clone()).await.unwrap();
    reopened.close().await.unwrap();
    assert!(read_checks().is_empty(), "evicted historical checks cannot acquire fresh timestamps");
}
