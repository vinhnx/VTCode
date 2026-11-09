use super::*;

#[tokio::test]
async fn update_status_serves_the_cached_check_without_network() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    // Isolate the update cache so an unrelated cached release on the machine
    // running the tests cannot change what this snapshot reports.
    let cache_dir = tempfile::tempdir().expect("cache");
    let previous = crate::updater::set_test_update_cache_dir(Some(cache_dir.path().to_path_buf()));
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
    assert!(!status.update_available, "an empty cache offers no update");
    assert!(status.latest_version.is_none(), "an empty cache has no release to read");
    assert!(status.release_url.is_none());
    // The seen marker is served from the shared update cache, so the client
    // and the CLI agree on whether this release was already announced.
    assert!(!status.current_release_seen, "a fresh cache reports unseen notes");

    crate::updater::set_test_update_cache_dir(previous);
}

/// The changelog viewer reads release notes, which is not the same as being
/// offered an update: a cached release that the check does not offer must
/// still arrive with its version, notes, and URL so the Settings → About row
/// can open it.
#[tokio::test]
async fn update_status_serves_notes_for_a_release_it_is_not_offering() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let cache_dir = tempfile::tempdir().expect("cache");
    let previous = crate::updater::set_test_update_cache_dir(Some(cache_dir.path().to_path_buf()));

    // Seed notes for a version that is deliberately neither the running one
    // nor flagged as newer, so the startup check reports no notice.
    let seeded = semver::Version::parse("99.0.0").expect("seeded version");
    assert_ne!(seeded.to_string(), env!("CARGO_PKG_VERSION"));
    crate::updater::seed_release_notes_for_installed_version(
        &seeded,
        "### Highlights\n- Shiny feature (abc1234)\n- Second item (def5678)\n",
    );

    let (context, _) = test_service_context(test_startup_context(&workspace), workspace);
    context.state.lock().await.initialized = true;
    let request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("update-status-reader".into()),
        method: protocol::method::UPDATE_STATUS.into(),
        params: json!({}),
    };
    let (result, _) = handle_request(&context, &request).await.expect("update status");
    let status: protocol::UpdateStatusSnapshot = serde_json::from_value(result).expect("snapshot");
    assert!(!status.update_available, "the seeded release is not offered as an update");
    assert_eq!(status.latest_version.as_deref(), Some("99.0.0"), "its notes are still readable");
    assert_eq!(status.release_highlights, vec!["Shiny feature".to_string(), "Second item".to_string()]);
    assert_eq!(
        status.release_url.as_deref(),
        Some("https://github.com/vinhnx/vtcode/releases/tag/99.0.0"),
        "the viewer links out to the release it is describing"
    );
    assert!(
        status.current_release_highlights.is_empty(),
        "a release other than the running one must not leak into the what's-new banner"
    );

    crate::updater::set_test_update_cache_dir(previous);
}

/// A cached release at or below the running version must never be advertised
/// as the latest known release: the reader would label a back-version "latest"
/// while the user already runs something newer. Notes for such a release stay
/// reachable through the running version's own slot instead.
#[tokio::test]
async fn update_status_never_labels_an_older_cached_release_as_latest() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let cache_dir = tempfile::tempdir().expect("cache");
    let previous = crate::updater::set_test_update_cache_dir(Some(cache_dir.path().to_path_buf()));

    let running = semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("running version");
    // Step down one minor: this project bumps 0.x minors, so the major never moves.
    let older = semver::Version::new(running.major, running.minor.saturating_sub(1), 0);
    assert!(older < running, "fixture {older} must be older than {running}");
    crate::updater::seed_release_notes_for_installed_version(&older, "### Highlights\n- Old news (abc1234)\n");

    let (context, _) = test_service_context(test_startup_context(&workspace), workspace);
    context.state.lock().await.initialized = true;
    let request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("update-status-older".into()),
        method: protocol::method::UPDATE_STATUS.into(),
        params: json!({}),
    };
    let (result, _) = handle_request(&context, &request).await.expect("update status");
    let status: protocol::UpdateStatusSnapshot = serde_json::from_value(result).expect("snapshot");
    assert!(!status.update_available);
    assert!(
        status.latest_version.is_none(),
        "an older cached release must not be advertised as latest: {:?}",
        status.latest_version
    );
    assert!(status.release_highlights.is_empty(), "nor its notes under that label");

    crate::updater::set_test_update_cache_dir(previous);
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

/// The snapshot carries the live install state, so a client that missed a
/// progress tick (or just launched mid-install) still renders the truth.
#[tokio::test]
#[serial_test::serial]
async fn update_status_carries_the_live_install_state() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    let cache_dir = tempfile::tempdir().expect("cache");
    let previous = crate::updater::set_test_update_cache_dir(Some(cache_dir.path().to_path_buf()));
    update_install::set_test_install_state(protocol::UpdateInstallState::Downloading {
        downloaded: 1024,
        total: Some(2048),
    });

    let (context, _) = test_service_context(test_startup_context(&workspace), workspace);
    context.state.lock().await.initialized = true;
    let request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("update-status-install".into()),
        method: protocol::method::UPDATE_STATUS.into(),
        params: json!({}),
    };
    let (result, _) = handle_request(&context, &request).await.expect("update status");
    let status: protocol::UpdateStatusSnapshot = serde_json::from_value(result).expect("snapshot");
    assert_eq!(
        status.install,
        protocol::UpdateInstallState::Downloading { downloaded: 1024, total: Some(2048) },
        "the snapshot must carry the live install, not a stale Idle"
    );

    update_install::set_test_install_state(protocol::UpdateInstallState::Idle);
    crate::updater::set_test_update_cache_dir(previous);
}

/// A second install while one runs is refused instead of racing two bundle
/// replacements. The guard is exercised without touching the network or the
/// app bundle by seeding the in-flight state directly.
#[tokio::test]
#[serial_test::serial]
async fn update_install_refuses_a_second_concurrent_install() {
    let directory = tempfile::tempdir().expect("workspace");
    let workspace = directory.path().to_path_buf();
    update_install::set_test_install_state(protocol::UpdateInstallState::Installing);

    let (context, _) = test_service_context(test_startup_context(&workspace), workspace);
    context.state.lock().await.initialized = true;
    let request = Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String("update-install-busy".into()),
        method: protocol::method::UPDATE_INSTALL.into(),
        params: json!({}),
    };
    let error = handle_request(&context, &request)
        .await
        .expect_err("concurrent install must be refused");
    assert!(error.to_string().contains("already in progress"), "{error:#}");

    update_install::set_test_install_state(protocol::UpdateInstallState::Idle);
}
