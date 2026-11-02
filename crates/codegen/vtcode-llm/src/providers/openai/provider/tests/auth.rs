use super::*;

#[test]
fn openai_auth_backend_setup_uses_api_key_base_url_and_static_auth() {
    let setup = OpenAIBackendSetup::from_api_key_config(None);
    let transport = setup.transport();
    let responses = setup.responses_defaults();

    assert_eq!(setup.base_url(), urls::OPENAI_API_BASE);
    assert!(matches!(setup.kind(), OpenAIBackendKind::ApiKey));
    assert_eq!(setup.refresh_behaviour(), OpenAIBackendRefreshBehaviour::StaticBearer);
    assert!(transport.websocket);
    assert!(transport.chat_completions_fallback);
    assert!(transport.responses_compaction_endpoint);
    assert!(!responses.force_store_false);
    assert!(responses.include_prompt_cache_retention);
    assert!(!responses.include_encrypted_reasoning);
}

#[test]
fn chatgpt_auth_backend_setup_uses_rig_chatgpt_by_default() {
    let setup = OpenAIBackendSetup::from_chatgpt_subscription_config(None);
    let transport = setup.transport();
    let responses = setup.responses_defaults();

    assert_eq!(setup.base_url(), CHATGPT_CODEX_BASE);
    assert!(matches!(
        setup.kind(),
        OpenAIBackendKind::ChatGptSubscription(ChatGptSubscriptionAuthSource::RigChatGpt)
    ));
    assert_eq!(setup.refresh_behaviour(), OpenAIBackendRefreshBehaviour::RefreshableChatGptSession);
    assert!(!transport.websocket);
    assert!(!transport.chat_completions_fallback);
    assert!(!transport.responses_compaction_endpoint);
    assert!(responses.force_store_false);
    assert!(!responses.include_prompt_cache_retention);
    assert!(responses.include_encrypted_reasoning);
    assert!(!responses.include_structured_history_in_input);
    assert!(responses.preserve_structured_history_on_replay);
}

#[test]
fn openai_base_url_override_applies_to_api_key_and_subscription_paths() {
    // Locks `OPENAI_BASE_URL` handling on both OpenAI backends (discussion #764):
    // config `base_url` wins, then `OPENAI_BASE_URL`, then the built-in default.
    // Keeping the override on the ChatGPT-subscription path is intentional so a
    // local recording proxy can capture/replay a session without code changes
    // or TLS interception.
    let env = vtcode_commons::env_lock::lock();
    let previous = std::env::var_os(env_vars::OPENAI_BASE_URL);
    env.set_var(env_vars::OPENAI_BASE_URL, "http://127.0.0.1:8931/v1");

    let api_key_setup = OpenAIBackendSetup::from_api_key_config(None);
    assert_eq!(api_key_setup.base_url(), "http://127.0.0.1:8931/v1");
    assert!(matches!(api_key_setup.kind(), OpenAIBackendKind::ApiKey));

    let subscription_setup = OpenAIBackendSetup::from_chatgpt_subscription_config(None);
    assert_eq!(subscription_setup.base_url(), "http://127.0.0.1:8931/v1");
    assert!(matches!(
        subscription_setup.kind(),
        OpenAIBackendKind::ChatGptSubscription(ChatGptSubscriptionAuthSource::RigChatGpt)
    ));

    let explicit_api_key = OpenAIBackendSetup::from_api_key_config(Some("https://proxy.example/v1".to_string()));
    assert_eq!(explicit_api_key.base_url(), "https://proxy.example/v1");

    let explicit_subscription =
        OpenAIBackendSetup::from_chatgpt_subscription_config(Some("https://proxy.example/codex".to_string()));
    assert_eq!(explicit_subscription.base_url(), "https://proxy.example/codex");

    env.restore_var(env_vars::OPENAI_BASE_URL, previous);
}

