use super::*;
use std::io::{Read, Write};

const SUCCESS: &str = r#"{"code":0,"data":{"results":[{"title":"Rust Book","url":"https://doc.rust-lang.org/book/","highlight":"Ownership and borrowing"},{"title":"Tokio","url":"https://tokio.rs/","highlight":"Async tasks"}]}}"#;

fn fixture(status: u16, body: &str, extra_headers: &str) -> (String, std::thread::JoinHandle<String>) {
    fixture_response(
        format!(
            "HTTP/1.1 {status} fixture\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n{body}",
            body.len()
        ),
        Duration::ZERO,
    )
}

fn fixture_response(response: String, delay: Duration) -> (String, std::thread::JoinHandle<String>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/search", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..index]).to_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .unwrap_or("0")
                    .parse()
                    .unwrap();
                if bytes.len() >= index + 4 + length {
                    break;
                }
            }
        }
        std::thread::sleep(delay);
        let _ = stream.write_all(response.as_bytes());
        String::from_utf8(bytes).unwrap()
    });
    (endpoint, server)
}

fn request(endpoint: &str, timeout: u64) -> reqwest::RequestBuilder {
    build_request(&build_client(timeout).unwrap(), endpoint, "rust ownership", 2, "fixture-secret").unwrap()
}

#[tokio::test]
async fn octen_request_maps_ranked_highlights_and_disables_full_content() {
    let (endpoint, server) = fixture(200, SUCCESS, "");
    let results = request_results(request(&endpoint, 5), 1).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Rust Book");
    assert_eq!(results[0].snippet, "Ownership and borrowing");
    let wire = server.join().unwrap();
    assert!(wire.starts_with("POST /search HTTP/1.1"));
    assert!(wire.contains("x-api-key: fixture-secret\r\n"));
    let payload: Value = serde_json::from_str(wire.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        payload,
        json!({"query":"rust ownership","count":2,"highlight":{"enable":true},"full_content":{"enable":false},"format":"text"})
    );
    let debug = format!("{:?}", request(&endpoint, 5).build().unwrap());
    assert!(!debug.contains("fixture-secret"));
}

#[tokio::test]
async fn octen_http_failures_and_redirects_are_distinct_and_redacted() {
    for (status, kind) in [
        (401, "credentials_rejected"),
        (403, "credentials_rejected"),
        (429, "rate_limited"),
        (503, "api_error"),
        (302, "redirect_rejected"),
    ] {
        let (endpoint, server) = fixture(status, "fixture-secret", "Location: http://127.0.0.1:1/leak\r\n");
        let error = request_results(request(&endpoint, 5), 2).await.err().unwrap();
        assert_eq!(error.kind, kind);
        assert!(!format!("{error:?}").contains("fixture-secret"));
        server.join().unwrap();
    }
}

#[tokio::test]
async fn octen_response_limit_checks_declared_and_streamed_bytes() {
    for response in [
        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", MAX_BODY_BYTES + 1),
        format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
            MAX_BODY_BYTES + 1,
            "x".repeat(MAX_BODY_BYTES + 1)
        ),
    ] {
        let (endpoint, server) = fixture_response(response, Duration::ZERO);
        assert_eq!(request_results(request(&endpoint, 5), 2).await.err().unwrap().kind, "response_too_large");
        server.join().unwrap();
    }
}

