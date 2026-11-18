use super::*;

#[test]
fn startup_interaction_error_is_typed_only_for_turn_submission() {
    for (method, typed) in [
        (protocol::method::TURN_SUBMIT, true),
        (protocol::method::SESSION_CREATE, false),
    ] {
        let mut pending = HashMap::from([(
            RequestId::Number(7),
            PendingRequest::new(method.into(), &json!({ "sessionId": "session" })),
        )]);
        let frame = serde_json::to_vec(&protocol::error_response(
            Some(RequestId::Number(7)),
            protocol::STARTUP_INTERACTION_PENDING_ERROR,
            "waiting",
        ))
        .unwrap();
        let event = decode_frame(&frame, &mut pending).unwrap().unwrap();
        assert_eq!(matches!(event, BackendEvent::TurnBlockedByStartupInteraction { .. }), typed);
    }
}

#[tokio::test]
async fn request_size_preflight_matches_protocol_encoding_at_the_boundary() {
    let method = protocol::method::TURN_SUBMIT;
    let empty = json!({ "prompt": "" });
    let envelope = json!({ "jsonrpc": "2.0", "id": i64::MAX, "method": method, "params": empty });
    let overhead = serde_json::to_vec(&envelope).unwrap().len();
    for (extra, fits) in [(0, true), (1, false)] {
        let params = json!({ "prompt": "a".repeat(protocol::MAX_FRAME_BYTES - overhead + extra) });
        assert_eq!(request_fits_frame(method, &params), fits);
        let request = json!({ "jsonrpc": "2.0", "id": i64::MAX, "method": method, "params": params });
        assert_eq!(protocol::write_frame(&mut tokio::io::sink(), &request).await.is_ok(), fits);
    }
    assert!(!request_fits_frame(method, &json!({ "prompt": "\u{1}".repeat(protocol::MAX_FRAME_BYTES / 5) })));
}