#[test]
fn chatgpt_auth_backend_setup_keeps_compatibility_auth_behind_boundary() {
    let setup = OpenAIBackendSetup::chatgpt_subscription_compatibility(CHATGPT_CODEX_BASE.to_string());

    assert_eq!(
        setup.kind(),
        &OpenAIBackendKind::ChatGptSubscription(ChatGptSubscriptionAuthSource::CodexAppServerCompatibility)
    );
    assert!(
        setup.uses_refreshable_auth(),
        "ChatGPT compatibility auth must remain refreshable behind the backend setup boundary"
    );

    let auth = setup.request_auth_from_session(OpenAIChatGptSession {
        openai_api_key: "exchanged-api-key".to_string(),
        id_token: "id-token".to_string(),
        access_token: "oauth-access".to_string(),
        refresh_token: "refresh-token".to_string(),
        account_id: Some("acc_123".to_string()),
        email: Some("test@example.com".to_string()),
        plan: Some("plus".to_string()),
        obtained_at: 1,
        refreshed_at: 1,
        expires_at: None,
    });
    assert_eq!(auth.bearer_token, "oauth-access");
    assert!(auth.rig_chatgpt_auth.is_none(), "compatibility fallback should not masquerade as Rig-backed auth");
}

#[test]
fn chatgpt_auth_backend_setup_applies_account_headers_without_session_id() {
    let setup = OpenAIBackendSetup::from_chatgpt_subscription_config(None);
    let auth = setup.request_auth_from_session(OpenAIChatGptSession {
        openai_api_key: "exchanged-api-key".to_string(),
        id_token: "id-token".to_string(),
        access_token: "oauth-access".to_string(),
        refresh_token: "refresh-token".to_string(),
        account_id: Some("acc_123".to_string()),
        email: Some("test@example.com".to_string()),
        plan: Some("plus".to_string()),
        obtained_at: 1,
        refreshed_at: 1,
        expires_at: None,
    });
    assert_eq!(auth.bearer_token, "oauth-access");
    assert_eq!(auth.chatgpt_account_id.as_deref(), Some("acc_123"));
    assert!(matches!(
        auth.rig_chatgpt_auth.as_ref(),
        Some(RigChatGptAuth::AccessToken {
            access_token,
            account_id: Some(account_id),
        }) if access_token == "oauth-access" && account_id.as_str() == "acc_123"
    ));

    let request = setup
        .authorize_request(
            reqwest::Client::builder()
                .no_proxy()
                .build()
                .expect("test client")
                .get("http://example.com"),
            &auth,
        )
        .build()
        .expect("request should build");

    assert_eq!(request.headers().get("authorization").and_then(|v| v.to_str().ok()), Some("Bearer oauth-access"));
    assert_eq!(request.headers().get("ChatGPT-Account-Id").and_then(|v| v.to_str().ok()), Some("acc_123"));
    assert_eq!(request.headers().get("originator").and_then(|v| v.to_str().ok()), Some("codex_cli_rs"));
    assert!(
        request.headers().get("session_id").is_none(),
        "ChatGPT subscription auth must not forward VT_SESSION_ID or any other conversation/session identifier"
    );
}

#[tokio::test]
async fn chatgpt_backend_uses_oauth_access_token_and_account_header() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let provider = OpenAIProvider::new_with_client(
        "api-key".to_string(),
        Some(sample_chatgpt_auth_handle()),
        models::openai::GPT_5.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client"),
        chatgpt_mock_base_url(&server),
        TimeoutsConfig::default(),
    );
    assert!(matches!(
        provider.backend_setup.kind(),
        OpenAIBackendKind::ChatGptSubscription(ChatGptSubscriptionAuthSource::RigChatGpt)
    ));

    let auth = provider.request_auth_from_session(OpenAIChatGptSession {
        openai_api_key: "exchanged-api-key".to_string(),
        id_token: "id-token".to_string(),
        access_token: "oauth-access".to_string(),
        refresh_token: "refresh-token".to_string(),
        account_id: Some("acc_123".to_string()),
        email: Some("test@example.com".to_string()),
        plan: Some("plus".to_string()),
        obtained_at: 1,
        refreshed_at: 1,
        expires_at: None,
    });
    assert_eq!(auth.bearer_token, "oauth-access");
    assert_eq!(auth.chatgpt_account_id.as_deref(), Some("acc_123"));

    let request = provider
        .authorize_with_api_key(provider.http_client.get("http://example.com"), &auth)
        .build()
        .expect("request should build");
    assert_eq!(request.headers().get("authorization").and_then(|v| v.to_str().ok()), Some("Bearer oauth-access"));
    assert_eq!(request.headers().get("ChatGPT-Account-Id").and_then(|v| v.to_str().ok()), Some("acc_123"));
}

