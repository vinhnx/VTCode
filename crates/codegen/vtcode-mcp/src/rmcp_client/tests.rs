use super::elicitation::{elicitation_response_result, restore_context_meta};
use super::*;
use rmcp::model::{BooleanSchema, ElicitationSchema, PrimitiveSchemaDefinition};

#[test]
fn peer_cache_config_is_bounded_fail_closed_and_partitioned() {
    let config = peer_cache_config("deepwiki");
    assert!(config.enabled);
    assert_eq!(config.max_entries, 128);
    assert!(!config.serve_stale_on_error);
    assert_eq!(config.private_partition.as_deref(), Some("deepwiki"));
}

#[test]
fn discover_preferred_versions_exclude_unsupported_draft_and_remain_newest_first() {
    let versions: Vec<&str> = DISCOVER_PREFERRED_VERSIONS
        .iter()
        .map(rmcp::model::ProtocolVersion::as_str)
        .collect();

    assert_eq!(versions, vec!["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05",]);
    assert!(!versions.contains(&"2026-07-28"));
}

#[test]
fn clamp_initialize_protocol_version_caps_draft_at_last_stable() {
    assert_eq!(
        clamp_initialize_protocol_version(rmcp::model::ProtocolVersion::V_2026_07_28),
        rmcp::model::ProtocolVersion::V_2025_11_25
    );
}

#[test]
fn clamp_initialize_protocol_version_caps_unknown_future_versions() {
    let future: rmcp::model::ProtocolVersion = serde_json::from_value(Value::String("2099-01-01".to_string()))
        .expect("custom protocol versions must deserialize");
    assert_eq!(clamp_initialize_protocol_version(future), rmcp::model::ProtocolVersion::V_2025_11_25);
}

#[test]
fn clamp_initialize_protocol_version_keeps_supported_versions() {
    for version in [
        rmcp::model::ProtocolVersion::V_2025_11_25,
        rmcp::model::ProtocolVersion::V_2025_06_18,
        rmcp::model::ProtocolVersion::V_2025_03_26,
        rmcp::model::ProtocolVersion::V_2024_11_05,
    ] {
        assert_eq!(clamp_initialize_protocol_version(version.clone()), version);
    }
}

#[test]
fn restore_context_meta_adds_request_meta_and_removes_progress_token() {
    let request = form_request(Some(meta(json!({ "existing": true }))));
    let restored = restore_context_meta(
        request,
        meta(json!({
            "persist": "always",
            "progressToken": "token-1"
        })),
    );

    let ElicitRequestParams::FormElicitationParams { meta, .. } = restored else {
        panic!("expected form elicitation request");
    };

    assert_eq!(
        serde_json::to_value(meta.expect("meta should be present")).expect("meta should serialize"),
        json!({
            "existing": true,
            "persist": "always"
        })
    );
}

#[test]
fn elicitation_response_result_serializes_response_meta() {
    let result = ClientResult::CustomResult(
        elicitation_response_result(super::super::McpElicitationResponse {
            action: ElicitationAction::Accept,
            content: Some(json!({ "confirmed": true })),
            meta: Some(json!({ "persist": "always" })),
        })
        .expect("elicitation response should serialize"),
    );

    assert_eq!(
        serde_json::to_value(result).expect("client result should serialize"),
        json!({
            "action": "accept",
            "content": { "confirmed": true },
            "_meta": { "persist": "always" }
        })
    );
}

#[test]
fn list_changed_state_consumes_signals_once() {
    let state = ListChangedState::default();

    assert!(!state.take_tools_changed());
    assert!(!state.take_resources_changed());
    assert!(!state.take_prompts_changed());

    state.mark_tools_changed();
    state.mark_resources_changed();
    state.mark_prompts_changed();

    assert!(state.take_tools_changed());
    assert!(state.take_resources_changed());
    assert!(state.take_prompts_changed());

    assert!(!state.take_tools_changed());
    assert!(!state.take_resources_changed());
    assert!(!state.take_prompts_changed());
}

#[test]
fn list_changed_state_coalesces_bursts_and_bounds_repeated_refreshes() {
    let state = ListChangedState::default();
    let now = std::time::Instant::now();

    for _ in 0..LIST_CHANGED_BUCKET_CAPACITY {
        state.mark_changed(ListChangedNamespace::Tools, now);
        assert!(state.take_changed(ListChangedNamespace::Tools, now));
    }

    // The fifth notification in the same refill interval is suppressed.
    state.mark_changed(ListChangedNamespace::Tools, now);
    assert!(!state.take_changed(ListChangedNamespace::Tools, now));

    // Buckets are independent per notification namespace.
    state.mark_changed(ListChangedNamespace::Resources, now);
    assert!(state.take_changed(ListChangedNamespace::Resources, now));
}

