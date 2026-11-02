use super::*;

#[test]
fn credential_overview_carries_no_token_fields() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");
    let overview = summarize_openai_credentials(
        &OpenAIAuthConfig::default(),
        AuthCredentialsStoreMode::File,
        Some("sk-overview-key".to_string()),
    )
    .expect("overview");
    // The overview is a display struct — verify it only carries metadata,
    // not the raw session or token strings.
    let debug_str = format!("{overview:?}");
    assert!(!debug_str.contains("oauth-access"), "access_token leaked: {debug_str}");
    assert!(!debug_str.contains("refresh-token"), "refresh_token leaked: {debug_str}");
    assert!(!debug_str.contains("api-key"), "openai_api_key leaked: {debug_str}");
    // The overview should not contain the raw API key value either.
    assert!(!debug_str.contains("sk-overview-key"), "api key value leaked: {debug_str}");
    clear_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("clear");
}

// ── Partial refresh response merge tests ──
//
// These test merge_refresh_response directly with resp.id_token = None,
// which skips the HTTP API-key exchange (has_new_id_token = false).
// This lets us verify field-preservation behavior without network access.

#[tokio::test]
async fn merge_preserves_omitted_access_token() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: None,
        refresh_token: Some("new-refresh".to_string()),
        expires_in: Some(3600),
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.access_token, current.access_token, "omitted access_token should be preserved");
    assert_eq!(merged.refresh_token, "new-refresh");
}

#[tokio::test]
async fn merge_preserves_omitted_refresh_token() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("new-access".to_string()),
        refresh_token: None,
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.refresh_token, current.refresh_token, "omitted refresh_token should be preserved");
    assert_eq!(merged.access_token, "new-access");
}

#[tokio::test]
async fn merge_preserves_omitted_id_token_and_api_key() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("new-access".to_string()),
        refresh_token: None,
        expires_in: Some(1800),
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    // No new id_token → old id_token and api_key preserved, no HTTP exchange.
    assert_eq!(merged.id_token, current.id_token, "omitted id_token should be preserved");
    assert_eq!(merged.openai_api_key, current.openai_api_key, "api_key should be preserved without new id_token");
    // Email/plan/account_id also preserved without a new id_token.
    assert_eq!(merged.email, current.email);
    assert_eq!(merged.plan, current.plan);
}

#[tokio::test]
async fn merge_all_omitted_preserves_everything() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: None,
        refresh_token: None,
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.id_token, current.id_token);
    assert_eq!(merged.access_token, current.access_token);
    assert_eq!(merged.refresh_token, current.refresh_token);
    assert_eq!(merged.openai_api_key, current.openai_api_key);
    assert_eq!(merged.email, current.email);
    // expires_in is None and no new access_token → old expiry preserved.
    assert_eq!(merged.expires_at, current.expires_at);
}

#[tokio::test]
async fn merge_new_access_token_updates_bearer_without_id_token() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("replaced-access".to_string()),
        refresh_token: None,
        expires_in: Some(7200),
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.access_token, "replaced-access");
    // active_api_bearer_token should use the exchanged api_key (unchanged)
    // because no new id_token was provided.
    assert_eq!(active_api_bearer_token(&merged), current.openai_api_key);
}

// ── Blank-string refresh field tests ──
//
// Some token endpoints return empty strings for omitted fields rather than
// leaving them out. These verify that blank strings are treated as omitted.

#[tokio::test]
async fn merge_treats_blank_access_token_as_omitted() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("   ".to_string()),
        refresh_token: Some("new-refresh".to_string()),
        expires_in: Some(3600),
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.access_token, current.access_token, "blank access_token should be treated as omitted");
    assert_eq!(merged.refresh_token, "new-refresh");
}

#[tokio::test]
async fn merge_treats_blank_refresh_token_as_omitted() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("new-access".to_string()),
        refresh_token: Some(String::new()),
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.refresh_token, current.refresh_token, "blank refresh_token should be treated as omitted");
    assert_eq!(merged.access_token, "new-access");
}

