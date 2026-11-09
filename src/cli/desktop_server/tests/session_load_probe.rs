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
