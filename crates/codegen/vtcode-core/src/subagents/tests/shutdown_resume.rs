use super::*;

#[tokio::test]
async fn close_reports_nested_child_failure() {
    let temp = TempDir::new().expect("tempdir");
    let parent = SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
        .await
        .unwrap();
    let nested = Arc::new(
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .unwrap(),
    );
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .unwrap();
    let handle = tokio::spawn(async { panic!("nested launch failed") });
    while !handle.is_finished() {
        tokio::task::yield_now().await;
    }
    let mut grandchild =
        test_child_record("grandchild", "grandchild-session", "child-session", &spec, SubagentStatus::Running, 2, None);
    grandchild.handle = Some(handle);
    nested.state.write().await.children.insert("grandchild".into(), grandchild);
    let mut child =
        test_child_record("child", "child-session", "parent-session", &spec, SubagentStatus::Running, 1, None);
    child.child_controller = Some(nested);
    parent.state.write().await.children.insert("child".into(), child);
    let error = parent.close("child").await.unwrap_err();
    assert!(format!("{error:#}").contains("grandchild"));
    assert!(parent.resume("child").await.unwrap_err().to_string().contains("closing"));
    parent.close("child").await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_retains_child_handle_after_timeout() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let handle = tokio::spawn(async move {
        let _ = entered_tx.send(());
        let _ = release_rx.recv();
        std::future::pending::<()>().await;
    });
    entered_rx.await.expect("child entered");
    let mut record =
        test_child_record("child", "child-session", "parent-session", &spec, SubagentStatus::Running, 1, None);
    record.handle = Some(handle);
    controller.state.write().await.children.insert("child".into(), record);

    assert!(
        tokio::time::timeout(Duration::from_millis(250), controller.close("child"))
            .await
            .is_err()
    );
    assert_eq!(controller.status_for("child").await.unwrap().status, SubagentStatus::Running);
    assert!(controller.shutdown_handles.lock().await.contains_key("child"));
    assert!(controller.state.read().await.children["child"].closing);
    let error = controller
        .send_input(SendInputRequest {
            target: "child".into(),
            message: Some("new work".into()),
            items: vec![],
            interrupt: true,
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("closed"));
    controller.state.write().await.children.get_mut("child").unwrap().status = SubagentStatus::Completed;
    assert!(controller.resume("child").await.unwrap_err().to_string().contains("closing"));
    release_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), controller.close("child"))
        .await
        .expect("retry finishes")
        .unwrap();
    assert_eq!(controller.status_for("child").await.unwrap().status, SubagentStatus::Closed);
    assert!(controller.shutdown_handles.lock().await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_restart_keeps_the_previous_launch_owned() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let handle = tokio::spawn(async move {
        let _ = entered_tx.send(());
        let _ = release_rx.recv();
        std::future::pending::<()>().await;
    });
    entered_rx.await.expect("child entered");
    let mut record =
        test_child_record("child", "child-session", "parent-session", &spec, SubagentStatus::Running, 1, None);
    record.handle = Some(handle);
    controller.state.write().await.children.insert("child".into(), record);
    let request = SendInputRequest {
        target: "child".into(),
        message: Some("new work".into()),
        items: vec![],
        interrupt: true,
    };
    assert!(
        tokio::time::timeout(Duration::from_millis(250), controller.send_input(request))
            .await
            .is_err()
    );
    assert!(controller.shutdown_handles.lock().await.contains_key("child"));
    assert!(controller.state.read().await.children["child"].handle.is_none());
    release_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), controller.close("child"))
        .await
        .expect("close finishes")
        .unwrap();
    assert_eq!(controller.status_for("child").await.unwrap().status, SubagentStatus::Closed);
    assert!(controller.shutdown_handles.lock().await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_retains_and_waits_for_an_aborted_child_after_timeout() {
    struct DropSignal(Arc<AtomicBool>);
    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");
    let dropped = Arc::new(AtomicBool::new(false));
    let child_dropped = dropped.clone();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let handle = tokio::spawn(async move {
        let _drop_signal = DropSignal(child_dropped);
        let _ = entered_tx.send(());
        let _ = release_rx.recv();
        std::future::pending::<()>().await;
    });
    entered_rx.await.expect("child entered its non-yielding section");
    let mut record =
        test_child_record("child", "child-session", "parent-session", &spec, SubagentStatus::Running, 1, None);
    record.handle = Some(handle);
    controller.state.write().await.children.insert("child".into(), record);

    assert!(
        tokio::time::timeout(Duration::from_millis(250), controller.signal_shutdown())
            .await
            .is_err()
    );
    assert!(!dropped.load(Ordering::SeqCst));
    assert_eq!(controller.shutdown_handles.lock().await.len(), 1, "the timed-out child remains owned");
    assert_eq!(controller.status_for("child").await.unwrap().status, SubagentStatus::Running);
    release_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), controller.signal_shutdown())
        .await
        .expect("retry observes child exit");
    assert!(dropped.load(Ordering::SeqCst));
    assert!(controller.shutdown_handles.lock().await.is_empty());
    assert_eq!(controller.status_for("child").await.unwrap().status, SubagentStatus::Closed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monitor_shutdown_keeps_the_handle_when_wait_is_cancelled() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    controller.stop_background_completion_monitor().await;
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let monitor = tokio::spawn(async move {
        let _ = entered_tx.send(());
        let _ = release_rx.recv();
        std::future::pending::<()>().await;
    });
    entered_rx.await.expect("monitor entered its non-yielding section");
    *controller.background_completion_monitor.lock().await = Some(monitor);

    assert!(
        tokio::time::timeout(Duration::from_millis(250), controller.stop_background_completion_monitor())
            .await
            .is_err()
    );
    assert!(controller.background_completion_monitor.lock().await.is_some());
    release_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), controller.stop_background_completion_monitor())
        .await
        .expect("retry joins the monitor");
    assert!(controller.background_completion_monitor.lock().await.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_closes_a_controller_attached_before_the_child_exits() {
    let temp = TempDir::new().expect("tempdir");
    let parent = Arc::new(
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("parent controller"),
    );
    let nested = Arc::new(
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("nested controller"),
    );
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");
    nested.state.write().await.children.insert(
        "grandchild".into(),
        test_child_record("grandchild", "grandchild-session", "child-session", &spec, SubagentStatus::Running, 2, None),
    );
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let child = tokio::spawn(async move {
        let _ = entered_tx.send(());
        let _ = release_rx.recv();
        std::future::pending::<()>().await;
    });
    entered_rx.await.unwrap();
    let mut record =
        test_child_record("child", "child-session", "parent-session", &spec, SubagentStatus::Running, 1, None);
    record.handle = Some(child);
    parent.state.write().await.children.insert("child".into(), record);
    let shutdown_parent = parent.clone();
    let shutdown = tokio::spawn(async move { shutdown_parent.signal_shutdown().await });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if parent
                .state
                .read()
                .await
                .children
                .get("child")
                .is_some_and(|record| record.handle.is_none())
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("shutdown requested child abort");
    parent.state.write().await.children.get_mut("child").unwrap().child_controller = Some(nested.clone());
    release_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), shutdown).await.unwrap().unwrap();
    assert_eq!(nested.status_for("grandchild").await.unwrap().status, SubagentStatus::Closed);
    assert_eq!(parent.status_for("child").await.unwrap().status, SubagentStatus::Closed);
}

