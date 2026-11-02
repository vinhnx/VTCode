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
}
