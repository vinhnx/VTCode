use super::*;

/// Foreign flock hold on a session lock, mirroring vtcode-memory's own
/// exclusivity tests: a separate open file description conflicts even
/// in-process, which is exactly the cross-process shape the load probe
/// guards against.
fn hold_session_lock_externally(workspace: &Path, session_id: &str) -> std::fs::File {
    use fs2::FileExt;
    // Establish the session dir + lock file first (dropped = released).
    drop(
        vtcode_memory::open_exclusive(workspace, session_id, vtcode_memory::DEFAULT_MAX_EVENTS)
            .expect("establish session dir"),
    );
    let lock_path = vtcode_memory::session_directory(workspace, session_id).join("session.lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("lock file must exist");
    file.try_lock_exclusive().expect("test must establish the foreign hold");
    file
}

fn load_request(session_id: &str) -> Request {
    Request {
        jsonrpc: "2.0".into(),
        id: RequestId::Number(1),
        method: protocol::method::SESSION_LOAD.into(),
        params: serde_json::to_value(protocol::SessionIdParams { session_id: session_id.into() }).expect("params"),
    }
}

#[tokio::test]
async fn session_load_fails_fast_when_another_process_holds_the_session() {
    let workspace = tempfile::tempdir().expect("workspace");
    let startup = test_startup_context(workspace.path());
    let (context, _) = test_service_context(startup, workspace.path().to_path_buf());
    let context = std::sync::Arc::new(context);
    context.state.lock().await.initialized = true;
    let _held = hold_session_lock_externally(workspace.path(), "sess-held");

    let error = handle_request(&context, &load_request("sess-held"))
        .await
        .expect_err("held session must fail fast");
    assert!(error.to_string().contains("already open in another process"), "names contention: {error:#}");
    assert!(error.to_string().contains("close it there and reload"), "names remedy: {error:#}");
    // The fork remedy must be named too: a reader who wants its own copy
    // cannot use the close-and-reload advice alone.
    assert!(error.to_string().contains("vtcode --fork-session sess-held"), "names the copy remedy: {error:#}");

    // No runtime installed, no transition left behind: the next load retries cleanly.
    let state = context.state.lock().await;
    assert!(state.runtime.is_none());
    assert!(!state.session_transitioning);
}

#[tokio::test]
async fn session_load_without_a_holder_reaches_archive_lookup() {
    let workspace = tempfile::tempdir().expect("workspace");
    let startup = test_startup_context(workspace.path());
    let (context, _) = test_service_context(startup, workspace.path().to_path_buf());
    let context = std::sync::Arc::new(context);
    context.state.lock().await.initialized = true;

    // Unique id: the global archive lookup must miss without touching real sessions.
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix time")
        .as_nanos();
    let fake_id = format!("nonexistent-load-probe-{unique}");
    let error = handle_request(&context, &load_request(&fake_id))
        .await
        .expect_err("missing archive must fail");
    assert!(error.to_string().contains("saved session was not found"), "probe passed, lookup ran: {error:#}");
    assert!(
        !vtcode_memory::session_directory(workspace.path(), &fake_id).exists(),
        "a rejected load must not litter a session directory"
    );
}

/// A CLI handoff is a fork: the desktop app receives a copy it can own
/// outright, so it loads while the CLI still holds the live thread's lock.
/// This is the end of the reported failure, where the desktop app could
/// never load the handed-over thread.
#[tokio::test]
#[serial_test::serial(session_dir_override)]
async fn a_forked_handoff_loads_while_the_cli_holds_the_live_thread() {
    let sessions = IsolatedSessionStore::new();
    let workspace = tempfile::tempdir().expect("workspace");
    // The service canonicalizes its workspace before matching it against a
    // session's recorded workspace, and a macOS temp dir resolves through a
    // symlink (`/var` -> `/private/var`).
    let canonical_workspace = vtcode_commons::paths::canonicalize(workspace.path()).expect("canonical workspace");
    let startup = test_startup_context(workspace.path());
    let (context, _) = test_service_context(startup, canonical_workspace.clone());
    let context = std::sync::Arc::new(context);
    context.state.lock().await.initialized = true;

    let live_id = "handoff-live-session";
    let messages = vec![vtcode_core::utils::session_archive::SessionMessage::new(
        vtcode_core::llm::provider::MessageRole::User,
        "hello",
    )];
    seed_archive(sessions.path(), live_id, &canonical_workspace, messages.clone()).await;

    // The CLI owns the live thread: it holds the canonical event-log lock.
    let _held = hold_session_lock_externally(workspace.path(), live_id);

    // Forking is what the CLI's `/desktop` command does before handing over.
    let listing = find_session_by_identifier(live_id)
        .await
        .expect("find live listing")
        .expect("live archive");
    let (fork_id, _archive) = vtcode_core::core::threads::fork_thread_for_handoff(
        &listing,
        canonical_workspace,
        listing.snapshot.metadata.clone(),
    )
    .await
    .expect("fork thread");
    assert_ne!(fork_id, live_id, "the fork owns a new id");

    // The fork loads: it is a different session, so the live thread's lock
    // does not block it.
    let loaded = handle_request(&context, &load_request(&fork_id))
        .await
        .expect("the forked handoff must load while the CLI holds the live thread");
    let result = loaded.0;
    assert_eq!(result["session"]["sessionId"], fork_id, "the fork is the selected thread");
    assert_eq!(result["messages"].as_array().map(Vec::len), Some(1), "the fork carries the transcript");
}
