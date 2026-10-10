use super::*;
use crate::config::VTCodeConfig;
use crate::core::agent::events::SessionStoreSink;
use crate::subagents::tests::test_controller_config;
use crate::tools::registry::ToolRegistry;
use std::time::Duration;

fn task(id: &str, access: WorkspaceAccess) -> MatrixTaskSpec {
    MatrixTaskSpec {
        id: id.into(),
        instructions: format!("perform {id}"),
        dependencies: vec![],
        workspace: ".".into(),
        access,
        checks: vec![format!("check-{id}"), format!("second-{id}")],
        resources: Default::default(),
        timeout_secs: 5,
        replay_safe: false,
        inputs: vec![],
    }
}
fn rejected(result: Result<Value>) -> bool {
    match result {
        Err(_) => true,
        Ok(value) => value.get("success") == Some(&json!(false)) || value.get("error").is_some(),
    }
}
async fn controller(root: &std::path::Path, sink: &SessionStoreSink) -> SubagentController {
    let controller = SubagentController::new(test_controller_config(root.to_path_buf(), VTCodeConfig::default()))
        .await
        .unwrap();
    controller.set_matrix_persistence(sink.matrix_persistence()).await.unwrap();
    controller
}
fn success(assignment: MatrixAssignment, task: MatrixTaskSpec) -> worker::WorkerResult {
    let evidence = if assignment.phase == MatrixPhase::Verify {
        task.checks
            .into_iter()
            .enumerate()
            .map(|(index, command)| MatrixCommandEvidence {
                command,
                attempt_id: assignment.attempt_id.clone(),
                worker_id: assignment.worker_id.clone(),
                generation: assignment.generation.clone().unwrap(),
                event_id: format!("{}-{index}", assignment.attempt_id),
                exit_code: Some(0),
                cancelled: false,
            })
            .collect()
    } else {
        vec![]
    };
    worker::WorkerResult {
        assignment,
        outcome: MatrixOutcome::Success,
        evidence,
        cleanup_confirmed: true,
    }
}
async fn wait_terminal(controller: &SubagentController) -> MatrixSnapshot {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = controller.matrix.state.lock().await.as_ref().unwrap().snapshot().clone();
            if matches!(
                snapshot.lifecycle,
                MatrixLifecycle::Succeeded | MatrixLifecycle::Blocked | MatrixLifecycle::Cancelled
            ) && !controller.matrix.driver_active.load(Ordering::Acquire)
            {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}
fn init_git(root: &std::path::Path) {
    assert!(
        std::process::Command::new("git")
            .arg("init")
            .current_dir(root)
            .output()
            .unwrap()
            .status
            .success()
    );
    std::fs::write(root.join("source.rs"), "before").unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["add", "source.rs"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
}

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

#[tokio::test]
async fn matrix_public_role_admission_and_worker_identity_are_enforced() {
    let root = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(root.path().to_path_buf()).await;
    registry.set_subagent_controller(Arc::new(
        SubagentController::new(test_controller_config(root.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .unwrap(),
    ));
    registry.set_matrix_coordinator(true);
    let catalog_config = crate::tools::handlers::SessionToolsConfig::full_public(
        crate::tools::handlers::SessionSurface::AgentRunner,
        crate::config::types::CapabilityLevel::CodeSearch,
        crate::config::ToolDocumentationMode::Full,
        Default::default(),
    );
    let catalog = registry.model_tools(catalog_config.clone()).await;
    let names = catalog.iter().map(|tool| tool.function_name()).collect::<Vec<_>>();
    for required in ["matrix", "agent", "task_tracker", "record_decision"] {
        assert!(names.contains(&required), "missing coordinator control: {required}");
    }
    assert!(names.iter().all(|name| registry.enforce_matrix_role(name).is_ok()));
    assert!(catalog.iter().all(|tool| tool.defer_loading != Some(true)));
    for name in ["exec_command", "apply_patch", "code_search"] {
        assert!(
            registry
                .execute_tool(name, json!({}))
                .await
                .unwrap_err()
                .to_string()
                .contains("coordinator delegates")
        );
    }
    assert!(rejected(
        registry
            .execute_tool("matrix", json!({"action":"report","outcome":"executed"}))
            .await
    ));
    let assignment = MatrixAssignment {
        task_id: "owned".into(),
        attempt_id: "attempt".into(),
        worker_id: "worker".into(),
        phase: MatrixPhase::Execute,
        generation: None,
    };
    registry.set_matrix_worker(MatrixWorkerContext::new(assignment));
    let worker_catalog = registry.model_tools(catalog_config).await;
    assert!(worker_catalog.iter().any(|tool| tool.function_name() == "matrix"));
    assert!(worker_catalog.iter().any(|tool| tool.function_name() == "exec_command"));
    assert!(worker_catalog.iter().all(|tool| tool.function_name() != "agent"));
    assert!(
        registry
            .execute_tool("agent", json!({"action":"spawn"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("nested delegation")
    );
    assert!(rejected(
        registry
            .execute_tool("matrix", json!({"action":"report","task_id":"other","outcome":"executed"}))
            .await
    ));
    let accepted = registry
        .execute_tool("matrix", json!({"action":"report","outcome":"executed"}))
        .await
        .unwrap();
    assert_eq!(accepted["accepted"], true);

    assert!(rejected(
        registry
            .execute_tool("matrix", json!({"action":"report","outcome":"executed"}))
            .await
    ));
    assert!(
        registry.enforce_matrix_role("exec_command").is_ok(),
        "coordinator catalog must not restrict authorized worker execution"
    );
}

#[tokio::test]
async fn matrix_read_lease_rejects_mutating_worker_overrides_before_model_setup() {
    for (access, mutating, expected) in [
        (WorkspaceAccess::Read, true, MatrixOutcome::PermissionDenied),
        (WorkspaceAccess::Write, true, MatrixOutcome::Failed),
        (WorkspaceAccess::Read, false, MatrixOutcome::Failed),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut specs = vtcode_config::builtin_subagents();
        let name = if access == WorkspaceAccess::Read {
            "explorer"
        } else {
            "default"
        };
        let spec = specs.iter_mut().find(|spec| spec.name == name).unwrap();
        if mutating {
            spec.tools = Some(vec!["apply_patch".into()]);
            spec.permissions = vtcode_config::core::permissions::AgentPermissionsConfig::new(
                vtcode_config::core::permissions::PermissionDefault::Allow,
            );
        }
        assert_eq!(spec.is_read_only(), !mutating);
        // A rejected model stops admitted cases before provider I/O. The read
        // lease violation must instead fail at worker-policy validation.
        spec.model = Some("invalid-matrix-regression-model".into());
        let controller = SubagentController::new_with_discovered(
            test_controller_config(root.path().to_path_buf(), VTCodeConfig::default()),
            vtcode_config::DiscoveredSubagents { effective: specs, shadowed: vec![] },
        )
        .await
        .unwrap();
        let result = Box::pin(worker::execute(
            &controller,
            MatrixAssignment {
                task_id: "read-policy".into(),
                attempt_id: "owned-attempt".into(),
                worker_id: "owned-worker".into(),
                phase: MatrixPhase::Execute,
                generation: None,
            },
            task("read-policy", access),
            CancellationToken::new(),
        ))
        .await;
        assert_eq!(result.outcome, expected, "{access:?}, mutating={mutating}");
        assert!(result.cleanup_confirmed);
        assert!(result.evidence.is_empty());
        assert!(!root.path().join("proof.txt").exists());
    }
}

#[tokio::test]
async fn matrix_real_checks_collect_nonzero_and_later_success_and_timeout_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let sink = SessionStoreSink::open(root.path(), "real-checks").await.unwrap();
    let mut cfg = VTCodeConfig::default();
    cfg.automation.full_auto.enabled = true;
    cfg.commands.allow_list.extend(["false", "printf", "sleep"].map(str::to_owned));
    let controller = SubagentController::new(test_controller_config(root.path().to_path_buf(), cfg))
        .await
        .unwrap();
    controller.set_matrix_persistence(sink.matrix_persistence()).await.unwrap();
    let assignment = MatrixAssignment {
        task_id: "checks".into(),
        attempt_id: "real-one".into(),
        worker_id: "worker-one".into(),
        phase: MatrixPhase::Verify,
        generation: Some("generation".into()),
    };
    let mut checks = task("checks", WorkspaceAccess::Read);
    checks.checks = vec!["false".into(), "printf verified > proof.txt".into()];
    let result =
        Box::pin(worker::execute(&controller, assignment.clone(), checks.clone(), CancellationToken::new())).await;
    assert_eq!(result.outcome, MatrixOutcome::Failed);
    assert_eq!(result.evidence.iter().map(|record| record.exit_code).collect::<Vec<_>>(), vec![Some(1), Some(0)]);
    assert!(result.cleanup_confirmed);
    assert_eq!(std::fs::read_to_string(root.path().join("proof.txt")).unwrap(), "verified");
    checks.timeout_secs = 1;
    checks.checks = vec!["sleep 2; printf leaked > late.txt".into()];
    let result = Box::pin(worker::execute(
        &controller,
        MatrixAssignment {
            attempt_id: "real-two".into(),
            worker_id: "worker-two".into(),
            ..assignment
        },
        checks,
        CancellationToken::new(),
    ))
    .await;
    assert_eq!(result.outcome, MatrixOutcome::TimedOut);
    assert!(result.cleanup_confirmed);
    tokio::time::sleep(Duration::from_millis(1250)).await;
    assert!(
        !root.path().join("late.txt").exists(),
        "owned process group must stop before resources can be reused"
    );
    sink.close().await.unwrap();
}

#[tokio::test]
async fn matrix_verification_preserves_full_auto_denials_and_work_budgets() {
    for allow_execution in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.automation.full_auto.enabled = true;
        cfg.agent.harness.max_tool_calls_per_turn = 1;
        cfg.automation.full_auto.allowed_tools = if allow_execution {
            vec!["exec_command".into(), "write_stdin".into()]
        } else {
            vec!["matrix".into()]
        };
        let controller = SubagentController::new(test_controller_config(root.path().to_path_buf(), cfg))
            .await
            .unwrap();
        let assignment = MatrixAssignment {
            task_id: "budget".into(),
            attempt_id: "attempt".into(),
            worker_id: "worker".into(),
            phase: MatrixPhase::Verify,
            generation: Some("generation".into()),
        };
        let mut checks = task("budget", WorkspaceAccess::Read);
        checks.checks = vec!["printf one > first.txt".into(), "printf two > second.txt".into()];
        let result = Box::pin(worker::execute(&controller, assignment, checks, CancellationToken::new())).await;
        assert_eq!(
            result.outcome,
            if allow_execution {
                MatrixOutcome::BudgetExhausted
            } else {
                MatrixOutcome::PermissionDenied
            }
        );
        assert_eq!(result.evidence.len(), usize::from(allow_execution));
        assert_eq!(root.path().join("first.txt").exists(), allow_execution);
        assert!(!root.path().join("second.txt").exists());
        assert!(result.cleanup_confirmed);
    }
}

#[tokio::test]
async fn matrix_verification_enforces_inherited_bash_permissions_despite_full_auto_grants() {
    for deny in [Some("bash"), Some("bash(printf*)"), None] {
        let root = tempfile::tempdir().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.automation.full_auto.enabled = true;
        cfg.automation.full_auto.allowed_tools = vec!["exec_command".into(), "write_stdin".into()];
        cfg.commands.allow_list.push("printf".into());
        cfg.permissions.deny = deny.into_iter().map(str::to_owned).collect();
        let controller = SubagentController::new(test_controller_config(root.path().to_path_buf(), cfg))
            .await
            .unwrap();
        let assignment = MatrixAssignment {
            task_id: "permission".into(),
            attempt_id: "permission-attempt".into(),
            worker_id: "permission-worker".into(),
            phase: MatrixPhase::Verify,
            generation: Some("generation".into()),
        };
        let mut checks = task("permission", WorkspaceAccess::Read);
        checks.checks = vec!["printf verified > proof.txt".into()];
        let result = Box::pin(worker::execute(&controller, assignment, checks, CancellationToken::new())).await;
        assert_eq!(
            result.outcome,
            if deny.is_some() {
                MatrixOutcome::PermissionDenied
            } else {
                MatrixOutcome::Success
            },
            "deny={deny:?}"
        );
        assert_eq!(result.evidence.len(), usize::from(deny.is_none()));
        assert_eq!(root.path().join("proof.txt").exists(), deny.is_none());
        assert!(result.cleanup_confirmed);
        if deny.is_none() {
            assert_eq!(std::fs::read_to_string(root.path().join("proof.txt")).unwrap(), "verified");
        }
    }
}

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