#[tokio::test]
async fn spawn_is_rejected_after_close_begins() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.subagents.max_depth = 2;
    let mut child_config = test_controller_config(temp.path().to_path_buf(), vt_cfg);
    child_config.depth = 1;
    let child_controller = SubagentController::new(child_config).await.expect("child controller");
    child_controller
        .set_turn_delegation_hints_from_input("delegate this task")
        .await;

    // Mark the controller as closing: this is exactly what `close_tree` does
    // to a child-scoped controller before aborting its subtree, so a still-
    // running child cannot spawn a grandchild after the descendant snapshot.
    child_controller.begin_close().await;

    let err = child_controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("explorer".to_string()),
            message: Some("Inspect the codebase.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect_err("spawn after close began must be rejected");

    assert!(err.to_string().contains("shutting down"));

    // Reopening the subtree clears the transient close flag, so a resumed
    // child can delegate again. Permanent `shutdown_requested` stays set only
    // for real shutdown, not for a subtree close.
    child_controller.end_close().await;
    child_controller
        .spawn(SpawnAgentRequest {
            agent_type: Some("explorer".to_string()),
            message: Some("Inspect the codebase.".to_string()),
            ..SpawnAgentRequest::default()
        })
        .await
        .expect("spawn after end_close must be allowed again");

    // Abort the queued child task so it cannot run provider work after the
    // temp workspace is removed.
    child_controller.signal_shutdown().await;
}
#[tokio::test]
async fn resume_rejected_after_shutdown() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");

    controller.signal_shutdown().await;

    let err = controller
        .resume("some-agent")
        .await
        .expect_err("resume after shutdown must be rejected");

    assert!(err.to_string().contains("shutting down"));
}
#[tokio::test]
async fn resume_restores_closed_grandchildren() {
    let temp = TempDir::new().expect("tempdir");
    let parent = SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
        .await
        .expect("parent controller");

    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");

    // Child-scoped controller that has a grandchild under the child session id.
    let child_controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("child controller");
    let child_session_id = "child-session".to_string();
    {
        let mut state = child_controller.state.write().await;
        state.children.insert(
            "grandchild".to_string(),
            test_child_record(
                "grandchild",
                "session-grandchild",
                &child_session_id,
                &spec,
                SubagentStatus::Running,
                2,
                None,
            ),
        );
    }
    let child_controller = Arc::new(child_controller);
    {
        let mut state = parent.state.write().await;
        state.children.insert(
            "child".to_string(),
            test_child_record(
                "child",
                &child_session_id,
                "parent-session",
                &spec,
                SubagentStatus::Running,
                1,
                Some(child_controller.clone()),
            ),
        );
    }

    // Closing the child cascades to the grandchild...
    parent.close("child").await.expect("close child");
    let closed_grandchild = child_controller.status_for("grandchild").await.expect("grandchild status");
    assert_eq!(closed_grandchild.status, SubagentStatus::Closed, "grandchild must be closed with the child");

    // ...and resuming the child must reopen the grandchild too, not just clear
    // the close gate.
    parent.resume("child").await.expect("resume child");
    let resumed_grandchild = child_controller.status_for("grandchild").await.expect("grandchild status");
    assert!(
        matches!(resumed_grandchild.status, SubagentStatus::Queued | SubagentStatus::Running),
        "resumed grandchild must be re-queued, got {:?}",
        resumed_grandchild.status
    );

    // Abort restarted child tasks so they cannot run provider work after the
    // temp workspace is removed.
    parent.signal_shutdown().await;
}
#[tokio::test]
async fn resume_does_not_affect_sibling_grandchildren() {
    let temp = TempDir::new().expect("tempdir");
    let parent = SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
        .await
        .expect("parent controller");

    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");

    // Two siblings each with their own child-scoped controller and a grandchild.
    let controller_a =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller a");
    {
        let mut state = controller_a.state.write().await;
        state.children.insert(
            "a-grandchild".to_string(),
            test_child_record(
                "a-grandchild",
                "session-a-grandchild",
                "session-a",
                &spec,
                SubagentStatus::Running,
                2,
                None,
            ),
        );
    }
    let controller_b =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller b");
    {
        let mut state = controller_b.state.write().await;
        state.children.insert(
            "b-grandchild".to_string(),
            test_child_record(
                "b-grandchild",
                "session-b-grandchild",
                "session-b",
                &spec,
                SubagentStatus::Running,
                2,
                None,
            ),
        );
    }
    let controller_a = Arc::new(controller_a);
    let controller_b = Arc::new(controller_b);
    {
        let mut state = parent.state.write().await;
        state.children.insert(
            "a".to_string(),
            test_child_record(
                "a",
                "session-a",
                "parent-session",
                &spec,
                SubagentStatus::Running,
                1,
                Some(controller_a.clone()),
            ),
        );
        state.children.insert(
            "b".to_string(),
            test_child_record(
                "b",
                "session-b",
                "parent-session",
                &spec,
                SubagentStatus::Running,
                1,
                Some(controller_b.clone()),
            ),
        );
    }

    // Close both siblings' grandchildren, then resume only "a": "b"'s
    // grandchild must stay closed (resume_tree must respect subtree isolation).
    parent.close("a").await.expect("close a");
    parent.close("b").await.expect("close b");

    parent.resume("a").await.expect("resume a");

    let a_grandchild = controller_a.status_for("a-grandchild").await.expect("a-grandchild status");
    assert!(
        matches!(a_grandchild.status, SubagentStatus::Queued | SubagentStatus::Running),
        "resumed 'a''s grandchild must be re-queued, got {:?}",
        a_grandchild.status
    );
    let b_grandchild = controller_b.status_for("b-grandchild").await.expect("b-grandchild status");
    assert_eq!(
        b_grandchild.status,
        SubagentStatus::Closed,
        "resuming sibling 'a' must not reopen sibling 'b''s grandchildren"
    );

    // Abort restarted child tasks so they cannot run provider work after the
    // temp workspace is removed.
    parent.signal_shutdown().await;
}
#[tokio::test]
async fn wait_returns_first_terminal_child() {
    let temp = TempDir::new().expect("tempdir");
    let controller =
        SubagentController::new(test_controller_config(temp.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .expect("controller");
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "default")
        .expect("default");

    {
        let mut state = controller.state.write().await;
        for id in ["first", "second"] {
            state.children.insert(
                id.to_string(),
                ChildRecord {
                    id: id.to_string(),
                    session_id: format!("session-{id}"),
                    parent_thread_id: "parent-session".to_string(),
                    spec: spec.clone(),
                    display_label: subagent_display_label(&spec),
                    status: SubagentStatus::Running,
                    background: false,
                    depth: 1,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    completed_at: None,
                    summary: None,
                    error: None,
                    archive_metadata: None,
                    archive_path: None,
                    transcript_path: None,
                    effective_config: None,
                    stored_messages: Vec::new(),
                    last_prompt: None,
                    queued_prompts: VecDeque::new(),
                    max_turns: None,
                    model_override: None,
                    reasoning_override: None,
                    thread_handle: None,
                    handle: None,
                    closing: false,
                    notify: Arc::new(Notify::new()),
                    worktree_path: None,
                    child_controller: None,
                },
            );
        }
    }

    let controller_clone = controller.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let mut state = controller_clone.state.write().await;
        let record = state.children.get_mut("second").expect("second child");
        record.status = SubagentStatus::Completed;
        record.summary = Some("done".to_string());
        record.completed_at = Some(Utc::now());
        record.updated_at = Utc::now();
        record.notify.notify_waiters();
    });

    let result = controller
        .wait(&["first".to_string(), "second".to_string()], Some(500))
        .await
        .expect("wait result")
        .expect("terminal child");
    assert_eq!(result.id, "second");
    assert_eq!(result.status, SubagentStatus::Completed);
}