#[tokio::test]
async fn octen_timeout_and_network_failures_do_not_leak_transport_details() {
    let (endpoint, server) =
        fixture_response("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_owned(), Duration::from_millis(1300));
    assert_eq!(request_results(request(&endpoint, 1), 2).await.err().unwrap().kind, "timeout");
    server.join().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/", listener.local_addr().unwrap());
    drop(listener);
    assert_eq!(request_results(request(&endpoint, 1), 2).await.err().unwrap().kind, "network_error");
}

#[test]
fn octen_payload_errors_are_not_empty_successes() {
    for body in [
        r#"{}"#,
        r#"{"code":0}"#,
        r#"{"code":0,"data":{}}"#,
        r#"{"code":0,"data":{"results":null}}"#,
        "invalid fixture-secret",
        r#"{"code":0,"data":{"results":[{"title":"unsafe","url":"javascript:alert(1)"}]}}"#,
    ] {
        assert_eq!(parse_response(body.as_bytes(), 2).err().unwrap().kind, "malformed_response");
    }
    for (code, kind) in [
        (401, "credentials_rejected"),
        (403, "credentials_rejected"),
        (429, "rate_limited"),
        (23, "api_error"),
    ] {
        let body = json!({"code":code,"msg":"fixture-secret"}).to_string();
        let error = parse_response(body.as_bytes(), 2).err().unwrap();
        assert_eq!(error.kind, kind);
        assert!(!format!("{error:?}").contains("fixture-secret"));
    }
    assert!(parse_response(br#"{"code":0,"data":{"results":[]}}"#, 2).unwrap().is_empty());
}

#[test]
fn octen_urls_and_unicode_fields_are_bounded() {
    for url in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "data:text/html,hi",
        "https://user:secret@example.com/",
        "https:///",
        "relative",
    ] {
        assert!(validate_result_url(url).is_none(), "{url}");
    }
    let body = json!({"code":0,"data":{"results":[
        {"title":"bad", "url":"javascript:alert(1)"},
        {"title":"界".repeat(205),"url":"https://example.com/b","highlight":"é".repeat(410)},
        {"title":"A","url":"http://example.com/a","highlight":"A excerpt"}
    ]}})
    .to_string();
    let results = parse_response(body.as_bytes(), 20).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].url, "https://example.com/b");
    assert_eq!(results[0].title, "界".repeat(200) + "…");
    assert_eq!(results[0].snippet, "é".repeat(400) + "…");
    assert_eq!(results[1].snippet, "A excerpt");
}