#[test]
fn protocol_responses_are_correlated_with_the_request_method() {
    let mut pending = HashMap::from([(RequestId::Number(9), PendingRequest::new("session/list".into(), &Value::Null))]);
    let event = decode_frame(br#"{"jsonrpc":"2.0","id":9,"result":[]}"#, &mut pending)
        .expect("valid response")
        .expect("decoded response");
    assert!(
        matches!(event, BackendEvent::Response { method, result: Some(_), error: None } if method == "session/list")
    );
    assert!(pending.is_empty());
}

#[test]
fn workspace_review_errors_retain_request_identity() {
    let mut pending = HashMap::from([(
        RequestId::Number(12),
        PendingRequest::new(protocol::method::WORKSPACE_REVIEW.into(), &json!({ "requestId": "review-old" })),
    )]);
    let event = decode_frame(br#"{"jsonrpc":"2.0","id":12,"error":{"message":"review failed"}}"#, &mut pending)
        .expect("valid response")
        .expect("decoded response");
    assert!(
        matches!(event, BackendEvent::WorkspaceReviewResponse { request_id, result: None, error: Some(error) }
        if request_id == "review-old" && error == "review failed")
    );
}

#[test]
fn session_history_responses_retain_the_page_request_identity() {
    let mut pending = HashMap::from([(
        RequestId::Number(10),
        PendingRequest::new(
            protocol::method::SESSION_HISTORY.into(),
            &json!({ "requestId": "page-current", "sessionId": "session" }),
        ),
    )]);
    let event = decode_frame(br#"{"jsonrpc":"2.0","id":10,"error":{"message":"page failed"}}"#, &mut pending)
        .expect("valid response")
        .expect("decoded response");
    assert!(matches!(
        event,
        BackendEvent::SessionHistoryResponse { request_id, result: None, error: Some(error) }
            if request_id == "page-current" && error == "page failed"
    ));
}

#[test]
fn session_output_responses_retain_the_page_request_identity() {
    let mut pending = HashMap::from([(
        RequestId::Number(11),
        PendingRequest::new(
            protocol::method::SESSION_OUTPUT_READ.into(),
            &json!({ "requestId": "output-current", "itemId": "tool-output" }),
        ),
    )]);
    let event = decode_frame(br#"{"jsonrpc":"2.0","id":11,"error":{"message":"page failed"}}"#, &mut pending)
        .expect("valid response")
        .expect("decoded response");
    assert!(matches!(
        event,
        BackendEvent::SessionOutputResponse { request_id, result: None, error: Some(error) }
            if request_id == "output-current" && error == "page failed"
    ));
}

#[test]
fn mcp_app_responses_retain_the_read_request_identity() {
    let mut pending = HashMap::from([(
        RequestId::Number(12),
        PendingRequest::new(
            protocol::method::MCP_APP_READ.into(),
            &json!({ "requestId": "app-current", "uri": "ui://demo/app.html" }),
        ),
    )]);
    let event = decode_frame(br#"{"jsonrpc":"2.0","id":12,"result":{"uri":"ui://demo/app.html"}}"#, &mut pending)
        .expect("valid response")
        .expect("decoded response");
    assert!(matches!(
        event,
        BackendEvent::McpAppResponse { request_id, result: Some(_), error: None }
            if request_id == "app-current"
    ));
}

#[test]
fn interaction_replies_retain_request_identity_when_responses_arrive_out_of_order() {
    let request = |interaction_id: &str, cancelled| {
        PendingRequest::new(
            protocol::method::INTERACTION_RESPOND.into(),
            &json!({ "sessionId": "session", "turnId": "turn", "interactionId": interaction_id, "cancelled": cancelled }),
        )
    };
    let mut pending = HashMap::from([
        (RequestId::Number(7), request("old-question", false)),
        (RequestId::Number(8), request("new-question", true)),
    ]);
    let rejected = decode_frame(br#"{"jsonrpc":"2.0","id":8,"error":{"message":"Retry this decision"}}"#, &mut pending)
        .expect("valid rejection")
        .expect("correlated event");
    assert!(matches!(rejected, BackendEvent::InteractionResponded { identity, error: Some(error) }
        if identity.interaction_id == "new-question" && identity.session_id == "session"
            && identity.turn_id == "turn" && identity.cancelled && error == "Retry this decision"));
    let accepted = decode_frame(br#"{"jsonrpc":"2.0","id":7,"result":{}}"#, &mut pending)
        .expect("valid acceptance")
        .expect("correlated event");
    assert!(matches!(accepted, BackendEvent::InteractionResponded { identity, error: None }
        if identity.interaction_id == "old-question" && !identity.cancelled));
    assert!(pending.is_empty());
}

#[test]
fn protocol_notifications_keep_authoritative_event_payloads() {
    let mut pending = HashMap::new();
    let frame = br#"{"jsonrpc":"2.0","method":"turn/status","params":{"status":"running","turnId":"t-1"}}"#;
    let event = decode_frame(frame, &mut pending)
        .expect("valid notification")
        .expect("decoded notification");
    assert!(matches!(event, BackendEvent::TurnStatus(value) if value["turnId"] == "t-1"));
}

#[test]
fn primary_agent_notifications_are_typed_and_session_scoped() {
    let mut pending = HashMap::new();
    let frame = br#"{"jsonrpc":"2.0","method":"session/primaryAgentChanged","params":{"sessionId":"session-a","displayName":"Plan"}}"#;
    let event = decode_frame(frame, &mut pending)
        .expect("valid notification")
        .expect("decoded notification");
    assert!(matches!(
        event,
        BackendEvent::PrimaryAgentChanged(protocol::PrimaryAgentChanged { session_id, display_name: Some(name) })
            if session_id == "session-a" && name == "Plan"
    ));
}

#[test]
fn null_id_service_errors_surface_their_message() {
    let mut pending = HashMap::new();
    let frame = serde_json::to_vec(&protocol::error_response(None, -32700, "invalid JSON-RPC message")).unwrap();
    let event = decode_frame(&frame, &mut pending).expect("null-id error must decode");
    assert!(matches!(event, Some(BackendEvent::LaunchFailed(message)) if message.contains("invalid JSON-RPC")));
    assert!(pending.is_empty());
}

#[test]
fn malformed_protocol_frames_fail_closed_and_unknown_notifications_are_ignored() {
    let mut pending = HashMap::new();
    assert!(matches!(decode_frame(b"not-json", &mut pending), Err(ProtocolDecodeError)));
    assert!(matches!(
        decode_frame(br#"{"jsonrpc":"2.0","method":"unknown/event","params":{}}"#, &mut pending),
        Ok(None)
    ));
}

#[test]
fn startup_auth_diagnostics_are_classified_without_exposing_raw_stderr() {
    assert!(missing_credentials_diagnostic(b"Authentication not found for OpenAI. Run `vtcode login openai`."));
    assert!(missing_credentials_diagnostic(b"API key not found for provider 'example'."));
    assert!(!missing_credentials_diagnostic(b"Connection refused while contacting the configured endpoint."));
}

#[test]
fn packaged_apps_only_resolve_their_bundled_backend() {
    let candidates = packaged_backend_candidates(Path::new("/Applications/VT Code.app/Contents/MacOS"));
    assert_eq!(candidates, Some(vec![PathBuf::from("/Applications/VT Code.app/Contents/Helpers/vtcode")]));
}

#[test]
fn development_builds_keep_adjacent_and_workspace_backend_fallbacks() {
    let candidates = backend_development_candidates(Path::new("/workspace/target/debug"), Path::new("/workspace"));
    assert_eq!(
        candidates,
        [
            PathBuf::from("/workspace/target/debug/vtcode"),
            PathBuf::from("/workspace/target/release/vtcode"),
        ]
    );
    assert!(packaged_backend_candidates(Path::new("/workspace/target/debug")).is_none());
}
