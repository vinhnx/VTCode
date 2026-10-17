//! Shared mock-server helpers for provider tests.
//!
//! `wiremock` cannot bind a local socket in every sandbox, so callers use
//! [`start_mock_server_or_skip`] and return early when it yields `None` instead
//! of failing the suite.

/// Extract a panic payload's message so a sandbox bind failure can be told
/// apart from a real test failure.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    "unknown panic".to_string()
}

/// Start a `wiremock` server, or return `None` when the sandbox forbids binding
/// a local socket so the calling test can skip itself.
pub(crate) async fn start_mock_server_or_skip() -> Option<wiremock::MockServer> {
    match tokio::spawn(async { wiremock::MockServer::start().await }).await {
        Ok(server) => Some(server),
        Err(err) if err.is_panic() => {
            let message = panic_message(err.into_panic());
            if message.contains("Operation not permitted") || message.contains("PermissionDenied") {
                return None;
            }
            panic!("mock server should start: {message}");
        }
        Err(err) => panic!("mock server task should complete: {err}"),
    }
}