#[tokio::test]
async fn octen_quota_counts_failures_and_does_not_cache_empty_or_failed_responses() {
    let config = WebSearchConfig {
        cooldown_ms: 0,
        session_max_requests: 3,
        ..Default::default()
    };
    let tool = WebSearchTool::with_config(config.clone());
    for (status, body) in [
        (401, "secret"),
        (200, r#"{"code":0,"data":{"results":[]}}"#),
        (200, SUCCESS),
    ] {
        let (endpoint, server) = fixture(status, body, "");
        let payload = tool
            .send_search(request(&endpoint, 5), "rust", 2, "2::rust", &config)
            .await
            .unwrap();
        server.join().unwrap();
        let state = tool.state.lock().unwrap();
        if status == 401 {
            assert_eq!(payload["error_type"], "credentials_rejected");
        }
        assert_eq!(state.cache.len(), usize::from(body == SUCCESS));
    }
    assert_eq!(tool.state.lock().unwrap().requests_made, 3);
    let capped = tool
        .send_search(request("http://127.0.0.1:1/", 1), "new", 2, "2::new", &config)
        .await
        .unwrap();
    assert_eq!(capped["session_max_requests"], 3);
    let cached = tool.run(json!({"query":"rust","max_results":2})).await.unwrap();
    assert_eq!(cached["cached"], true);
    assert_eq!(cached["provider"], "octen");
    assert_eq!(tool.state.lock().unwrap().requests_made, 3);
}

#[tokio::test]
async fn octen_concurrent_admission_is_atomic_and_cooldown_consumes_no_quota() {
    let config = WebSearchConfig {
        cooldown_ms: 5000,
        session_max_requests: 3,
        ..Default::default()
    };
    let tool = WebSearchTool::with_config(config.clone());
    let (endpoint, server) = fixture(200, SUCCESS, "");
    let first = tool.send_search(request(&endpoint, 5), "rust", 2, "2::rust", &config);
    let second = tool.send_search(request("http://127.0.0.1:1/", 1), "other", 2, "2::other", &config);
    let (a, b) = tokio::join!(first, second);
    assert_eq!(a.unwrap()["count"], 2);
    assert_eq!(b.unwrap()["error"], "web_search cooldown active");
    server.join().unwrap();
    assert_eq!(tool.state.lock().unwrap().requests_made, 1);
    let mut state = tool.state.lock().unwrap();
    let entry = state.cache.get_mut("2::rust").unwrap();
    entry.stored_at = Instant::now().checked_sub(Duration::from_secs(301)).unwrap();
    assert!(state.cache_get("2::rust", Duration::from_secs(300)).is_none());
}

#[tokio::test]
async fn octen_arguments_reject_missing_blank_and_unknown_fields() {
    let tool = WebSearchTool::new();
    for args in [
        json!({}),
        json!({"query":" "}),
        json!({"query":"x".repeat(501)}),
        json!({"query":"rust","pattern":"legacy"}),
    ] {
        assert!(tool.run(args).await.is_err());
    }
    assert!(serde_json::from_value::<WebSearchArgs>(json!({"query":"rust","max_results":20})).is_ok());
}

#[tokio::test]
async fn octen_missing_credentials_and_storage_errors_consume_no_quota_or_cache() {
    let tool = WebSearchTool::new();
    let missing = tool.run_with_credentials(json!({"query":"rust"}), || Ok(None)).await.unwrap();
    assert_eq!(missing["error_type"], "setup_error");
    assert!(missing["next_action"].as_str().unwrap().contains("/secret add octen"));
    let failed = tool
        .run_with_credentials(json!({"query":"rust"}), || Err(anyhow!("fixture-secret backend error")))
        .await
        .unwrap();
    assert_eq!(failed["error_type"], "credential_storage_error");
    assert!(!failed.to_string().contains("fixture-secret"));
    let state = tool.state.lock().unwrap();
    assert_eq!(state.requests_made, 0);
    assert!(state.last_request_at.is_none());
    assert!(state.cache.is_empty());
}

#[tokio::test]
async fn octen_cancelled_send_still_consumes_quota() {
    let tool = WebSearchTool::with_config(WebSearchConfig {
        cooldown_ms: 0,
        session_max_requests: 1,
        ..Default::default()
    });
    let config = tool.config.lock().unwrap().clone();
    let (endpoint, server) = fixture_response(
        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{SUCCESS}", SUCCESS.len()),
        Duration::from_millis(200),
    );
    let send = tool.send_search(request(&endpoint, 5), "rust", 2, "2::rust", &config);
    assert!(tokio::time::timeout(Duration::from_millis(100), send).await.is_err());
    server.join().unwrap();
    let state = tool.state.lock().unwrap();
    assert_eq!(state.requests_made, 1);
    assert!(state.cache.is_empty());
}

#[tokio::test]
#[ignore = "explicit paid Octen pilot; at most five requests using existing credentials"]
async fn octen_live_coding_query_pilot() {
    let manager = vtcode_config::loader::ConfigManager::load_from_workspace(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3).unwrap(),
    )
    .expect("load pilot credential storage mode");
    let storage_mode = manager.config().agent.credential_storage_mode;
    match resolve_octen_credential_with_mode(storage_mode) {
        Ok(Some(_)) => {}
        Ok(None) => {
            println!("Live pilot UNVERIFIED: no Octen credential available");
            return;
        }
        Err(_) => {
            println!("Live pilot UNVERIFIED: secure storage unavailable");
            return;
        }
    }
    let tool = WebSearchTool::with_config_and_storage(
        WebSearchConfig {
            cooldown_ms: 0,
            session_max_requests: 5,
            ..Default::default()
        },
        storage_mode,
    );
    for (query, useful_host) in [
        ("Rust book ownership borrowing", "rust-lang.org"),
        ("Tokio cancellation safety select documentation", "tokio.rs"),
        ("reqwest redirect Policy none docs", "docs.rs"),
        ("serde enum deserialize aliases documentation", "serde.rs"),
        ("cargo build --locked documentation", "rust-lang.org"),
    ] {
        let started = Instant::now();
        let payload = tool.run(json!({"query":query,"max_results":5})).await.unwrap();
        let useful = payload["results"].as_array().is_some_and(|results| {
            results.iter().any(|hit| {
                Url::parse(hit["url"].as_str().unwrap_or_default()).ok().is_some_and(|url| {
                    url.host_str()
                        .is_some_and(|host| host == useful_host || host.ends_with(&format!(".{useful_host}")))
                })
            })
        });
        println!(
            "Pilot: {query}; latency_ms={}; count={}; useful_primary_source={useful}; error_type={}",
            started.elapsed().as_millis(),
            payload["count"],
            payload["error_type"]
        );
        if payload.get("error").is_some() {
            break;
        }
    }
}
