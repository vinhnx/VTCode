use super::*;
use crate::FilesystemWorkspace;
use tempfile::TempDir;

#[tokio::test]
async fn explanation_requests_require_origin_bound_tokens_and_report_unsupported() {
    let temp = TempDir::new().expect("workspace");
    let adapter = Arc::new(FilesystemWorkspace::new(temp.path(), [], false).await.expect("adapter"));
    let origin = "https://example.test";
    let server = WebmcpServer::new(
        adapter,
        WebmcpServerConfig {
            allowed_origins: vec![origin.to_owned(), "https://other.test".to_owned()],
            ..Default::default()
        },
    )
    .expect("server");
    let pairing = server.begin_pairing_for_origin(origin).expect("pairing");
    let session = server
        .state
        .pairing
        .pair(pairing.code(), origin)
        .expect("authenticated session");
    let token = session.token().to_owned();
    let reference = vtcode_memory::explanation::EvidenceRef {
        session_id: "session".into(),
        offset: 0,
        length: 1,
        digest: "0".repeat(64),
        item_id: None,
    };
    let requests = [
        BridgeRequest::ExplanationGet {
            request_id: "get".into(),
            token: token.clone(),
            scope: vtcode_memory::explanation::ExplanationScope::Task,
            offset: 0,
        },
        BridgeRequest::ExplanationEvidence {
            request_id: "evidence".into(),
            token: token.clone(),
            reference: reference.clone(),
            offset: 0,
        },
        BridgeRequest::ExplanationNavigate {
            request_id: "navigate".into(),
            token: token.clone(),
            reference,
        },
    ];
    for request in &requests {
        let dispatch = || Arc::clone(&server.state.dispatch);
        assert!(matches!(
            dispatch_request(dispatch(), origin.into(), "wrong-token".into(), request.clone()).await,
            Err(WebmcpError::Unauthorized)
        ));
        let error = dispatch_request(dispatch(), origin.into(), token.clone(), request.clone())
            .await
            .expect_err("headless adapter does not provide explanations");
        assert!(matches!(error, WebmcpError::Unsupported(_)));
        assert_eq!(response_for_error("operation", error).error.expect("error payload").code, "unsupported");
    }
    assert!(matches!(
        dispatch_request(
            Arc::clone(&server.state.dispatch),
            "https://other.test".into(),
            token.clone(),
            requests[0].clone()
        )
        .await,
        Err(WebmcpError::Unauthorized)
    ));
    for request in requests {
        assert!(matches!(
            dispatch_request(Arc::clone(&server.state.dispatch), origin.into(), token.clone(), request).await,
            Err(WebmcpError::Unauthorized)
        ));
    }
    let status = server.state.adapter.status().await.expect("status");
    assert!(!status.explanations_available);
    assert!(!status.turns_available);
    assert!(std::fs::read_dir(temp.path()).expect("workspace files").next().is_none());
}

#[tokio::test]
async fn server_requires_explicit_origins_and_remote_flags() {
    let temp = TempDir::new().expect("temp dir");
    let adapter = Arc::new(FilesystemWorkspace::new(temp.path(), [], false).await.expect("adapter"));
    assert!(matches!(
        WebmcpServer::new(adapter.clone(), WebmcpServerConfig::default()),
        Err(WebmcpError::InvalidRequest(_))
    ));
    let config = WebmcpServerConfig {
        host: "0.0.0.0".to_string(),
        allowed_origins: vec!["https://example.test".to_string()],
        ..Default::default()
    };
    assert!(matches!(WebmcpServer::new(adapter, config), Err(WebmcpError::InvalidRequest(_))));

    let remote_config = WebmcpServerConfig {
        host: "0.0.0.0".to_string(),
        allowed_origins: vec!["https://example.test".to_string()],
        allow_remote: true,
        public_url: Some("wss://bridge.example.test/webmcp".to_string()),
        ..Default::default()
    };
    let temp = TempDir::new().expect("temp dir");
    let adapter = Arc::new(FilesystemWorkspace::new(temp.path(), [], false).await.expect("adapter"));
    assert!(matches!(WebmcpServer::new(adapter, remote_config), Err(WebmcpError::InvalidRequest(_))));

    let temp = TempDir::new().expect("temp dir");
    let adapter = Arc::new(FilesystemWorkspace::new(temp.path(), [], false).await.expect("adapter"));
    let valid_proxy_config = WebmcpServerConfig {
        allowed_origins: vec!["https://example.test".to_string()],
        allow_remote: true,
        public_url: Some("wss://bridge.example.test/webmcp".to_string()),
        ..Default::default()
    };
    assert!(WebmcpServer::new(adapter.clone(), valid_proxy_config).is_ok());
    let invalid_public_url_config = WebmcpServerConfig {
        allowed_origins: vec!["https://example.test".to_string()],
        public_url: Some("ws://bridge.example.test/webmcp".to_string()),
        ..Default::default()
    };
    assert!(matches!(WebmcpServer::new(adapter, invalid_public_url_config), Err(WebmcpError::InvalidRequest(_))));

    let temp = TempDir::new().expect("temp dir");
    let adapter = Arc::new(FilesystemWorkspace::new(temp.path(), [], false).await.expect("adapter"));
    let invalid_public_url_config = WebmcpServerConfig {
        allowed_origins: vec!["https://example.test".to_string()],
        allow_remote: true,
        public_url: Some("wss://".to_string()),
        ..Default::default()
    };
    assert!(matches!(WebmcpServer::new(adapter, invalid_public_url_config), Err(WebmcpError::InvalidRequest(_))));
}
