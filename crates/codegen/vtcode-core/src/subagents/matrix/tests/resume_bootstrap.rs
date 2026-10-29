use super::*;

#[tokio::test]
async fn matrix_fingerprint_handles_gitlinks_and_tracks_submodule_state() {
    fn git(root: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git").args(args).current_dir(root).output().unwrap();
        assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }
    fn commit(root: &std::path::Path) {
        git(
            root,
            &[
                "-c",
                "user.name=Matrix Test",
                "-c",
                "user.email=matrix@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "fixture",
            ],
        );
    }
    let root = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    init_git(root.path());
    init_git(source.path());
    commit(source.path());
    git(
        root.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            source.path().to_str().unwrap(),
            "vendor",
        ],
    );
    let spec = MatrixSpec {
        id: "gitlinks".into(),
        tasks: vec![task("read", WorkspaceAccess::Read)],
        resources: Default::default(),
    };
    let initial = fingerprint::fingerprint(root.path(), &spec).await.unwrap();
    assert_eq!(fingerprint::fingerprint(root.path(), &spec).await.unwrap(), initial);
    commit(&root.path().join("vendor"));
    let head_changed = fingerprint::fingerprint(root.path(), &spec).await.unwrap();
    assert_ne!(initial, head_changed, "submodule HEAD alone invalidates verification");
    std::fs::write(root.path().join("vendor/source.rs"), "dirty submodule source").unwrap();
    let dirty = fingerprint::fingerprint(root.path(), &spec).await.unwrap();
    assert_ne!(head_changed, dirty, "uncommitted submodule content invalidates verification");

    git(
        &root.path().join("vendor"),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            source.path().to_str().unwrap(),
            "nested",
        ],
    );
    let nested = fingerprint::fingerprint(root.path(), &spec).await.unwrap();
    std::fs::write(root.path().join("vendor/nested/source.rs"), "nested dirty source").unwrap();
    assert_ne!(nested, fingerprint::fingerprint(root.path(), &spec).await.unwrap());
    git(root.path(), &["submodule", "deinit", "--force", "--", "vendor"]);
    let uninitialized = fingerprint::fingerprint(root.path(), &spec).await.unwrap();
    assert_ne!(dirty, uninitialized);
    assert_eq!(uninitialized, fingerprint::fingerprint(root.path(), &spec).await.unwrap());
    #[cfg(unix)]
    {
        std::fs::remove_dir(root.path().join("vendor")).unwrap();
        std::os::unix::fs::symlink(source.path(), root.path().join("vendor")).unwrap();
        assert!(
            fingerprint::fingerprint(root.path(), &spec).await.is_err(),
            "gitlinks cannot follow escaping symlinks"
        );
    }
}
#[tokio::test]
async fn matrix_resume_prepared_assignment_and_blocks_launched_survivor() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    let sink = SessionStoreSink::open(root.path(), "resume").await.unwrap();
    let persistence = sink.matrix_persistence();
    let mut state = MatrixState::create(
        MatrixSpec {
            id: "prepared".into(),
            tasks: vec![task("one", WorkspaceAccess::Read)],
            resources: Default::default(),
        },
        root.path(),
    )
    .unwrap();
    (persistence.persist)(state.snapshot().clone()).await.unwrap();
    state.start().unwrap();
    state.reserve_ready(3).unwrap();
    (persistence.persist)(state.snapshot().clone()).await.unwrap();
    let restored = controller(root.path(), &sink).await;
    *restored.matrix.executor_override.write() =
        Some(Arc::new(|assignment, task, _| Box::pin(async move { success(assignment, task) })));
    restored
        .matrix_control(json!({"action":"resume","matrix_id":"prepared"}))
        .await
        .unwrap();
    assert_eq!(wait_terminal(&restored).await.lifecycle, MatrixLifecycle::Succeeded);
    let mut uncertain = MatrixState::create(
        MatrixSpec {
            id: "launched".into(),
            tasks: vec![task("one", WorkspaceAccess::Read)],
            resources: Default::default(),
        },
        root.path(),
    )
    .unwrap();
    uncertain.start().unwrap();
    let assignment = uncertain.reserve_ready(3).unwrap().remove(0);
    uncertain.mark_launch_requested(&assignment.attempt_id).unwrap();
    (persistence.persist)(uncertain.snapshot().clone()).await.unwrap();
    let blocked = controller(root.path(), &sink).await;
    assert!(
        blocked.ensure_ordinary_delegation_allowed().is_err(),
        "persisted launched work must exclude discovery before any matrix control call"
    );
    assert!(
        blocked
            .matrix_control(json!({"action":"resume","matrix_id":"launched"}))
            .await
            .is_err()
    );
    let snapshot = blocked.matrix.state.lock().await.as_ref().unwrap().snapshot().clone();
    assert_eq!(snapshot.lifecycle, MatrixLifecycle::Blocked);
    assert_eq!(snapshot.tasks[0].status, MatrixTaskStatus::CleanupUncertain);
    assert!(!blocked.matrix.driver_active.load(Ordering::Acquire));
    assert!(blocked.matrix_is_executing(), "uncertain recovered leases must still exclude discovery");
    let registry = ToolRegistry::new(root.path().to_path_buf()).await;
    registry.set_subagent_controller(Arc::new(blocked));
    registry.set_matrix_coordinator(true);
    assert!(rejected(
        registry
            .execute_tool("agent", json!({"action":"spawn","agent_type":"explore","message":"Inspect source.rs"}))
            .await
    ));
    sink.close().await.unwrap();
}
#[tokio::test]
async fn matrix_bootstrap_load_failure_keeps_discovery_closed_until_successful_replay() {
    let root = tempfile::tempdir().unwrap();
    let sink = SessionStoreSink::open(root.path(), "bootstrap").await.unwrap();
    let controller =
        SubagentController::new(test_controller_config(root.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .unwrap();
    assert!(controller.ensure_ordinary_delegation_allowed().is_ok());
    assert!(
        controller
            .set_matrix_persistence(MatrixPersistence {
                persist: sink.matrix_persistence().persist,
                load: Arc::new(|| Box::pin(async { bail!("injected replay failure") })),
            })
            .await
            .is_err()
    );
    assert!(controller.ensure_ordinary_delegation_allowed().is_err());
    controller.set_matrix_persistence(sink.matrix_persistence()).await.unwrap();
    assert!(controller.ensure_ordinary_delegation_allowed().is_ok());
    sink.close().await.unwrap();
}
#[tokio::test]
async fn matrix_restored_resume_waits_for_discovery_and_idle_cancel_releases_admission() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    let sink = SessionStoreSink::open(root.path(), "discovery-admission").await.unwrap();
    let mut state = MatrixState::create(
        MatrixSpec {
            id: "restored".into(),
            resources: Default::default(),
            tasks: vec![task("one", WorkspaceAccess::Read)],
        },
        root.path(),
    )
    .unwrap();
    state.start().unwrap();
    (sink.matrix_persistence().persist)(state.snapshot().clone()).await.unwrap();
    let restored = controller(root.path(), &sink).await;
    *restored.matrix.executor_override.write() =
        Some(Arc::new(|assignment, task, _| Box::pin(async move { success(assignment, task) })));
    let discovery = restored.admission.clone().try_acquire_owned().unwrap();
    restored
        .matrix_control(json!({"action":"status","matrix_id":"restored"}))
        .await
        .unwrap();
    assert!(restored.matrix_is_executing());
    assert!(
        restored
            .matrix_control(json!({"action":"resume","matrix_id":"restored"}))
            .await
            .is_err(),
        "resuming shared-workspace work must wait for discovery to stop"
    );
    assert!(!restored.matrix_is_driving());
    assert!(
        restored
            .matrix
            .state
            .lock()
            .await
            .as_ref()
            .unwrap()
            .active_assignments()
            .is_empty()
    );
    drop(discovery);
    restored
        .matrix_control(json!({"action":"cancel","matrix_id":"restored"}))
        .await
        .unwrap();
    assert!(!restored.matrix_is_executing(), "a cancelled matrix with no owned work is idle");
    restored
        .matrix_control(json!({"action":"create","spec":{
            "id":"new","tasks":[task("one",WorkspaceAccess::Read)]
        }}))
        .await
        .unwrap();
    assert!(!restored.matrix_is_executing(), "a new idle matrix must permit discovery");
    restored
        .matrix_control(json!({"action":"start","matrix_id":"new"}))
        .await
        .unwrap();
    assert_eq!(wait_terminal(&restored).await.lifecycle, MatrixLifecycle::Succeeded);
    assert_eq!(restored.admission.available_permits(), 3);
    sink.close().await.unwrap();
}