#[tokio::test]
async fn merge_treats_blank_id_token_as_omitted() {
    let current = sample_session();
    let resp = OpenAIRefreshResponse {
        id_token: Some("  ".to_string()),
        access_token: Some("new-access".to_string()),
        refresh_token: None,
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    // Blank id_token → treated as omitted → no HTTP exchange, old preserved.
    assert_eq!(merged.id_token, current.id_token, "blank id_token should be treated as omitted");
    assert_eq!(merged.openai_api_key, current.openai_api_key, "api_key preserved when id_token is blank");
}

// ── Opaque access token expiry tests ──

#[tokio::test]
async fn merge_changed_opaque_access_token_clears_stale_expiry() {
    let mut current = sample_session();
    current.expires_at = Some(now_secs() + 3600); // old expiry for old token

    // New access token without expires_in and without a parseable JWT exp.
    // "opaque-new-token" is not a JWT, so parse_jwt_exp returns None.
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("opaque-new-token".to_string()),
        refresh_token: None,
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.access_token, "opaque-new-token");
    // Changed token without expires_in or JWT exp → expiry must be None,
    // NOT the old token's expiry.
    assert_eq!(merged.expires_at, None, "changed opaque access token must not inherit old token's expiry");
}

#[tokio::test]
async fn merge_repeated_opaque_access_token_preserves_expiry() {
    let old_expiry = now_secs() + 3600;
    let mut current = sample_session();
    current.access_token = "opaque-same-token".to_string();
    current.expires_at = Some(old_expiry);

    // The endpoint repeats the SAME opaque access token without expires_in.
    // Since the token didn't change, the old expiry is still valid.
    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: Some("opaque-same-token".to_string()),
        refresh_token: Some("new-refresh".to_string()),
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    assert_eq!(merged.access_token, "opaque-same-token");
    assert_eq!(merged.expires_at, Some(old_expiry), "repeated same opaque access token must preserve old expiry");
}

#[tokio::test]
async fn merge_omitted_access_token_preserves_old_expiry() {
    let old_expiry = now_secs() + 3600;
    let mut current = sample_session();
    current.expires_at = Some(old_expiry);

    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: None,
        refresh_token: Some("new-refresh".to_string()),
        expires_in: None,
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    // No new access_token and no expires_in → old expiry preserved.
    assert_eq!(merged.expires_at, Some(old_expiry), "omitted access_token should preserve old expiry");
}

#[tokio::test]
async fn merge_expires_in_overrides_for_omitted_access_token() {
    let old_expiry = now_secs() + 3600;
    let mut current = sample_session();
    current.expires_at = Some(old_expiry);

    let resp = OpenAIRefreshResponse {
        id_token: None,
        access_token: None,
        refresh_token: None,
        expires_in: Some(1800),
    };
    let merged = merge_refresh_response(&current, resp).await.expect("merge");
    // expires_in takes priority over old expiry even without a new access_token.
    assert_ne!(merged.expires_at, Some(old_expiry));
    assert!(merged.expires_at.is_some());
}

// ── Error classification tests (pure, no network) ──

