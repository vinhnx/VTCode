use super::*;

#[test]
fn restored_history_retains_indices_for_hidden_runtime_context() {
    use vtcode_core::llm::provider::Message;
    use vtcode_core::utils::session_archive::{COLLAPSED_TOOL_OUTPUT_NOTICE, SessionMessage};

    let messages = [
        SessionMessage::from(&Message::user("hello".into())),
        SessionMessage::from(&Message::turn_scoped_system(COLLAPSED_TOOL_OUTPUT_NOTICE.into())),
        SessionMessage::from(&Message::assistant("answer".into())),
    ];
    let page = restored_history_page(&messages, None, 3);
    assert_eq!(
        page.messages.iter().map(|message| message.archive_index).collect::<Vec<_>>(),
        vec![Some(0), Some(1), Some(2)]
    );
    assert!(!page.messages[0].hidden);
    assert!(page.messages[1].hidden);
    assert!(page.messages[1].text.is_empty());
    assert!(!page.messages[2].hidden);
}
#[test]
fn restored_history_pages_are_bounded_ordered_and_non_overlapping() {
    use vtcode_core::llm::provider::Message;
    use vtcode_core::utils::session_archive::SessionMessage;

    let messages = (0..MAX_RESTORED_MESSAGES + 5)
        .map(|index| SessionMessage::from(&Message::user(format!("message-{index}"))))
        .collect::<Vec<_>>();

    let recent = restored_history_page(&messages, None, MAX_RESTORED_MESSAGES);
    assert_eq!(recent.messages.len(), MAX_RESTORED_MESSAGES);
    assert_eq!(recent.start_index, 5);
    assert!(recent.has_earlier);
    assert_eq!(recent.messages.first().unwrap().archive_index, Some(5));
    assert_eq!(recent.messages.last().unwrap().archive_index, Some(MAX_RESTORED_MESSAGES + 4));

    let earlier = restored_history_page(&messages, Some(recent.start_index), MAX_RESTORED_MESSAGES);
    assert_eq!(earlier.messages.len(), 5);
    assert_eq!(earlier.start_index, 0);
    assert!(!earlier.has_earlier);
    assert_eq!(earlier.messages.first().unwrap().archive_index, Some(0));
    assert_eq!(earlier.messages.last().unwrap().archive_index, Some(4));
}
#[tokio::test]
async fn session_close_excludes_webmcp_turns_until_shutdown_finishes() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, _) = test_service_context(test_startup_context(&workspace), workspace);
    let context = std::sync::Arc::new(context);
    let (input, mut input_receiver) = mpsc::unbounded_channel();
    let (exit_sender, exit) = oneshot::channel();
    let runtime_task = tokio::spawn(std::future::pending::<()>());
    {
        let mut state = context.state.lock().await;
        state.initialized = true;
        state.session_id = Some("current".into());
        state.runtime = Some(RuntimeSession {
            session_id: "current".into(),
            input,
            stop: stop_requests::new_stop_handle_slot(),
            exit,
            abort: runtime_task.abort_handle(),
            command_forwarder: std::thread::spawn(|| {}),
        });
    }
    let close_context = std::sync::Arc::clone(&context);
    let close = tokio::spawn(async move {
        handle_request(
            &close_context,
            &Request {
                jsonrpc: "2.0".into(),
                id: RequestId::String("close".into()),
                method: protocol::method::SESSION_CLOSE.into(),
                params: json!({ "sessionId": "current" }),
            },
        )
        .await
    });
    assert!(matches!(input_receiver.recv().await, Some(InlineEvent::Exit)));
    assert!(!webmcp_status(&context).await.turns_available);
    let error = enqueue_turn(
        &context.state,
        &context.notifications,
        "current",
        "Inspect".into(),
        Vec::new(),
        EnqueueMode::Immediate,
    )
    .await
    .expect_err("a transition must reject a browser turn");
    assert!(error.to_string().contains("session change"));
    exit_sender.send(Ok(())).expect("finish shutdown");
    close.await.expect("close task").expect("close request");
    let state = context.state.lock().await;
    assert!(!state.session_transitioning);
    assert!(state.session_id.is_none());
    drop(state);
    runtime_task.abort();
}
#[tokio::test]
async fn failed_session_transition_releases_the_turn_gate() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, _) = test_service_context(test_startup_context(&workspace), workspace);
    context.state.lock().await.initialized = true;
    let request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("bad-close".into()),
        method: protocol::method::SESSION_CLOSE.into(),
        params: json!({ "sessionId": "missing" }),
    };
    assert!(handle_request(&context, &request).await.is_err());
    assert!(!context.state.lock().await.session_transitioning);
}
#[tokio::test]
async fn shutdown_rechecks_active_turn_before_taking_the_runtime() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, _) = test_service_context(test_startup_context(&workspace), workspace);
    {
        let mut state = context.state.lock().await;
        state.session_id = Some("current".into());
    }
    ensure_no_active_turn(&context).await.expect("initially idle");
    context.state.lock().await.active_turn_id = Some("webmcp-turn".into());

    let error = shutdown_current_session(&context, false)
        .await
        .expect_err("new turn must prevent shutdown");
    assert!(error.to_string().contains("active turn"));
    let state = context.state.lock().await;
    assert_eq!(state.session_id.as_deref(), Some("current"));
    assert_eq!(state.active_turn_id.as_deref(), Some("webmcp-turn"));
}
#[tokio::test]
async fn desktop_mode_snapshot_uses_the_cli_default_and_resolution_fallback() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = vtcode_commons::canonicalize(directory.path()).expect("canonical workspace");
    let mut startup = test_startup_context(&workspace);
    startup.config.default_primary_agent = "auto".into();
    let (context, _) = test_service_context(startup, workspace);
    let agent = active_agent_config(&context).await;
    let snapshot = configuration_snapshot(&context, &agent).await.expect("snapshot");
    let options = snapshot.session_options.expect("options");
    assert_eq!(options.mode, "auto");
    for name in ["build", "plan", "auto", "duck", "coordinator"] {
        assert!(options.available_modes.iter().any(|mode| mode.name == name));
    }
    context.state.lock().await.session_options.mode = Some("missing-agent".into());
    let snapshot = configuration_snapshot(&context, &agent).await.expect("fallback snapshot");
    assert_eq!(snapshot.session_options.expect("fallback options").mode, "build");
}
#[tokio::test]
async fn initialize_exposes_the_desktop_slash_command_catalog() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = vtcode_commons::canonicalize(directory.path()).expect("canonical workspace");
    let (context, _notifications) = test_service_context(test_startup_context(&workspace), workspace.clone());
    let request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("initialize".into()),
        method: protocol::method::INITIALIZE.into(),
        params: serde_json::to_value(protocol::InitializeParams {
            protocol: protocol::ProtocolRange {
                minimum: protocol::ProtocolVersion { major: 1, minor: 0 },
                maximum: protocol::PROTOCOL_VERSION,
            },
            client_name: "desktop-test".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        })
        .expect("initialize params"),
    };

    let (result, should_stop) = handle_request(&context, &request).await.expect("initialize service");
    assert!(!should_stop);
    let initialized: protocol::InitializeResult = serde_json::from_value(result).expect("initialize result");
    let expected = vtcode_core::ui::slash::visible_commands_for_desktop()
        .into_iter()
        .map(|command| protocol::SlashCommandInfo {
            name: command.name.to_string(),
            description: command.description.to_string(),
        })
        .collect::<Vec<_>>();
    assert_eq!(initialized.slash_commands, expected);
    assert!(initialized.slash_commands.iter().any(|command| command.name == "config"));
}
#[tokio::test]
async fn workspace_review_protocol_uses_shared_cli_git_capture() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = vtcode_commons::canonicalize(directory.path()).expect("canonical workspace");
    test_git(&workspace, &["init", "--quiet"]);
    test_git(&workspace, &["config", "user.name", "VT Code test"]);
    test_git(&workspace, &["config", "user.email", "vtcode-test@example.invalid"]);
    std::fs::create_dir(workspace.join("src")).expect("source folder");
    std::fs::write(workspace.join("src/tracked.rs"), "before\n").expect("tracked baseline");
    test_git(&workspace, &["add", "."]);
    test_git(&workspace, &["commit", "--quiet", "-m", "baseline"]);
    std::fs::write(workspace.join("src/tracked.rs"), "after\n").expect("tracked edit");
    std::fs::write(workspace.join("src/new file.rs"), "new source\n").expect("untracked source");
    std::fs::write(workspace.join("outside.txt"), "outside scope").expect("outside-scope file");

    let startup = test_startup_context(&workspace);
    let (context, _notifications) = test_service_context(startup, workspace.clone());
    let initialize = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("initialize".into()),
        method: protocol::method::INITIALIZE.into(),
        params: serde_json::to_value(protocol::InitializeParams {
            protocol: protocol::ProtocolRange {
                minimum: protocol::ProtocolVersion { major: 1, minor: 0 },
                maximum: protocol::PROTOCOL_VERSION,
            },
            client_name: "desktop-test".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        })
        .expect("initialize params"),
    };
    handle_request(&context, &initialize).await.expect("initialize service");

    let review_request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("review-1".into()),
        method: protocol::method::WORKSPACE_REVIEW.into(),
        params: serde_json::to_value(protocol::WorkspaceReviewParams {
            include_untracked: true,
            request_id: "review-1".into(),
            scope: Some("src".into()),
        })
        .expect("review params"),
    };
    let (result, should_stop) = handle_request(&context, &review_request).await.expect("review request");
    assert!(!should_stop);
    let review: protocol::WorkspaceReviewResult = serde_json::from_value(result).expect("review result");
    assert_eq!(review.request_id, "review-1");
    assert_eq!(review.workspace, workspace.to_string_lossy());
    assert_eq!(review.scope, "src");
    assert!(review.include_untracked);
    let patch = review.patch.expect("Git snapshot available");
    assert!(patch.contains("+after"));
    assert!(patch.contains("+++ b/src/new file.rs"));
    assert!(patch.contains("+new source"));
    assert!(!patch.contains("outside scope"));

    let invalid_scope = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("review-2".into()),
        method: protocol::method::WORKSPACE_REVIEW.into(),
        params: serde_json::to_value(protocol::WorkspaceReviewParams {
            include_untracked: false,
            request_id: "review-2".into(),
            scope: Some("../outside".into()),
        })
        .expect("invalid scope params"),
    };
    let (result, _) = handle_request(&context, &invalid_scope)
        .await
        .expect("unavailable review result");
    let invalid: protocol::WorkspaceReviewResult = serde_json::from_value(result).expect("unavailable result");
    assert_eq!(invalid.request_id, "review-2");
    assert!(invalid.patch.is_none(), "an escaped scope must never capture files");
    assert!(invalid.note.contains("Git review unavailable"));
}