#[tokio::test]
async fn external_chatgpt_auth_retries_with_refreshed_tokens_after_401() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let refresh_calls = Arc::new(Mutex::new(0usize));
    let seen_bearer_tokens = Arc::new(Mutex::new(Vec::new()));

    Mock::given(method("GET"))
        .and(path("/auth-retry"))
        .respond_with(mock_retry_by_call_count(Arc::clone(&seen_bearer_tokens)))
        .expect(2)
        .mount(&server)
        .await;

    let provider = OpenAIProvider::new_with_client(
        "api-key".to_string(),
        Some(OpenAIChatGptAuthHandle::new_external(
            OpenAIChatGptSession {
                openai_api_key: String::new(),
                id_token: "id-token".to_string(),
                access_token: "oauth-access".to_string(),
                refresh_token: String::new(),
                account_id: Some("acc_123".to_string()),
                email: Some("test@example.com".to_string()),
                plan: Some("plus".to_string()),
                obtained_at: 1,
                refreshed_at: u64::MAX / 2,
                expires_at: None,
            },
            true,
            Arc::new(ExternalSessionRefresher { calls: Arc::clone(&refresh_calls) }),
        )),
        models::openai::GPT_5.to_string(),
        reqwest::Client::builder().no_proxy().build().expect("test client"),
        chatgpt_mock_base_url(&server),
        TimeoutsConfig::default(),
    );

    let response = provider
        .send_authorized(|auth| {
            provider
                .authorize_with_api_key(provider.http_client.get(format!("{}{}", server.uri(), "/auth-retry")), auth)
        })
        .await
        .expect("request should succeed after refresh");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        seen_bearer_tokens.lock().expect("mutex not poisoned").as_slice(),
        &[
            "Bearer oauth-access".to_string(),
            "Bearer oauth-access-refreshed".to_string()
        ]
    );
    assert_eq!(*refresh_calls.lock().expect("mutex not poisoned"), 1);
}

#[tokio::test]
async fn custom_provider_auth_retries_with_refreshed_tokens_after_401() {
    let Some(server) = start_mock_server_or_skip().await else {
        return;
    };
    let tempdir = TempDir::new().expect("tempdir");
    let seen_bearer_tokens = Arc::new(Mutex::new(Vec::new()));

    Mock::given(method("GET"))
        .and(path("/custom-auth-retry"))
        .respond_with(mock_retry_by_call_count(Arc::clone(&seen_bearer_tokens)))
        .expect(2)
        .mount(&server)
        .await;

    let provider = OpenAIProvider::from_custom_config(
        "mycorp".to_string(),
        "MyCorp".to_string(),
        None,
        Some(models::openai::GPT_5.to_string()),
        Some(native_openai_mock_base_url(&server)),
        None,
        None,
        None,
        None,
        Some(CustomProviderAuthHandle::new(
            custom_provider_auth_fixture(&tempdir, &["first-token", "second-token"]),
            None,
        )),
        None,
    );

    let response = provider
        .send_authorized(|auth| {
            provider.authorize_with_api_key(
                provider.http_client.get(format!("{}{}", server.uri(), "/custom-auth-retry")),
                auth,
            )
        })
        .await
        .expect("request should succeed after command-auth refresh");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        seen_bearer_tokens.lock().expect("mutex not poisoned").as_slice(),
        &["Bearer first-token".to_string(), "Bearer second-token".to_string()]
    );
}

#[test]
fn openai_request_auth_debug_redacts_bearer_token() {
    use super::super::super::backend_setup::OpenAIRequestAuth;

    let auth = OpenAIRequestAuth::bearer_token("sk-secret-bearer-token".to_string());
    let debug_str = format!("{auth:?}");
    assert!(!debug_str.contains("sk-secret-bearer-token"), "bearer token leaked in Debug: {debug_str}");
}

#[test]
fn openai_request_auth_debug_redacts_rig_chatgpt_auth() {
    use super::super::super::backend_setup::OpenAIRequestAuth;

    let auth = OpenAIRequestAuth {
        bearer_token: "rig-secret-access".to_string(),
        chatgpt_account_id: Some("acc_123".to_string()),
        rig_chatgpt_auth: Some(RigChatGptAuth::AccessToken {
            access_token: "rig-secret-access".to_string(),
            account_id: Some("acc_123".to_string()),
        }),
    };
    let debug_str = format!("{auth:?}");
    assert!(!debug_str.contains("rig-secret-access"), "rig access token leaked in Debug: {debug_str}");
    // Non-secret metadata should still be visible.
    assert!(debug_str.contains("acc_123"), "account_id should be visible: {debug_str}");
}