#[test]
fn extract_error_code_flat_form() {
    assert_eq!(extract_error_code(r#"{"error": "invalid_grant"}"#), "invalid_grant");
}

#[test]
fn extract_error_code_nested_form() {
    let body = r#"{"error": {"code": "refresh_token_expired", "message": "token expired"}}"#;
    assert_eq!(extract_error_code(body), "refresh_token_expired");
}

#[test]
fn extract_error_code_nested_form_does_not_fall_back_to_message() {
    // A descriptive message that happens to contain a terminal code string
    // must NOT be treated as a structured error code. Only `code` and
    // `type` fields are recognized — `message` is free-text.
    let body = r#"{"error": {"message": "invalid_grant"}}"#;
    assert_eq!(extract_error_code(body), "", "message field should not be used as error code: {body}");
}

#[test]
fn extract_error_code_empty_body() {
    assert_eq!(extract_error_code(""), "");
}

#[test]
fn extract_error_code_non_json_body() {
    assert_eq!(extract_error_code("Internal Server Error"), "");
}

#[test]
fn extract_error_code_blank_error_field() {
    assert_eq!(extract_error_code(r#"{"error": "  "}"#), "");
}

#[test]
fn classify_refresh_status_error_invalid_grant_clears_session() {
    let _guard = TestAuthDirGuard::new();
    // Store a session so we can verify it gets cleared.
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load")
            .is_some()
    );

    let err = classify_refresh_status_error(reqwest::StatusCode::BAD_REQUEST, r#"{"error": "invalid_grant"}"#);
    assert!(err.to_string().contains("session expired"));

    // Session should have been cleared.
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load")
            .is_none()
    );
}

#[test]
fn classify_refresh_status_error_nested_refresh_token_expired_clears_session() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");

    let err = classify_refresh_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": {"code": "refresh_token_expired", "message": "The refresh token has expired"}}"#,
    );
    assert!(err.to_string().contains("session expired"));

    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load")
            .is_none()
    );
}

#[test]
fn classify_refresh_status_error_unauthorized_preserves_session() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");

    let err = classify_refresh_status_error(reqwest::StatusCode::UNAUTHORIZED, "");
    assert!(err.to_string().contains("HTTP 401"), "should report HTTP 401: {err}");
    // 401 without a confirmed terminal code preserves the session.
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load")
            .is_some(),
        "session should be preserved on ambiguous 401"
    );
}

#[test]
fn classify_refresh_status_error_server_error_does_not_clear_session() {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");

    let err =
        classify_refresh_status_error(reqwest::StatusCode::INTERNAL_SERVER_ERROR, r#"{"error": "internal_error"}"#);
    assert!(err.to_string().contains("HTTP 500"));
    // Transient error → session preserved for retry.
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load")
            .is_some()
    );
}

#[test]
fn classify_refresh_status_error_never_includes_raw_body() {
    let _guard = TestAuthDirGuard::new();
    let err = classify_refresh_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": "invalid_grant", "sensitive_data": "secret-leak-attempt"}"#,
    );
    assert!(!err.to_string().contains("secret-leak-attempt"), "raw body leaked into error: {err}");
}

// ── Table-driven classification matrix (status × body shape × expected) ──

/// Expected outcome of `classify_refresh_status_error`.
#[derive(Debug, PartialEq)]
enum ClassifyOutcome {
    /// Session was cleared (terminal grant error).
    Terminal,
    /// Session was preserved; error message contains this fragment.
    Preserved(&'static str),
}

fn run_classify_matrix(status: reqwest::StatusCode, body: &str, expected: ClassifyOutcome) {
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");

    let err = classify_refresh_status_error(status, body);
    let msg = err.to_string();
    let loaded = load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File).expect("load");

    match expected {
        ClassifyOutcome::Terminal => {
            assert!(msg.contains("session expired"), "terminal error should say 'session expired': {msg}");
            assert!(loaded.is_none(), "session should be cleared for terminal: {status} {body}");
        }
        ClassifyOutcome::Preserved(frag) => {
            assert!(!msg.contains("session expired"), "non-terminal should not say 'session expired': {msg}");
            assert!(loaded.is_some(), "session should be preserved for: {status} {body}");
            if !frag.is_empty() {
                assert!(msg.contains(frag), "error should contain '{frag}': {msg}");
            }
        }
    }
}

#[test]
fn classify_matrix_terminal_400_flat() {
    run_classify_matrix(reqwest::StatusCode::BAD_REQUEST, r#"{"error": "invalid_grant"}"#, ClassifyOutcome::Terminal);
}

#[test]
fn classify_matrix_terminal_400_nested_code() {
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": {"code": "invalid_grant", "message": "..."}}"#,
        ClassifyOutcome::Terminal,
    );
}

