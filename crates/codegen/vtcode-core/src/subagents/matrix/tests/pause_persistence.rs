use super::*;

#[tokio::test]
async fn matrix_pause_finishes_active_work_cancel_waits_for_cleanup_and_disallows_resume() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    let sink = SessionStoreSink::open(root.path(), "pause-cancel").await.unwrap();
    let controller = controller(root.path(), &sink).await;
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let started_capture = started.clone();
    let release_capture = release.clone();
    *controller.matrix.executor_override.write() = Some(Arc::new(move |assignment, task, cancel| {
        let started = started_capture.clone();
        let release = release_capture.clone();
        Box::pin(async move {
            started.notify_one();
            tokio::select! { () = release.notified() => success(assignment,task), () = cancel.cancelled() => {
                tokio::time::sleep(Duration::from_millis(30)).await;
                worker::WorkerResult { assignment,outcome:MatrixOutcome::Cancelled,evidence:vec![],cleanup_confirmed:true }
            } }
        })
    }));
    let spec = MatrixSpec {
        id: "pause".into(),
        tasks: vec![
            task("writer", WorkspaceAccess::Write),
            task("queued", WorkspaceAccess::Read),
        ],
        resources: Default::default(),
    };
    controller.matrix_control(json!({"action":"create","spec":spec})).await.unwrap();
    controller
        .matrix_control(json!({"action":"start","matrix_id":"pause"}))
        .await
        .unwrap();
    started.notified().await;
    controller
        .matrix_control(json!({"action":"pause","matrix_id":"pause"}))
        .await
        .unwrap();
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), async {
        while controller.matrix.driver_active.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let state = controller.matrix.state.lock().await.as_ref().unwrap().snapshot().clone();
    assert_eq!(state.lifecycle, MatrixLifecycle::Paused);
    assert_eq!(state.tasks[0].status, MatrixTaskStatus::Executed);
    assert!(state.tasks[1].attempts.is_empty());
    controller
        .matrix_control(json!({"action":"resume","matrix_id":"pause"}))
        .await
        .unwrap();
    started.notified().await;
    controller
        .matrix_control(json!({"action":"cancel","matrix_id":"pause"}))
        .await
        .unwrap();
    assert!(
        controller
            .matrix_control(json!({"action":"resume","matrix_id":"pause"}))
            .await
            .is_err()
    );
    assert_eq!(wait_terminal(&controller).await.lifecycle, MatrixLifecycle::Cancelled);
    assert_eq!(controller.admission.available_permits(), controller.config.vt_cfg.subagents.max_concurrent);
    sink.close().await.unwrap();
}
#[tokio::test]
async fn matrix_persistence_rejection_rolls_back_start_and_worker_capacity() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    let sink = SessionStoreSink::open(root.path(), "reject").await.unwrap();
    let controller = controller(root.path(), &sink).await;
    let spec = MatrixSpec {
        id: "reject".into(),
        tasks: vec![task("a", WorkspaceAccess::Read)],
        resources: Default::default(),
    };
    controller.matrix_control(json!({"action":"create","spec":spec})).await.unwrap();
    controller
        .set_matrix_persistence(MatrixPersistence {
            persist: Arc::new(|_| Box::pin(async { bail!("injected persistence failure") })),
            load: sink.matrix_persistence().load,
        })
        .await
        .unwrap();
    assert!(
        controller
            .matrix_control(json!({"action":"start","matrix_id":"reject"}))
            .await
            .is_err()
    );
    assert!(!controller.matrix_is_executing());
    assert_eq!(
        controller.matrix.state.lock().await.as_ref().unwrap().snapshot().lifecycle,
        MatrixLifecycle::Created
    );
    assert_eq!(controller.admission.available_permits(), 3);
    assert!(controller.cancel_matrix().await.is_err());
    assert!(controller.matrix.cancellation.read().is_cancelled());
    sink.close().await.unwrap();
}
#[tokio::test]
async fn matrix_persistence_failures_do_not_cross_launch_or_completion_barriers() {
    use std::sync::atomic::AtomicUsize;

    #[derive(Clone, Copy, Debug)]
    enum FailureStage {
        Assignment,
        Launch,
        ExecutionResult,
        VerificationResult,
    }

    for stage in [
        FailureStage::Assignment,
        FailureStage::Launch,
        FailureStage::ExecutionResult,
        FailureStage::VerificationResult,
    ] {
        let root = tempfile::tempdir().unwrap();
        init_git(root.path());
        let sink = SessionStoreSink::open(root.path(), "persistence-boundaries").await.unwrap();
        let running = controller(root.path(), &sink).await;
        running
            .matrix_control(json!({"action":"create", "spec": {
                "id":"barriers", "tasks":[task("one", WorkspaceAccess::Read)]
            }}))
            .await
            .unwrap();
        let persistence = sink.matrix_persistence();
        let persist = persistence.persist.clone();
        let failed = Arc::new(AtomicBool::new(false));
        let failed_capture = failed.clone();
        running
            .set_matrix_persistence(MatrixPersistence {
                persist: Arc::new(move |snapshot| {
                    let persist = persist.clone();
                    let failed = failed_capture.clone();
                    Box::pin(async move {
                        let reject = snapshot.tasks[0].attempts.last().is_some_and(|attempt| match stage {
                            FailureStage::Assignment => !attempt.launch_requested,
                            FailureStage::Launch => attempt.launch_requested && attempt.outcome.is_none(),
                            FailureStage::ExecutionResult => {
                                attempt.phase == MatrixPhase::Execute && attempt.outcome.is_some()
                            }
                            FailureStage::VerificationResult => {
                                attempt.phase == MatrixPhase::Verify && attempt.outcome.is_some()
                            }
                        });
                        if reject && !failed.swap(true, Ordering::AcqRel) {
                            bail!("injected {stage:?} persistence failure");
                        }
                        persist(snapshot).await
                    })
                }),
                load: persistence.load.clone(),
            })
            .await
            .unwrap();
        let launches = Arc::new(AtomicUsize::new(0));
        let launches_capture = launches.clone();
        *running.matrix.executor_override.write() = Some(Arc::new(move |assignment, task, _| {
            launches_capture.fetch_add(1, Ordering::AcqRel);
            Box::pin(async move { success(assignment, task) })
        }));
        running
            .matrix_control(json!({"action":"start", "matrix_id":"barriers"}))
            .await
            .unwrap();
        let snapshot = tokio::time::timeout(Duration::from_secs(10), running.wait_matrix_idle())
            .await
            .unwrap()
            .unwrap();
        assert!(failed.load(Ordering::Acquire), "{stage:?}");
        assert_eq!(
            launches.load(Ordering::Acquire),
            match stage {
                FailureStage::Assignment | FailureStage::Launch => 0,
                FailureStage::ExecutionResult => 1,
                FailureStage::VerificationResult => 2,
            }
        );
        assert!(running.matrix.error.read().is_some());
        assert!(running.take_matrix_completion().is_none());
        assert_eq!(running.admission.available_permits(), 3);
        assert_eq!(
            snapshot.lifecycle,
            if matches!(stage, FailureStage::VerificationResult) {
                MatrixLifecycle::Verifying
            } else {
                MatrixLifecycle::Running
            }
        );
        assert_eq!((persistence.load)().await.unwrap(), vec![snapshot.clone()]);
        match stage {
            FailureStage::Assignment => {
                assert_eq!(snapshot.tasks[0].status, MatrixTaskStatus::Queued);
                assert!(snapshot.tasks[0].attempts.is_empty());
            }
            FailureStage::Launch | FailureStage::ExecutionResult | FailureStage::VerificationResult => {
                let attempt = snapshot.tasks[0].attempts.last().unwrap();
                assert_eq!(snapshot.tasks[0].status, MatrixTaskStatus::Assigned);
                assert_eq!(attempt.launch_requested, !matches!(stage, FailureStage::Launch));
                assert!(attempt.outcome.is_none());
                assert!(!attempt.cleanup_confirmed);
            }
        }

        // Resume through a fresh controller using only acknowledged checkpoints.
        let restored = controller(root.path(), &sink).await;
        *restored.matrix.executor_override.write() =
            Some(Arc::new(|assignment, task, _| Box::pin(async move { success(assignment, task) })));
        let resumed = restored
            .matrix_control(json!({"action":"resume", "matrix_id":"barriers"}))
            .await;
        if matches!(stage, FailureStage::ExecutionResult | FailureStage::VerificationResult) {
            assert!(resumed.is_err());
            let snapshot = restored.matrix_snapshot().await.unwrap();
            assert_eq!(snapshot.lifecycle, MatrixLifecycle::Blocked);
            assert_eq!(snapshot.tasks[0].status, MatrixTaskStatus::CleanupUncertain);
            assert_eq!(restored.matrix.state.lock().await.as_ref().unwrap().active_assignments().len(), 1);
            assert!(!restored.matrix_is_driving());
            assert!(restored.matrix_is_executing());
        } else {
            resumed.unwrap();
            assert_eq!(wait_terminal(&restored).await.lifecycle, MatrixLifecycle::Succeeded);
        }
        sink.close().await.unwrap();
    }
}
#[tokio::test]
async fn matrix_persists_stopped_verification_before_missing_input_reconciliation() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    std::fs::write(root.path().join("input.txt"), "input").unwrap();
    let sink = SessionStoreSink::open(root.path(), "missing-input").await.unwrap();
    let controller = controller(root.path(), &sink).await;
    let input = root.path().join("input.txt");
    *controller.matrix.executor_override.write() = Some(Arc::new(move |assignment, task, _cancel| {
        let input = input.clone();
        Box::pin(async move {
            if assignment.phase == MatrixPhase::Verify {
                std::fs::remove_file(input).unwrap();
            }
            success(assignment, task)
        })
    }));
    let mut work = task("check", WorkspaceAccess::Read);
    work.inputs.push("input.txt".into());
    let spec = MatrixSpec {
        id: "missing-input".into(),
        tasks: vec![work],
        resources: Default::default(),
    };
    controller.matrix_control(json!({"action":"create","spec":spec})).await.unwrap();
    controller
        .matrix_control(json!({"action":"start","matrix_id":"missing-input"}))
        .await
        .unwrap();
    let snapshot = tokio::time::timeout(Duration::from_secs(3), controller.wait_matrix_idle())
        .await
        .unwrap()
        .unwrap();
    assert_ne!(snapshot.lifecycle, MatrixLifecycle::Succeeded);
    let attempt = snapshot.tasks[0].attempts.last().unwrap();
    assert_eq!(attempt.phase, MatrixPhase::Verify);
    assert_eq!(attempt.outcome, Some(MatrixOutcome::Success));
    assert!(attempt.cleanup_confirmed);
    assert_eq!(attempt.evidence.len(), 2);
    assert_eq!((sink.matrix_persistence().load)().await.unwrap(), vec![snapshot]);
    assert!(controller.matrix.error.read().is_some());
    std::fs::write(root.path().join("input.txt"), "restored input").unwrap();
    *controller.matrix.executor_override.write() =
        Some(Arc::new(|assignment, task, _| Box::pin(async move { success(assignment, task) })));
    controller
        .matrix_control(json!({"action":"resume","matrix_id":"missing-input"}))
        .await
        .unwrap();
    assert_eq!(wait_terminal(&controller).await.lifecycle, MatrixLifecycle::Succeeded);
    assert!(controller.matrix.error.read().is_none());
    controller.cancel_matrix().await.unwrap();
    sink.close().await.unwrap();
}
