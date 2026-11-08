use super::*;

#[tokio::test]
async fn update_status_serves_the_cached_check_without_network() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let (context, _) = test_service_context(test_startup_context(&workspace), workspace);
    context.state.lock().await.initialized = true;
    let request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("update-status".into()),
        method: protocol::method::UPDATE_STATUS.into(),
        params: json!({}),
    };
    let (result, shutdown) = handle_request(&context, &request).await.expect("update status");
    assert!(!shutdown);
    let status: protocol::UpdateStatusSnapshot = serde_json::from_value(result).expect("snapshot");
    assert_eq!(status.current_version, env!("CARGO_PKG_VERSION"));
    assert!(status.channel.is_some());
    if status.update_available {
        assert!(status.latest_version.is_some());
        assert!(status.release_url.as_deref().is_some_and(|url| url.starts_with("https://")));
    } else {
        assert!(status.latest_version.is_none());
        assert!(status.release_url.is_none());
    }
    // The seen marker is served from the shared update cache, so the client
    // and the CLI agree on whether this release was already announced.
    assert_eq!(status.current_release_seen, crate::updater::current_version_notes_seen());
}

#[tokio::test]
async fn update_acknowledge_records_the_running_version_as_seen() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    // Isolate the update cache: the thread-local override keeps this test off
    // the real user cache and its marker.
    let cache_dir = tempfile::tempdir().expect("cache");
    let previous = crate::updater::set_test_update_cache_dir(Some(cache_dir.path().to_path_buf()));
    let (context, _) = test_service_context(test_startup_context(&workspace), workspace);
    context.state.lock().await.initialized = true;

    let request = |method: &str, id: &str| Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String(id.into()),
        method: method.into(),
        params: json!({}),
    };

    let (before, _) = handle_request(&context, &request(protocol::method::UPDATE_STATUS, "status-1"))
        .await
        .expect("update status");
    let before: protocol::UpdateStatusSnapshot = serde_json::from_value(before).expect("snapshot");
    assert!(!before.current_release_seen, "a fresh cache reports unseen notes");

    let (after, _) = handle_request(&context, &request(protocol::method::UPDATE_ACKNOWLEDGE, "ack"))
        .await
        .expect("update acknowledge");
    let after: protocol::UpdateStatusSnapshot = serde_json::from_value(after).expect("snapshot");
    assert!(after.current_release_seen, "acknowledge records the shared seen marker");
    assert_eq!(after.current_version, before.current_version, "only the seen marker moves");

    crate::updater::set_test_update_cache_dir(previous);
}
