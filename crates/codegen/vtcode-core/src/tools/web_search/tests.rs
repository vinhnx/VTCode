use super::parser::parse_duckduckgo_response;
use super::*;
use vtcode_config::WebSearchProvider;

#[test]
fn missing_query_is_rejected() {
    let tool = WebSearchTool::new();
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(tool.run(json!({ "max_results": 5 })));
    assert!(result.is_err());
}

#[test]
fn oversized_query_is_rejected_before_network_admission() {
    let tool = WebSearchTool::new();
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(tool.run(json!({ "query": "x".repeat(MAX_QUERY_CHARS + 1) })));
    assert!(result.is_err());
    assert_eq!(tool.state.lock().unwrap().requests_made, 0);
}

#[test]
fn pattern_is_rejected_as_an_unknown_field() {
    let result = serde_json::from_value::<WebSearchArgs>(json!({
        "query": "vinhnx",
        "pattern": "legacy"
    }));
    assert!(result.is_err());
}

#[test]
fn canonical_query_and_max_results_are_accepted() {
    let args = serde_json::from_value::<WebSearchArgs>(json!({
        "query": "vinhnx",
        "max_results": 5
    }))
    .expect("canonical web_search arguments");
    assert_eq!(args.query, "vinhnx");
    assert_eq!(args.max_results, Some(5));
}

#[test]
fn truncate_chars_appends_ellipsis() {
    assert_eq!(truncate_chars("hello world", 5), "hello…");
    assert_eq!(truncate_chars("hi", 5), "hi");
}