#[test]
fn classify_matrix_terminal_400_nested_type() {
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": {"type": "invalid_grant", "message": "..."}}"#,
        ClassifyOutcome::Terminal,
    );
}

#[test]
fn classify_matrix_terminal_400_toplevel_code() {
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"code": "refresh_token_revoked", "message": "..."}"#,
        ClassifyOutcome::Terminal,
    );
}

#[test]
fn classify_matrix_terminal_401_flat() {
    // Terminal codes on 401 should also clear the session.
    run_classify_matrix(reqwest::StatusCode::UNAUTHORIZED, r#"{"error": "invalid_token"}"#, ClassifyOutcome::Terminal);
}

#[test]
fn classify_matrix_terminal_401_nested() {
    run_classify_matrix(
        reqwest::StatusCode::UNAUTHORIZED,
        r#"{"error": {"code": "refresh_token_expired"}}"#,
        ClassifyOutcome::Terminal,
    );
}

#[test]
fn classify_matrix_terminal_401_refresh_token_invalidated() {
    run_classify_matrix(
        reqwest::StatusCode::UNAUTHORIZED,
        r#"{"error": "refresh_token_invalidated"}"#,
        ClassifyOutcome::Terminal,
    );
}

#[test]
fn classify_matrix_terminal_400_toplevel_refresh_token_reused() {
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"code": "refresh_token_reused", "message": "..."}"#,
        ClassifyOutcome::Terminal,
    );
}

#[test]
fn classify_matrix_message_only_does_not_clear_session() {
    // A body with only a free-text "message" field (no structured code/type)
    // must NOT be treated as terminal, even if the message text happens to
    // contain a terminal code string. This prevents false-positive session
    // clearing from descriptive error messages.
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": {"message": "invalid_grant"}}"#,
        ClassifyOutcome::Preserved("HTTP 400"),
    );
}

#[test]
fn classify_matrix_invalid_client_preserves() {
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": "invalid_client"}"#,
        ClassifyOutcome::Preserved("invalid_client"),
    );
}

#[test]
fn classify_matrix_invalid_client_401_preserves() {
    run_classify_matrix(
        reqwest::StatusCode::UNAUTHORIZED,
        r#"{"error": "invalid_client"}"#,
        ClassifyOutcome::Preserved("invalid_client"),
    );
}

#[test]
fn classify_matrix_429_throttling_preserves() {
    run_classify_matrix(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        r#"{"error": "rate_limited"}"#,
        ClassifyOutcome::Preserved("rate-limited"),
    );
}

#[test]
fn classify_matrix_500_server_error_preserves() {
    run_classify_matrix(
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        r#"{"error": "internal_error"}"#,
        ClassifyOutcome::Preserved("HTTP 500"),
    );
}

#[test]
fn classify_matrix_502_bad_gateway_preserves() {
    run_classify_matrix(reqwest::StatusCode::BAD_GATEWAY, "", ClassifyOutcome::Preserved("HTTP 502"));
}

#[test]
fn classify_matrix_503_service_unavailable_preserves() {
    run_classify_matrix(reqwest::StatusCode::SERVICE_UNAVAILABLE, "", ClassifyOutcome::Preserved("HTTP 503"));
}

#[test]
fn classify_matrix_ambiguous_401_empty_body_preserves() {
    run_classify_matrix(reqwest::StatusCode::UNAUTHORIZED, "", ClassifyOutcome::Preserved("HTTP 401"));
}

#[test]
fn classify_matrix_400_nonterminal_code_preserves() {
    // A 400 with a code that is NOT in the terminal list should preserve.
    run_classify_matrix(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": "some_unknown_error"}"#,
        ClassifyOutcome::Preserved("HTTP 400"),
    );
}