#[test]
fn list_changed_state_refills_after_cooldown() {
    let state = ListChangedState::default();
    let now = std::time::Instant::now();

    for _ in 0..LIST_CHANGED_BUCKET_CAPACITY {
        state.mark_changed(ListChangedNamespace::Prompts, now);
        assert!(state.take_changed(ListChangedNamespace::Prompts, now));
    }
    state.mark_changed(ListChangedNamespace::Prompts, now);
    assert!(!state.take_changed(ListChangedNamespace::Prompts, now));
    assert!(state.take_changed(ListChangedNamespace::Prompts, now + LIST_CHANGED_REFILL_INTERVAL));
}

fn form_request(meta: Option<RequestMetaObject>) -> ElicitRequestParams {
    ElicitRequestParams::FormElicitationParams {
        meta,
        message: "Confirm?".to_string(),
        requested_schema: ElicitationSchema::builder()
            .required_property("confirmed", PrimitiveSchemaDefinition::Boolean(BooleanSchema::new()))
            .build()
            .expect("schema should build"),
    }
}

fn meta(value: Value) -> RequestMetaObject {
    let Value::Object(map) = value else {
        panic!("meta must be an object");
    };
    RequestMetaObject(MetaObject(map))
}

/// Regression test for CVE-2026-64684 (GHSA-9g45-5xwm-f3wc): the MCP
/// streamable-HTTP client must not follow redirects, otherwise
/// caller-supplied custom headers (API keys) leak to the redirect target.
#[tokio::test]
async fn streamable_http_client_does_not_follow_redirects_with_custom_headers() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rmcp_reqwest::header::{HeaderMap, HeaderValue};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut chunk = [0_u8; 1024];
        while let Ok(n) = stream.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.len() > 65536 {
                break;
            }
            let text = String::from_utf8_lossy(&buf);
            let Some(header_end) = text.find("\r\n\r\n") else {
                continue;
            };
            let Some(header_text) = text.get(..header_end) else {
                continue;
            };
            let content_length = header_text
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if buf.len() >= header_end + 4 + content_length {
                break;
            }
        }
        buf
    }

    // Arrange: attacker-controlled capture endpoint. Records hits and
    // whether the custom auth header arrived.
    let hits = Arc::new(AtomicUsize::new(0));
    let leaked = Arc::new(AtomicUsize::new(0));
    let capture_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind capture listener");
    let capture_addr = capture_listener.local_addr().expect("capture addr");
    let capture_task = tokio::spawn({
        let hits = Arc::clone(&hits);
        let leaked = Arc::clone(&leaked);
        async move {
            if let Ok((mut stream, _)) = capture_listener.accept().await {
                let raw = read_http_request(&mut stream).await;
                let _ = hits.fetch_add(1, Ordering::SeqCst);
                if String::from_utf8_lossy(&raw).contains("x-api-key:") {
                    let _ = leaked.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
    });

    // Arrange: compromised MCP endpoint answering 307 to the capture host.
    let redirect_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind redirect listener");
    let redirect_addr = redirect_listener.local_addr().expect("redirect addr");
    let redirect_task = tokio::spawn(async move {
        if let Ok((mut stream, _)) = redirect_listener.accept().await {
            let _ = read_http_request(&mut stream).await;
            let response = format!(
                "HTTP/1.1 307 Temporary Redirect\r\nlocation: http://{capture_addr}/mcp\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            );
            drop(stream.write_all(response.as_bytes()).await);
        }
    });

    // Act: initialize against the redirecting endpoint with a secret header.
    let mut headers = HeaderMap::new();
    drop(headers.insert("x-api-key", HeaderValue::from_static("super-secret")));
    let client = RmcpClient::new_streamable_http_client(
        "redirect-probe".to_string(),
        &format!("http://{redirect_addr}/mcp"),
        None,
        headers,
        None,
    )
    .await
    .expect("client builds");
    let params = InitializeRequestParams::new(
        rmcp::model::ClientCapabilities::default(),
        super::super::utils::build_client_implementation(),
    );
    let result = client
        .initialize(params, Some(Duration::from_secs(10)), ClientLifecycleMode::Initialize)
        .await;

    redirect_task.await.expect("redirect server completes");
    capture_task.abort();

    // Assert: handshake surfaces the 307 as a transport error and the
    // redirect target is never contacted, so the secret cannot leak.
    assert!(result.is_err(), "307 must surface as an error, got {result:?}");
    assert_eq!(hits.load(Ordering::SeqCst), 0, "redirect target must never be contacted");
    assert_eq!(leaked.load(Ordering::SeqCst), 0, "custom auth header must never reach redirect target");
}