#[test]
fn challenge_response_is_rejected_before_empty_results_are_cached() {
    let challenge = r#"<html><body>Anomaly detected.</body></html>"#;
    let error = match parse_duckduckgo_response(challenge, 10) {
        Ok(_) => panic!("challenge pages must be rejected"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("anti-bot challenge"));

    let empty = "<html><body><p>No results found.</p></body></html>";
    assert!(parse_duckduckgo_response(empty, 10).unwrap().is_empty());
}

#[test]
fn session_cap_short_circuits_with_structured_error() {
    let config = WebSearchConfig {
        provider: Default::default(),
        max_results: 5,
        timeout_secs: 20,
        cooldown_ms: 0,
        cache_ttl_secs: 300,
        session_max_requests: 2,
    };
    let tool = WebSearchTool::with_config(config);
    // Bump the counter without going to the network.
    {
        let mut state = tool.state.lock().unwrap();
        state.requests_made = 2;
        state.last_request_at = Some(Instant::now());
    }
    let payload = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(tool.run(json!({ "query": "rust" })))
        .expect("cap should be a structured JSON, not a runtime error");
    assert_eq!(payload["error"], "web_search session request cap reached");
    assert_eq!(payload["provider"], WebSearchProvider::Auto.backend_id());
    assert_eq!(payload["error_type"], "session_cap");
    assert_eq!(payload["retryable"], false);
    assert_eq!(payload["session_max_requests"], 2);
    assert_eq!(payload["request_budget_remaining"], 0);
}

#[test]
fn request_admission_reserves_before_network_and_enforces_cap() {
    let tool = WebSearchTool::new();
    let provider = WebSearchProvider::Auto.backend_id();
    assert!(tool.admit_request("first", provider, Duration::ZERO, 1).unwrap().is_none());

    let response = tool
        .admit_request("second", provider, Duration::ZERO, 1)
        .unwrap()
        .expect("the second concurrent admission must hit the cap");
    assert_eq!(response["error"], "web_search session request cap reached");
    assert_eq!(response["provider"], provider);
    assert_eq!(response["error_type"], "session_cap");
    assert_eq!(response["retryable"], false);
    assert_eq!(response["session_max_requests"], 1);
    assert_eq!(response["request_budget_remaining"], 0);
}

#[test]
fn cooldown_short_circuits_with_retry_after() {
    let config = WebSearchConfig {
        provider: Default::default(),
        max_results: 5,
        timeout_secs: 20,
        cooldown_ms: 5_000,
        cache_ttl_secs: 300,
        session_max_requests: 100,
    };
    let tool = WebSearchTool::with_config(config);
    {
        let mut state = tool.state.lock().unwrap();
        state.requests_made = 0;
        state.last_request_at = Some(Instant::now());
    }
    let payload = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(tool.run(json!({ "query": "rust" })))
        .expect("cooldown should be a structured JSON");
    assert_eq!(payload["error"], "web_search cooldown active");
    assert_eq!(payload["provider"], WebSearchProvider::Auto.backend_id());
    assert_eq!(payload["error_type"], "cooldown");
    assert_eq!(payload["retryable"], true);
    assert!(payload["retry_after_ms"].as_u64().unwrap() > 0);
}

#[test]
fn result_payloads_report_the_canonical_backend() {
    let provider = WebSearchProvider::Auto.backend_id();
    let success = results_response(
        "rust",
        provider,
        vec![SearchResult::new(
            "Rust Book".to_owned(),
            "https://doc.rust-lang.org/book/".to_owned(),
            "Ownership and borrowing".to_owned(),
        )],
    );
    let empty = results_response("no matching result", provider, Vec::new());

    assert_eq!(success["provider"], WebSearchProvider::Duckduckgo.backend_id());
    assert_eq!(success["count"], 1);
    assert!(success.get("warning").is_none());
    assert_eq!(empty["provider"], WebSearchProvider::Duckduckgo.backend_id());
    assert_eq!(empty["count"], 0);
    assert_eq!(empty["results"], json!([]));
    assert_eq!(
        empty["warning"],
        "No results were returned. DuckDuckGo may have rate-limited the request or matched nothing. Try a different query, or wait a few seconds and try again."
    );
}

#[test]
fn cache_serves_repeat_queries_without_network() {
    // Pin max_results so the cache key matches the one the tool builds.
    let tool = WebSearchTool::with_config(WebSearchConfig {
        provider: Default::default(),
        max_results: 5,
        ..WebSearchConfig::default()
    });
    let cached_payload = json!({
        "query": "rust",
        "provider": WebSearchProvider::Duckduckgo.backend_id(),
        "count": 1,
        "results": [{
            "title": "Cached Result",
            "url": "https://example.com/cached",
            "snippet": "from cache"
        }]
    });
    {
        let mut state = tool.state.lock().unwrap();
        state.cache_put("5::rust".to_string(), cached_payload.clone());
    }
    let payload = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(tool.run(json!({ "query": "rust" })))
        .expect("cache hit must not error");
    assert_eq!(payload["cached"], json!(true));
    assert_eq!(payload["provider"], WebSearchProvider::Auto.backend_id());
    assert_eq!(payload["count"], 1);
    assert_eq!(payload["results"][0]["title"], "Cached Result");
}

#[test]
fn classify_search_error_flags_antiban_statuses() {
    for status in [202, 403, 429] {
        let classification = classify_search_error(&format!(
            "DuckDuckGo declined the request (HTTP {status}), likely an anti-bot challenge for this network."
        ));
        assert_eq!(classification.error_type, "antiban_blocked");
        assert!(!classification.retryable);
        assert!(classification.next_action.contains("Do not retry immediately"));
    }
}

#[test]
fn challenge_html_is_antiban_and_non_retryable() {
    let error = match parse_duckduckgo_response("<html><body><div class=\"captcha\">verify</div></body></html>", 10) {
        Ok(_) => panic!("challenge HTML must not become an empty success"),
        Err(error) => error,
    };
    let classification = classify_search_error(&error.to_string());
    assert_eq!(classification.error_type, "antiban_blocked");
    assert!(!classification.retryable);
}

#[test]
fn classify_search_error_flags_timeout_as_network_error() {
    let classification = classify_search_error("request timed out after 20s");
    assert_eq!(classification.error_type, "network_error");
    assert!(classification.retryable);
    assert!(classification.next_action.contains("Retry once"));

    let payload = search_error_response("rust", WebSearchProvider::Auto.backend_id(), "request timed out after 20s");
    assert_eq!(payload["provider"], WebSearchProvider::Duckduckgo.backend_id());
    assert_eq!(payload["error_type"], "network_error");
    assert_eq!(payload["retryable"], true);
}

#[test]
fn classify_search_error_flags_five_hundred_as_retryable_upstream_error() {
    let classification = classify_search_error("DuckDuckGo returned HTTP 503");
    assert_eq!(classification.error_type, "upstream_error");
    assert!(classification.retryable);
}

#[test]
fn classify_search_error_flags_oversized_response() {
    let classification = classify_search_error("DuckDuckGo response body exceeded the 1000000-byte limit");
    assert_eq!(classification.error_type, "response_too_large");
    assert!(!classification.retryable);
    assert!(classification.next_action.contains("Narrow the query"));
}