#[test]
fn classify_matrix_never_leaks_body_in_any_branch() {
    // Terminal branch
    let _guard = TestAuthDirGuard::new();
    let err = classify_refresh_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": "invalid_grant", "leak": "TERMINAL_LEAK"}"#,
    );
    assert!(!err.to_string().contains("TERMINAL_LEAK"));

    // Preserved branch (invalid_client)
    let err = classify_refresh_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": "invalid_client", "leak": "CLIENT_LEAK"}"#,
    );
    assert!(!err.to_string().contains("CLIENT_LEAK"));

    // Preserved branch (429)
    let err = classify_refresh_status_error(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        r#"{"error": "rate_limited", "leak": "THROTTLE_LEAK"}"#,
    );
    assert!(!err.to_string().contains("THROTTLE_LEAK"));
}

// ── Error code extraction shape tests ──

#[test]
fn extract_error_code_nested_with_type_field() {
    let body = r#"{"error": {"type": "invalid_grant", "message": "..."}}"#;
    assert_eq!(extract_error_code(body), "invalid_grant");
}

#[test]
fn extract_error_code_toplevel_code_field() {
    let body = r#"{"code": "refresh_token_expired", "message": "..."}"#;
    assert_eq!(extract_error_code(body), "refresh_token_expired");
}

#[test]
fn extract_error_code_case_insensitive_normalization() {
    assert_eq!(extract_error_code(r#"{"error": "INVALID_GRANT"}"#), "invalid_grant");
}

#[test]
fn extract_error_code_no_substring_matching() {
    // "invalid_grant_really" is not a terminal code — extract_error_code
    // returns it verbatim, but classify won't match it as terminal.
    let code = extract_error_code(r#"{"error": "invalid_grant_really_not_a_real_code"}"#);
    assert_eq!(code, "invalid_grant_really_not_a_real_code");
    // Verify classify does NOT treat this as terminal.
    let _guard = TestAuthDirGuard::new();
    let session = sample_session();
    save_openai_chatgpt_session_with_mode(&session, AuthCredentialsStoreMode::File).expect("save");
    let err = classify_refresh_status_error(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error": "invalid_grant_really_not_a_real_code"}"#,
    );
    assert!(!err.to_string().contains("session expired"));
    assert!(
        load_openai_chatgpt_session_with_mode(AuthCredentialsStoreMode::File)
            .expect("load")
            .is_some()
    );
}

// ── PKCE and callback Debug redaction tests ──

#[test]
fn pkce_challenge_debug_redacts_verifier() {
    let challenge = generate_pkce_challenge().expect("generate pkce");
    let debug_str = format!("{challenge:?}");
    assert!(!debug_str.contains(&challenge.code_verifier), "code_verifier leaked: {debug_str}");
    assert!(debug_str.contains("<redacted>"), "verifier should be redacted: {debug_str}");
    // Challenge and method are safe to display.
    assert!(debug_str.contains(&challenge.code_challenge), "code_challenge should be visible: {debug_str}");
}

#[test]
fn auth_callback_outcome_debug_redacts_code() {
    let outcome = AuthCallbackOutcome::Code("super-secret-auth-code".to_string());
    let debug_str = format!("{outcome:?}");
    assert!(!debug_str.contains("super-secret-auth-code"), "authorization code leaked: {debug_str}");
    assert!(debug_str.contains("<redacted>"), "code should be redacted: {debug_str}");
}

#[test]
fn auth_callback_outcome_debug_shows_cancelled_and_redacts_error() {
    let cancelled = format!("{:?}", AuthCallbackOutcome::Cancelled);
    assert!(cancelled.contains("Cancelled"));

    // Error messages from OAuth callbacks are untrusted query parameters
    // that may contain sensitive values — Debug must redact them.
    let error = format!("{:?}", AuthCallbackOutcome::Error("access_denied".to_string()));
    assert!(!error.contains("access_denied"), "error message leaked through Debug: {error}");
    assert!(error.contains("<redacted>"), "error should be redacted: {error}");
}
